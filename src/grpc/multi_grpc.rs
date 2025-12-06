use anyhow::{anyhow, Result};
use futures::{SinkExt, StreamExt};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc::UnboundedSender, Mutex};
use tracing::{info, debug, error, warn};
use yellowstone_grpc_client::GeyserGrpcClient;
use yellowstone_grpc_proto::geyser::{CommitmentLevel, SubscribeRequest, SubscribeRequestFilterAccounts, SubscribeRequestFilterAccountsFilter, SubscribeRequestFilterTransactions, SubscribeRequestPing, subscribe_update::UpdateOneof};

use crate::{config::{GrpcConfig, GrpcEndpoint}, grpc::ConnectionStatus};

pub struct MultiGrpc {
    pub config: GrpcConfig,
    pub request: Arc<Mutex<SubscribeRequest>>,
    pub sinks: Arc<Mutex<Vec<Option<UnboundedSender<SubscribeRequest>>>>>,
    pub ping_sent_at: Arc<Mutex<Vec<Option<std::time::Instant>>>>,
    pub ping_counter: Arc<Mutex<Vec<i32>>>,
}

impl MultiGrpc {
    pub fn new(config: GrpcConfig) -> Self {
        let num_endpoints = config.endpoints.len();
        Self {
            config,
            request: Arc::new(Mutex::new(SubscribeRequest {
                commitment: Some(CommitmentLevel::Confirmed.into()),
                ..Default::default()
            })),
            sinks: Arc::new(Mutex::new(vec![None; num_endpoints])),
            ping_sent_at: Arc::new(Mutex::new(vec![None; num_endpoints])),
            ping_counter: Arc::new(Mutex::new(vec![0; num_endpoints])),
        }
    }

    pub async fn connect<F>(&self, tx: &UnboundedSender<UpdateOneof>, status_cb: F) -> ()
    where
        F: Fn(usize, ConnectionStatus, u64) + Send + Sync + 'static + Clone,
    {
        let mut handles = vec![];

        for (idx, endpoint) in self.config.endpoints.iter().enumerate() {
            let endpoint = endpoint.clone();
            let tx = tx.clone();
            let request = self.request.clone();
            let sinks = self.sinks.clone();
            let ping_sent_at = self.ping_sent_at.clone();
            let ping_counter: Arc<Mutex<Vec<i32>>> = self.ping_counter.clone();
            let status_cb_clone = status_cb.clone();

            let handle = tokio::spawn(async move {
                Self::connect_endpoint(idx, endpoint, tx, request, sinks, ping_sent_at, ping_counter, status_cb_clone).await
            });

            handles.push(handle);
        }

        // Wait for all connection tasks to complete (they run indefinitely with retries)
        for handle in handles {
            if let Err(e) = handle.await {
                error!("Connection task panicked: {:?}", e);
            }
        }

        ()
    }

