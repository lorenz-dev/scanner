use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use dashmap::DashMap;
use futures::stream::StreamExt;
use futures::sink::SinkExt;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::{mpsc::UnboundedSender, Mutex};
use tracing::{error, debug, info, warn};
use yellowstone_grpc_client::GeyserGrpcClient;
use yellowstone_grpc_proto::{geyser::{CommitmentLevel, SubscribeRequestFilterAccounts}, prelude::{
    SubscribeRequest, SubscribeRequestFilterTransactions, SubscribeUpdate
}};

type SubscribeSink = Box<dyn futures::sink::Sink<SubscribeRequest, Error = futures::channel::mpsc::SendError> + Send + Unpin>;

#[derive(Clone, Deserialize, Eq, Hash, PartialEq)]
pub struct GrpcConfig {
    pub endpoint: String,
}

pub struct Grpc {
    pub config: GrpcConfig,
    pub connection_status: Arc<Mutex<GrpcConnectionStatus>>,
    subscribe_sink: Arc<Mutex<Option<SubscribeSink>>>,
    transactions_filters: Arc<DashMap<String, SubscribeRequestFilterTransactions>>,
    accounts_filters: Arc<DashMap<String, SubscribeRequestFilterAccounts>>,
}

pub struct GrpcConnectionStatus {
    pub status: ConnectionStatus,
    pub ping: u64,
}

pub enum ConnectionStatus {
    Connected,
    Connecting,
    Disconnected,
}

impl Grpc {
    pub fn new(config: &GrpcConfig) -> Self {
        Self {
            config: config.clone(),
            connection_status: Arc::new(Mutex::new(GrpcConnectionStatus {
                status: ConnectionStatus::Disconnected,
                ping: 0,
            })),
            subscribe_sink: Arc::new(Mutex::new(None)),
            transactions_filters: Arc::new(DashMap::new()),
            accounts_filters: Arc::new(DashMap::new()),
        }
    }

