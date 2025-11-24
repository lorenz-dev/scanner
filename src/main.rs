use std::time::Duration;
use std::sync::Arc;

use clap::Parser;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{debug, info, warn};
use yellowstone_grpc_proto::prelude::{
    SubscribeUpdate, subscribe_update::UpdateOneof,
};

use scanner::{
    ScannerContext, cli::Cli, config::Config, dashboard::Dashboard, grpc::Grpc, process::Process
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI arguments first to determine logging mode
    let cli = Cli::parse();

    // Initialize tracing based on mode
    if cli.term {
        // Terminal mode: log to stdout
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
            )
            .init();
    } else {
        // Dashboard mode: log to file
        let log_file = std::fs::File::create("scanner.log")?;
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
            )
            .with_writer(std::sync::Arc::new(log_file))
            .init();
    }

    info!("Starting scanner");

    // Load config from file
    let config = Config::from_file(&cli.config)?;

    // Parse arb program addresses
    let arb_programs: Vec<Pubkey> = config
        .arb_programs
        .iter()
        .map(|s| s.parse::<Pubkey>())
        .collect::<Result<Vec<_>, _>>()?;

    let scanner_context = Arc::new(ScannerContext::new());
    let grpc_connections: Vec<Arc<Grpc>> = Vec::new();

    // Clone scanner context for various tasks
    let scanner_context_for_grpc = scanner_context.clone();
    let scanner_context_for_ttl = scanner_context.clone();
    let scanner_context_for_dashboard = scanner_context.clone();

    let (subscribe_tx, mut subscribe_rx) = mpsc::unbounded_channel::<SubscribeUpdate>();

    info!("Scanner started with {} endpoints", config.grpc.len());

    let mut grpc_connections = grpc_connections;
    for grpc_config in config.grpc {
        let grpc = Arc::new(Grpc::new(&grpc_config));
        grpc_connections.push(grpc);
    }

    // Establish Grpc Connection
    for grpc in &grpc_connections {
        tokio::spawn({
            let grpc_clone = grpc.clone();
            let subscribe_tx_clone = subscribe_tx.clone();
            async move {
                grpc_clone.connect(&subscribe_tx_clone).await
            }
        });
    }

    // Receive Grpc Messages
    tokio::spawn({
        let grpc_connections_clone = grpc_connections.clone();
        let scanner_context = scanner_context_for_grpc;
        async move {
            while let Some(subscribe_update) = subscribe_rx.recv().await {
                if let Some(ref update_oneof) = subscribe_update.update_oneof {
                    match update_oneof {
                        UpdateOneof::Account(account) => {
                            info!("Received Account update");
                            let account = Process::process_account(account);
                            scanner_context.insert_mint(account);
                        }
                        UpdateOneof::Slot(_) => {
                            debug!("Received Slot update");
                        }
                        UpdateOneof::Transaction(transaction) => {
                            info!("Received Transaction update");

                            scanner_context.insert_received_transaction(transaction.clone());

                            let arb_transaction = Process::process_transaction(transaction.clone(), |mint| {
                                info!("Detected mint: {}", mint);
                                for grpc in &grpc_connections_clone {
                                    tokio::spawn({
                                        let grpc_clone = grpc.clone();
                                        let mint_clone = mint.clone();
                                        async move {
                                            if let Err(e) = grpc_clone.subscribe_accounts(vec![mint_clone]).await {
                                                warn!("Account subscription failed (will retry): {}", e);
                                            }
                                        }
                                    });
                                }
                            });
                            scanner_context.remove_received_transaction(transaction.clone());
                            scanner_context.insert_transaction(arb_transaction);
                        }
                        UpdateOneof::TransactionStatus(_) => {
                            debug!("Received TransactionStatus update");
                        }
                        UpdateOneof::Block(_) => {
                            info!("Received Block Update");
                        }
                        UpdateOneof::Ping(_) => {
                            debug!("Received Ping Update");
                        }
                        UpdateOneof::Pong(_) => {
                            debug!("Received Pong Update");
                        }
                        UpdateOneof::BlockMeta(_) => {
                            debug!("Received BlockMeta Update");
                        }
                        UpdateOneof::Entry(_) => {
                            debug!("Received Entry Update");
                        }
                    }
                }
            }
            info!("Subscribe channel closed");
        }
    });

    // Immediately try to subscribe (may fail, but will retry)
    for grpc in &grpc_connections {
        tokio::spawn({
            let grpc_clone = grpc.clone();
            let arb_programs_clone = arb_programs.clone();
            async move {
                if let Err(e) = grpc_clone.subscribe_transactions(arb_programs_clone).await {
                    warn!("Initial subscription failed (will retry): {}", e);
                }
            }
        });
    }

    // Transaction TTL cleanup task
    tokio::spawn({
        let scanner_context = scanner_context_for_ttl;
        let transaction_ttl_secs_clone = config.ttl.transaction_ttl_secs;
        let ttl_interval_secs_clone = config.ttl.ttl_interval_secs;
        async move {
            loop {
                sleep(Duration::from_secs(ttl_interval_secs_clone)).await;

                // Get current time in seconds
                let current_time_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;

                // Remove transactions older than TTL
                let removed_count = scanner_context.transactions.len();
                scanner_context.transactions.retain(|_sig, arb_transaction| {
                    if let Some(timestamp) = arb_transaction.timestamp {
                        // Keep if timestamp is within TTL window
                        current_time_secs - timestamp <= transaction_ttl_secs_clone as i64
                    } else {
                        // Keep transactions without timestamp for now
                        true
                    }
                });
                let removed_count = removed_count - scanner_context.transactions.len();

                if removed_count > 0 {
                    debug!("Removed {} expired transactions", removed_count);
                }
            }
        }
    });

    // Run the dashboard (blocks until user quits with Ctrl+C, Esc, or Q)
    Dashboard::new(scanner_context_for_dashboard);

    info!("Shutting down scanner");

    Ok(())
}