    async fn connect_endpoint<F>(
        idx: usize,
        endpoint: GrpcEndpoint,
        tx: UnboundedSender<UpdateOneof>,
        request: Arc<Mutex<SubscribeRequest>>,
        sinks: Arc<Mutex<Vec<Option<tokio::sync::mpsc::UnboundedSender<SubscribeRequest>>>>>,
        ping_sent_at: Arc<Mutex<Vec<Option<std::time::Instant>>>>,
        ping_counter: Arc<Mutex<Vec<i32>>>,
        status_cb: F,
    ) -> Result<()>
    where
        F: Fn(usize, ConnectionStatus, u64) + Send + Sync + 'static,
    {
        loop {
            info!("Connecting to endpoint {}: {}", idx, endpoint.url);

            match Self::try_connect_endpoint(
                idx,
                endpoint.clone(),
                tx.clone(),
                request.clone(),
                sinks.clone(),
                ping_sent_at.clone(),
                ping_counter.clone(),
                &status_cb,
            ).await {
                Ok(_) => {
                    warn!("Connection to endpoint {} ({}) closed, reconnecting...", idx, endpoint.url);
                }
                Err(e) => {
                    error!("Failed to connect to endpoint {} ({}): {:?}", idx, endpoint.url, e);
                }
            }

            // Clear the sink for this endpoint on disconnect
            sinks.lock().await[idx] = None;
            ping_sent_at.lock().await[idx] = None;

            // Wait before reconnecting
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }

    async fn try_connect_endpoint<F>(
        idx: usize,
        endpoint: GrpcEndpoint,
        tx: UnboundedSender<UpdateOneof>,
        request: Arc<Mutex<SubscribeRequest>>,
        sinks: Arc<Mutex<Vec<Option<tokio::sync::mpsc::UnboundedSender<SubscribeRequest>>>>>,
        ping_sent_at: Arc<Mutex<Vec<Option<std::time::Instant>>>>,
        ping_counter: Arc<Mutex<Vec<i32>>>,
        status_cb: &F,
    ) -> Result<()>
    where
        F: Fn(usize, ConnectionStatus, u64) + Send + Sync + 'static,
    {
        let mut builder = GeyserGrpcClient::build_from_shared(endpoint.url.clone())
            .map_err(|e| anyhow!("Failed to build gRPC client: {:?}", e))?;

        if let Some(token) = endpoint.x_token {
            builder = builder.x_token(Some(token))
                .map_err(|e| anyhow!("Failed to set x-token: {:?}", e))?;
        }
        status_cb(idx, ConnectionStatus::Connecting, 0);

        let mut client = builder.connect().await
            .map_err(|e| anyhow!("Failed to connect to gRPC endpoint: {:?}", e))?;

        status_cb(idx, ConnectionStatus::Connected, 0);

        info!("Connected to endpoint {}: {}", idx, endpoint.url);

        let (mut subscribe_sink, mut stream) = client.subscribe().await
            .map_err(|e| anyhow!("Failed to create subscription stream: {:?}", e))?;

        // Create a channel for resubscription
        let (resubscribe_tx, mut resubscribe_rx) = tokio::sync::mpsc::unbounded_channel();

        // Store the resubscribe sender
        sinks.lock().await[idx] = Some(resubscribe_tx);

        // Send initial subscription request
        let current_request = request.lock().await.clone();
        subscribe_sink.send(current_request).await
            .map_err(|e| anyhow!("Failed to send initial subscription: {:?}", e))?;

        info!("Subscription established for endpoint {}: {}", idx, endpoint.url);

        // Process stream and resubscription requests concurrently
        loop {
            tokio::select! {
                // Handle incoming messages from stream
                message = stream.next() => {
                    match message {
                        Some(Ok(update)) => {
                            if let Some(update_oneof) = update.update_oneof {
                                match update_oneof {
                                    UpdateOneof::Ping(_) => {
                                        // Server sent us a Ping - respond immediately and record timestamp
                                        debug!("Received Ping from endpoint {}", idx);

                                        // Increment our ping counter
                                        let mut counter = ping_counter.lock().await;
                                        counter[idx] += 1;
                                        let current_ping_id = counter[idx];
                                        drop(counter);

                                        // Record when we received the ping (to measure our pong roundtrip later)
                                        ping_sent_at.lock().await[idx] = Some(std::time::Instant::now());

                                        // Respond with Pong using our own counter
                                        let pong_request = SubscribeRequest {
                                            ping: Some(SubscribeRequestPing { id: current_ping_id }),
                                            ..Default::default()
                                        };

                                        if let Err(e) = subscribe_sink.send(pong_request).await {
                                            error!("Failed to send pong response for endpoint {}: {:?}", idx, e);
                                        }
                                    },
                                    UpdateOneof::Pong(_msg) => {
                                        // Server acknowledged our pong - calculate roundtrip
                                        let mut sent_at_guard = ping_sent_at.lock().await;
                                        if let Some(sent_at) = sent_at_guard[idx] {
                                            let now = std::time::Instant::now();
                                            let roundtrip = now.duration_since(sent_at).as_millis() as u64;
                                            debug!("Roundtrip for endpoint {}: {}ms", idx, roundtrip);

                                            // Call the callback with endpoint index and roundtrip time
                                            status_cb(idx, ConnectionStatus::Connected, roundtrip);
                                        }
                                        sent_at_guard[idx] = None;
                                    },
                                    _ => {
                                        if let Err(e) = tx.send(update_oneof) {
                                            error!("Failed to forward update from endpoint {}: {:?}", idx, e);
                                            return Err(anyhow!("Channel closed"));
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            error!("Stream error from endpoint {} ({}): {:?}", idx, endpoint.url, e);
                            return Err(anyhow!("Stream error: {:?}", e));
                        }
                        None => {
                            warn!("Stream closed for endpoint {}: {}", idx, endpoint.url);
                            return Ok(());
                        }
                    }
                }
                // Handle resubscription requests
                request = resubscribe_rx.recv() => {
                    match request {
                        Some(new_request) => {
                            if let Err(e) = subscribe_sink.send(new_request).await {
                                error!("Failed to send resubscription for endpoint {}: {:?}", idx, e);

                                status_cb(idx, ConnectionStatus::Disconnected, 999);
                                return Err(anyhow!("Failed to resubscribe: {:?}", e));
                            }
                            debug!("Resubscribed endpoint {}: {}", idx, endpoint.url);
                        }
                        None => {
                            debug!("Resubscribe channel closed for endpoint {}", idx);
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    pub async fn add_transaction_request(&self, program_id: String) -> Result<()> {
        let key = format!("tx_{}", program_id);

        let mut request = self.request.lock().await;

        if request.transactions.contains_key(&key) {
            return Ok(());
        }

        info!("Added to tx request: {}", program_id);

        request.transactions.insert(key, SubscribeRequestFilterTransactions {
            vote: None,
            failed: None,
            signature: None,
            account_include: vec![],
            account_exclude: vec![],
            account_required: vec![program_id],
        });

        drop(request); // Release lock before resubscribe

        self.resubscribe().await?;

        Ok(())
    }

    pub async fn add_account_request(&self, account_pubkey: String) -> Result<()> {
        let key = format!("account_{}", account_pubkey);

        let mut request = self.request.lock().await;

        if request.transactions.contains_key(&key) {
            return Ok(());
        }

        info!("Added to account request: {}", account_pubkey);

        request.accounts.insert(key, SubscribeRequestFilterAccounts {
            account: vec![account_pubkey],
            owner: vec![],
            filters: vec![],
            nonempty_txn_signature: None,
        });

        drop(request); // Release lock before resubscribe

        self.resubscribe().await?;

        Ok(())
    }

    pub async fn resubscribe(&self) -> Result<()> {
        debug!("resubscribe");
        let sinks: tokio::sync::MutexGuard<'_, Vec<Option<UnboundedSender<SubscribeRequest>>>> = self.sinks.lock().await;
        let request = self.request.lock().await.clone();

        for (idx, sink_opt) in sinks.iter().enumerate() {
            if let Some(sink) = sink_opt {
                match sink.send(request.clone()) {
                    Ok(_) => {
                        debug!("Queued resubscription for endpoint {}", idx);
                    }
                    Err(e) => {
                        debug!("Failed to queue resubscription for endpoint {}: {:?}", idx, e);
                    }
                }
            } else {
                debug!("Endpoint {} not connected, skipping resubscribe", idx);
            }
        }

        Ok(())
    }
}