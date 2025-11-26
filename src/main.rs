use std::time::Duration;
use std::sync::Arc;
use std::str::FromStr;

use clap::Parser;
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{debug, info, warn};
use yellowstone_grpc_proto::prelude::{
    SubscribeUpdate, subscribe_update::UpdateOneof,
};

use scanner::{
    ArbTransaction, ScannerContext, cli::Cli, config::Config, dashboard::Dashboard, grpc::Grpc, process::Process
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

    let scanner_context = Arc::new(ScannerContext::new(arb_programs.clone()));
    let grpc_connections: Vec<Arc<Grpc>> = Vec::new();

    // Clone scanner context for various tasks
    let scanner_context_for_grpc = scanner_context.clone();
    let scanner_context_for_ttl = scanner_context.clone();
    let scanner_context_for_dashboard = scanner_context.clone();

    let (subscribe_tx, mut subscribe_rx) = mpsc::unbounded_channel::<SubscribeUpdate>();

    info!("Scanner started with {} endpoints", config.grpc.len());

    let mut grpc_connections = grpc_connections;
    for grpc_config in &config.grpc {
        let grpc = Arc::new(Grpc::new(grpc_config));
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
                            debug!("Received Account update");

                            // Filter by data length FIRST (SPL token accounts are exactly 165 bytes)
                            const SPL_TOKEN_ACCOUNT_SIZE: usize = 165;
                            let account_info = account.account.as_ref().unwrap();

                            if account_info.data.len() != SPL_TOKEN_ACCOUNT_SIZE {
                                debug!(
                                    "Skipping account update - data length {} (expected {})",
                                    account_info.data.len(),
                                    SPL_TOKEN_ACCOUNT_SIZE
                                );
                                continue;
                            }

                            let mint_ctx = Process::process_account(account);

                            // Token Program IDs (all token accounts are owned by one of these)
                            let token_program_id = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
                                .parse::<Pubkey>()
                                .unwrap();
                            let token_2022_program_id = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
                                .parse::<Pubkey>()
                                .unwrap();

                            // Check if this is a token account (owned by Token Program or Token2022)
                            if mint_ctx.owner == token_program_id || mint_ctx.owner == token_2022_program_id {
                                info!("Processing token account update: mint={}, authority={}, liquidity={}",
                                    mint_ctx.mint, mint_ctx.authority, mint_ctx.liquidity);
                                // This is a token account - ALWAYS aggregate liquidity for tracked intermediate mints
                                debug!("Updating liquidity for mint {} from authority {}: {} lamports",
                                    mint_ctx.mint, mint_ctx.authority, mint_ctx.liquidity);
                                scanner_context.upsert_pool_liquidity(
                                    mint_ctx.mint,
                                    mint_ctx.authority,
                                    mint_ctx.liquidity
                                );

                                // If authority is NOT a known DEX yet, subscribe to it to discover the DEX program
                                if !scanner_context.known_dex_programs.contains_key(&mint_ctx.authority) {
                                    debug!("? Authority {} not yet known as DEX, subscribing to discover owner",
                                        mint_ctx.authority);

                                    for grpc in &grpc_connections_clone {
                                        tokio::spawn({
                                            let grpc_clone = grpc.clone();
                                            let authority = mint_ctx.authority;
                                            async move {
                                                match grpc_clone.subscribe_accounts(vec![authority]).await {
                                                    Ok(_) => debug!("Subscribed to authority address: {}", authority),
                                                    Err(e) => warn!("Failed to subscribe to authority {}: {:?}", authority, e),
                                                }
                                            }
                                        });
                                    }
                                } else {
                                    debug!("✓ Authority {} is known DEX program", mint_ctx.authority);
                                }
                            } else {
                                // This is NOT a token account - it's a PDA/program account
                                // The owner field is the DEX program that controls this PDA
                                info!("✓ Discovered DEX program: {} (from authority account update)", mint_ctx.owner);
                                scanner_context.known_dex_programs.insert(mint_ctx.owner, ());
                            }
                        }
                        UpdateOneof::Slot(_) => {
                            debug!("Received Slot update");
                        }
                        UpdateOneof::Transaction(transaction) => {
                            debug!("Received Transaction update");

                            scanner_context.insert_received_transaction(transaction.clone());

                            let arb_transaction = Process::process_transaction(transaction.clone(), |mint, decimals, token_account_addresses| {
                                // Save mint decimals to scanner context
                                scanner_context.upsert_mint_decimals(*mint, decimals);

                                info!(
                                    "Found intermediate mint: {} with {} decimals and {} token accounts",
                                    mint,
                                    decimals,
                                    token_account_addresses.len()
                                );

                                // Subscribe to token accounts (not the mint account)
                                if !token_account_addresses.is_empty() {
                                    for grpc in &grpc_connections_clone {
                                        tokio::spawn({
                                            let grpc_clone = grpc.clone();
                                            let mint_clone = *mint;
                                            let accounts_clone = token_account_addresses.clone();
                                            async move {
                                                info!(
                                                    "Subscribing to {} token accounts for mint: {}",
                                                    accounts_clone.len(),
                                                    mint_clone
                                                );
                                                match grpc_clone.subscribe_accounts(accounts_clone).await {
                                                    Ok(_) => info!(
                                                        "Successfully subscribed to token accounts for mint: {}",
                                                        mint_clone
                                                    ),
                                                    Err(e) => warn!(
                                                        "Failed to subscribe to token accounts for mint {}: {:?}",
                                                        mint_clone,
                                                        e
                                                    ),
                                                }
                                            }
                                        });
                                    }
                                } else {
                                    warn!("No token accounts found for intermediate mint: {}", mint);
                                }
                            }).unwrap_or_else(|_| {
                                let tx_info = transaction.clone().transaction.unwrap();

                                let sig_str = bs58::encode(&tx_info.signature).into_string();
                                let signature = Signature::from_str(&sig_str).unwrap();

                                ArbTransaction {
                                    signature: signature,
                                    is_success: false,
                                    instructions: vec![],
                                    program_instructions: vec![],
                                    timestamp: 0,
                                }
                            });

                            // Register all pools from DexSwaps in known_pools and track DEX programs
                            for program_instruction in &arb_transaction.program_instructions {
                                for arb_instruction in &program_instruction.arb_instructions {
                                    if let scanner::ArbTransactionInstruction::DexSwap(swap) = arb_instruction {
                                        // Track the DEX program ID
                                        if !scanner_context.known_dex_programs.contains_key(&swap.swap_program_id) {
                                            info!("Registering new DEX program: {}", swap.swap_program_id);
                                        }
                                        scanner_context.known_dex_programs.insert(swap.swap_program_id, ());

                                        // Track individual pools
                                        for pool in &swap.pools {
                                            if !scanner_context.known_pools.contains_key(pool) {
                                                info!("Registering new pool in known_pools: {}", pool);
                                            }
                                            scanner_context.known_pools.insert(*pool, ());
                                        }
                                    }
                                }
                            } 
                            
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

    // Wait a bit for connections to establish before subscribing
    tokio::spawn({
        let grpc_connections_clone = grpc_connections.clone();
        let arb_programs_clone = arb_programs.clone();
        async move {
            // Give connections time to establish
            sleep(Duration::from_secs(2)).await;

            for grpc in &grpc_connections_clone {
                tokio::spawn({
                    let grpc_clone = grpc.clone();
                    let arb_programs_clone = arb_programs_clone.clone();
                    async move {
                        let _ = grpc_clone.subscribe_transactions(arb_programs_clone).await;
                    }
                });
            }
        }
    });

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
                scanner_context.transactions.retain(|_sig, arb_transaction| {
                    // Keep if timestamp is within TTL window
                    current_time_secs - arb_transaction.timestamp <= transaction_ttl_secs_clone as i64
                });
            }
        }
    });

    // Periodic ping task to measure roundtrip latency
    tokio::spawn({
        let grpc_connections_clone = grpc_connections.clone();
        async move {
            // Wait a bit for connections to establish
            sleep(Duration::from_secs(2)).await;

            loop {
                sleep(Duration::from_secs(5)).await; // Send ping every 5 seconds

                for grpc in &grpc_connections_clone {
                    tokio::spawn({
                        let grpc_clone = grpc.clone();
                        async move {
                            let _ = grpc_clone.send_ping().await;
                        }
                    });
                }
            }
        }
    });

    // Sync grpc connection status to scanner context
    let scanner_context_for_status = scanner_context.clone();
    tokio::spawn({
        let grpc_connections_clone = grpc_connections.clone();
        async move {
            loop {
                sleep(Duration::from_millis(500)).await;

                // Sync each grpc connection status to scanner_context
                for grpc in &grpc_connections_clone {
                    let status = grpc.connection_status.lock().await;
                    debug!("Syncing status for {}: ping={}ms", grpc.config.endpoint, status.ping);
                    scanner_context_for_status.connections.insert(
                        grpc.config.endpoint.clone(),
                        scanner::grpc::GrpcConnectionStatus {
                            status: match status.status {
                                scanner::grpc::ConnectionStatus::Connected => scanner::grpc::ConnectionStatus::Connected,
                                scanner::grpc::ConnectionStatus::Connecting => scanner::grpc::ConnectionStatus::Connecting,
                                scanner::grpc::ConnectionStatus::Disconnected => scanner::grpc::ConnectionStatus::Disconnected,
                            },
                            ping: status.ping,
                            last_update: status.last_update,
                            ping_sent_at: status.ping_sent_at,
                        }
                    );
                }
            }
        }
    });

    // Run the dashboard (blocks until user quits with Ctrl+C, Esc, or Q)
    Dashboard::new(scanner_context_for_dashboard, config);

    info!("Shutting down scanner");

    Ok(())
}
