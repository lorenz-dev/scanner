use std::fs;
use std::path::Path;
use anyhow::Result;
use scanner::process::Process;
use tracing::{info, warn};

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let transactions_dir = Path::new("transactions");

    if !transactions_dir.exists() {
        warn!("Transactions directory does not exist: {:?}", transactions_dir);
        return Ok(());
    }

    info!("Reading transactions from: {:?}", transactions_dir);

    // Collect all .pb files from transactions directory and its subdirectories
    let mut pb_files = Vec::new();
    collect_pb_files(transactions_dir, &mut pb_files)?;

    info!("Found {} transaction files to process", pb_files.len());

    for (file_path, file_name) in pb_files {
        info!("Processing transaction file: {}", file_name);

        // Load the transaction update
        let tx_update = scanner::utils::load_transaction_update(&file_name);

        // Process the transaction to extract DEX information
        let arb_transaction = match Process::process_transaction(tx_update, |_mint, _decimals, _token_accounts| {
            // We don't need to do anything with the mint callback for this script
        }) {
            Ok(tx) => tx,
            Err(e) => {
                warn!("Failed to process transaction {}: {:?}", file_name, e);
                continue;
            }
        };

        // Collect all DEX names from the transaction
        let mut dex_names = Vec::new();

        for program_instruction in &arb_transaction.program_instructions {
            for arb_instruction in &program_instruction.arb_instructions {
                if let scanner::ArbTransactionInstruction::DexSwap(swap) = arb_instruction {
                    // Get DEX name from the registry
                    let dex_name = scanner::dex::DexRegistry::global()
                        .get_dex_name(&swap.swap_program_id);

                    if let Some(name) = dex_name {
                        if !dex_names.contains(&name) {
                            dex_names.push(name);
                        }
                    }
                }
            }
        }

        if dex_names.is_empty() {
            info!("No DEX swaps found in transaction: {}", file_name);
            continue;
        }

        // Determine status subdirectory based on transaction success
        let status_subdir = if arb_transaction.is_success {
            "successful"
        } else {
            "failed"
        };

        // Copy the transaction file to each DEX directory with status separation
        let mut all_copies_successful = true;
        for dex_name in dex_names {
            let dex_dir = transactions_dir.join(dex_name).join(status_subdir);

            // Create the DEX/status directory if it doesn't exist
            if !dex_dir.exists() {
                fs::create_dir_all(&dex_dir)?;
                info!("Created directory: {:?}", dex_dir);
            }

            let dest_path = dex_dir.join(&file_name);

            // Skip if already exists in the correct location
            if dest_path.exists() {
                continue;
            }

            // Copy the file
            match fs::copy(&file_path, &dest_path) {
                Ok(_) => {
                    info!("Copied {} to {}/{}/", file_name, dex_name, status_subdir);
                }
                Err(e) => {
                    warn!("Failed to copy {} to {}/{}/: {:?}", file_name, dex_name, status_subdir, e);
                    all_copies_successful = false;
                }
            }
        }

        // Remove the source file after all copies are successful
        if all_copies_successful {
            match fs::remove_file(&file_path) {
                Ok(_) => {
                    info!("Removed source file: {}", file_name);
                }
                Err(e) => {
                    warn!("Failed to remove source file {}: {:?}", file_name, e);
                }
            }
        }
    }

    info!("Transaction organization complete!");
    Ok(())
}

fn collect_pb_files(dir: &Path, files: &mut Vec<(std::path::PathBuf, String)>) -> Result<()> {
    let entries = fs::read_dir(dir)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // Recursively collect from subdirectories
            collect_pb_files(&path, files)?;
        } else if path.extension().map_or(false, |ext| ext == "pb") {
            let file_name = path.file_name().unwrap().to_string_lossy().to_string();
            files.push((path, file_name));
        }
    }

    Ok(())
}