    async fn resilient_retry<Operation, OnError, Fut, T, E>(
        &self,
        operation_name: &str,
        mut operation: Operation,
        on_error: OnError,
    ) -> Result<T>
    where
        Operation: FnMut() -> Fut,
        OnError: Fn(&E) -> (),
        Fut: std::future::Future<Output = Result<T, E>>,
        E: std::fmt::Debug,
    {
        const BASE_DELAY_MS: u64 = 1000; // Start with 1 second
        const MAX_DELAY_MS: u64 = 60_000; // Cap at 60 seconds

        let mut attempt = 0u32;

        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(err) => {
                    on_error(&err);
                    // Calculate delay with exponential backoff, capped at max_delay_ms
                    let exponential_delay = BASE_DELAY_MS * 2_u64.pow(attempt.min(10));
                    let delay = Duration::from_millis(exponential_delay.min(MAX_DELAY_MS));

                    warn!(
                        "{} failed on {} (attempt {}): {:?}. Retrying in {:?}...",
                        operation_name,
                        self.config.endpoint,
                        attempt + 1,
                        err,
                        delay
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    pub async fn connect(&self, tx: &UnboundedSender<SubscribeUpdate>) -> Result<()> {
        self.resilient_retry(
            "Connection",
            || async {
                // Clear the sink if we're retrying
                *self.subscribe_sink.lock().await = None;

                let builder = GeyserGrpcClient::build_from_shared(self.config.endpoint.clone())
                    .map_err(|e| anyhow::anyhow!("Failed to build gRPC client: {:?}", e))?;

                self.connection_status.lock().await.status = ConnectionStatus::Connecting;

                let mut client = builder.connect().await
                    .map_err(|e| anyhow::anyhow!("Failed to connect to gRPC endpoint: {:?}", e))?;

                info!("Connected to {}", self.config.endpoint);

                let (subscribe_sink, mut stream) = client.subscribe().await
                    .map_err(|e| anyhow::anyhow!("Failed to create subscription stream: {:?}", e))?;

                
                self.connection_status.lock().await.status = ConnectionStatus::Connected;

                // Store the sink for later use
                *self.subscribe_sink.lock().await = Some(Box::new(subscribe_sink));
                info!("Subscription stream established for {}", self.config.endpoint);

                // Automatically resubscribe if we have filters configured
                if !self.transactions_filters.is_empty() || !self.accounts_filters.is_empty() {
                    if let Err(e) = self.subscribe().await {
                        warn!("Failed to send subscription after reconnection: {}", e);
                    }
                }

                // Process stream (no retry here, but wrapped in resilient_retry for reconnection)
                while let Some(subscribe_update) = stream.next().await {
                    match subscribe_update {
                        Ok(subscribe_update) => {
                            if tx.send(subscribe_update).is_err() {
                                info!("Receiver dropped, exiting cleanly");
                                return Ok(());
                            }
                        },
                        Err(e) => {
                            // Stream errors are logged but we don't force exit here.
                            // The gRPC stream closes itself after an error - the next
                            // stream.next() will return None, which naturally exits this loop
                            // and triggers reconnection via resilient_retry.
                            error!("Stream error from {}: {:?}", self.config.endpoint, e);
                        }
                    }
                }

                info!("Stream ended for {}, will reconnect...", self.config.endpoint);
                Err(anyhow::anyhow!("Stream ended"))
            },
            |_err| {
                // Status will be updated when reconnection succeeds
            },
        ).await
    }

    async fn subscribe(&self) -> Result<()> {
        let transactions_filters: HashMap<String, SubscribeRequestFilterTransactions> =
            self.transactions_filters.iter()
                .map(|entry| (entry.key().clone(), entry.value().clone()))
                .collect();

        let accounts_filters: HashMap<String, SubscribeRequestFilterAccounts> =
            self.accounts_filters.iter()
                .map(|entry| (entry.key().clone(), entry.value().clone()))
                .collect();

        debug!("Sending {} transactions {} accounts requests", &transactions_filters.len(), &accounts_filters.len());

        let request: SubscribeRequest = SubscribeRequest {
            transactions: transactions_filters,
            accounts: accounts_filters,
            commitment: Some(CommitmentLevel::Confirmed.into()),
            ..Default::default()
        };

        // Retry infinitely until subscription succeeds
        self.resilient_retry(
            "Subscription request",
            || async {
                let mut sink_guard = self.subscribe_sink.lock().await;
                match sink_guard.as_mut() {
                    Some(sink) => {
                        sink.send(request.clone()).await
                            .map_err(|e| anyhow::anyhow!("Send error: {:?}", e))
                    }
                    None => {
                        // Connection not yet established, return error to trigger retry
                        Err(anyhow::anyhow!("Connection not yet established"))
                    }
                }
            },
            |_err| {

            }
        ).await?;
        debug!("Subscription request sent to {}", self.config.endpoint);
        Ok(())
    }

    pub async fn subscribe_transactions(&self, account_pubkeys: Vec<Pubkey>) -> Result<()> {
        if account_pubkeys.is_empty() {
            anyhow::bail!("No accounts provided for transaction subscription");
        }

        debug!("Subscribing to {} transaction accounts on {}", account_pubkeys.len(), self.config.endpoint);

        for account_pubkey in account_pubkeys {
            self.transactions_filters.insert(
                format!("transaction_{}", account_pubkey),
                SubscribeRequestFilterTransactions {
                    vote: None,
                    failed: None,
                    signature: None,
                    account_include: vec![],
                    account_exclude: vec![],
                    account_required: vec![account_pubkey.to_string()],
                }
            );
        }

        self.subscribe().await
    }

    pub async fn subscribe_accounts(&self, account_pubkeys: Vec<Pubkey>) -> Result<()> {
        if account_pubkeys.is_empty() {
            anyhow::bail!("No accounts provided for account subscription");
        }

        debug!("Subscribing to {} accounts on {}", account_pubkeys.len(), self.config.endpoint);

        for account_pubkey in account_pubkeys {
            self.accounts_filters.insert(
                format!("account_{}", account_pubkey),
                SubscribeRequestFilterAccounts {
                    account: vec![account_pubkey.to_string()],
                    owner: vec![],
                    filters: vec![],
                    nonempty_txn_signature: None,
                }
            );
        }

        self.subscribe().await
    }
}
