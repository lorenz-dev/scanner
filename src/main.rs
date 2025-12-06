use std::{collections::{HashMap, HashSet}, sync::Arc};
use anyhow::{anyhow, Result};
use scanner::{
    ScannerContext, config::ScannerConfig, dashboard::Dashboard, grpc::{ConnectionContext, ConnectionStatus, multi_grpc::MultiGrpc}, logger::Logger, process
};
use tokio::sync::mpsc;
use tracing::{info, debug, warn};
use yellowstone_grpc_proto::geyser::subscribe_update::UpdateOneof;

#[tokio::main]
async fn main() -> Result<()> {
    info!("Starting!");

    Logger::init().map_err(|err| anyhow!("Cannot initialize logger"))?;

    let config = ScannerConfig::from_toml_file("config.toml".to_string())
        .unwrap_or_else(|err| {
            warn!("Could not load config.toml: {}", err);
            ScannerConfig::default()
        });

    let ctx = Arc::new(ScannerContext::new(config.arb_programs.clone()));
    let mut temp_txs: HashSet<String> = HashSet::new();

    let (grpc_tx, mut grpc_rx) = mpsc::unbounded_channel();

    // Grpc Config
    let multi_grpc = Arc::new(MultiGrpc::new(config.grpc.clone()));

    {
        let mut connections = ctx.connections.write();
        for endpoint in &multi_grpc.config.endpoints {
            connections.push(ConnectionContext {
                endpoint: endpoint.url.clone(),
                status: ConnectionStatus::Disconnected,
                ping: 0,
            });
        }
    }

    // Grpc Connection
    tokio::spawn({
        let multi_grpc_clone = multi_grpc.clone();
        let grpc_tx_clone = grpc_tx.clone();
        let ctx_clone = ctx.clone();
        async move {
            multi_grpc_clone.connect(
                &grpc_tx_clone,
                move |idx, status, ping_ms| {
                    let mut connections = ctx_clone.connections.write();
                    if idx < connections.len() {
                        connections[idx].status = status;
                        connections[idx].ping = ping_ms;
                    }
                }
            ).await;
        }
    });

    tokio::spawn({
        let ctx_clone = ctx.clone();
        let multi_grpc_clone = multi_grpc.clone();
        async move {
            while let Some(update_oneof) = grpc_rx.recv().await {
                match update_oneof {
                    UpdateOneof::Transaction(tx_update) => {
                        if let Some(ref tx) = tx_update.transaction {
                            let signature = bs58::encode(&tx.signature).into_string();

                            if temp_txs.contains(&signature) {
                                // skipping
                                return
                            }

                            let ctx_inner = ctx_clone.clone();
                            let multi_grpc_inner = multi_grpc_clone.clone();
                            let _ = process::process_transaction::process_transaction(&ctx_clone, tx_update, |token_account, mint| {
                                ctx_inner.add_intermediate_mint(mint);
                                let multi_grpc = multi_grpc_inner.clone();
                                tokio::spawn(async move {
                                    let _ = multi_grpc.add_account_request(token_account).await;
                                });
                            });

                            // remove from list
                            temp_txs.remove(&signature);
                        }
                    },
                    UpdateOneof::Account(account_update) => {
                        process::process_account::process_account(&ctx_clone, account_update);
                    },
                    _ => {}
                }
            }
        }
    });

    // Arb Program Subscription
    for arb_program in &config.arb_programs {
        tokio::spawn({
            let multi_grpc_clone = multi_grpc.clone();
            let arb_program_clone = arb_program.clone();
            async move {
                let _ = multi_grpc_clone.add_transaction_request(arb_program_clone).await;
            }
        });
    }

    // Clean up transactions
    tokio::spawn({
        let ctx_clone = ctx.clone();
        let ttl_sec = config.transaction_ttl_sec;
        async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

                let current_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                let cutoff_time = current_time.saturating_sub(ttl_sec);

                // Remove transactions older than TTL
                let mut transactions = ctx_clone.transactions.write();
                let before_count = transactions.len();
                transactions.retain(|tx| tx.timestamp >= cutoff_time);
                let after_count = transactions.len();

                if before_count > after_count {
                    debug!("Cleaned up {} old transactions (TTL: {}s)", before_count - after_count, ttl_sec);
                }

                // Collect all intermediate mints from remaining transactions
                let mut intermediate_mints = std::collections::HashSet::new();
                for tx in transactions.iter() {
                    // Extract all swaps
                    let mut all_swaps = Vec::new();
                    for instruction in &tx.instructions {
                        for inner in &instruction.inner_instrucions {
                            if let scanner::ParseInnerInstructionEnum::Swap(swap) = inner {
                                all_swaps.push(swap);
                            }
                        }
                    }

                    // Find intermediate mints
                    for i in 0..all_swaps.len().saturating_sub(1) {
                        let current_swap = all_swaps[i];
                        let next_swap = all_swaps[i + 1];

                        if current_swap.token_out == next_swap.token_in {
                            intermediate_mints.insert(current_swap.token_out.clone());
                        }
                    }
                }

                drop(transactions); // Release the lock

                // Clean up token_account_data for mints not in intermediate_mints
                let mut removed_count = 0;
                ctx_clone.token_account_data.retain(|_token_account, mint_data| {
                    let keep = intermediate_mints.contains(&mint_data.mint);
                    if !keep {
                        removed_count += 1;
                    }
                    keep
                });

                if removed_count > 0 {
                    debug!("Cleaned up {} token accounts for mints no longer in use", removed_count);
                }
            }
        }
    });

    // Dashboard is blocking so must go last
    Dashboard::new().run(&ctx, &config)?;

    info!("Shutting Down!");

    Ok(())
}
