use std::fs;
use yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction;

use scanner::{process::Process, utils::load_transaction_update};

// umW5vSenssHYTxYgPSsPjZTZXTdR85QHK1NkaZVhQHTkZ9ZdK8EspMFsVZqP2x38LWV8ovUFQ35YoXduAztexsc
// 2dA9Wyxkhx1yHfWdCd6cV4axrueukzSAvCEzzNMcEtWu6MBe7zwwUZ8ymEz5RvJ9ticj8qf2mRwY8Cm8UUjiwaT4 >  src/dex/whirlpools.rs:56:67: index out of bounds

#[test]
pub fn test_transaction() {
    // Initialize tracing subscriber for tests
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .with_test_writer()
        .try_init();

    let tx_update: SubscribeUpdateTransaction = load_transaction_update("1CZkmo3purfWxe8Jwt6Sdxb7mcrmHw3BchmA5mmrYEPcRUr9zJMsEzJf9S5MD3JNHH8GvThynXzx967f2TwVSQ9.pb");
    // let tx_update: SubscribeUpdateTransaction = load_transaction_update("2ibprU4CWxQMmA2WFM5ikx56Tq1TpPUFF8ysx4EadyTvRq3KPnnJ3Ab2nkiGxTCD19vS9WRLuYuTj7v5y2Yjjq7R.pb");

    let _ = Process::process_transaction(tx_update, |mint| {
        println!("OnMintAccount: {}", mint);
    });
}

#[test]
#[ignore]
pub fn test_all_transaction() {
    // Initialize tracing subscriber for tests
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .with_test_writer()
        .try_init();

    // Read all files from the transactions directory
    let transactions_dir = "transactions";
    let entries = fs::read_dir(transactions_dir)
        .expect("Failed to read transactions directory");

    let mut processed_count = 0;
    let mut parsing_errors_count = 0;

    for entry in entries {
        let entry = entry.expect("Failed to read directory entry");
        let path = entry.path();

        // Only process .pb files
        if path.extension().and_then(|s| s.to_str()) == Some("pb") {
            let filename = path.file_name()
                .and_then(|s| s.to_str())
                .expect("Failed to get filename");

            println!("\n=== Processing: {} ===", filename);

            let tx_update: SubscribeUpdateTransaction = load_transaction_update(filename);

            let arb_tx = Process::process_transaction(tx_update, |_mint| {
                // Mint discovery callback - not needed for tests
            });

            // Count parsed vs failed instructions
            let parsed_swaps = arb_tx.instructions.iter().filter(|inst| {
                matches!(inst, scanner::ArbTransactionInstruction::DexSwap(_))
            }).count();

            let raw_instructions = arb_tx.instructions.iter().filter(|inst| {
                matches!(inst, scanner::ArbTransactionInstruction::Raw(_, _))
            }).count();

            if raw_instructions > 0 {
                println!("  ⚠ PARSING ERROR: {} raw (failed), {} parsed swaps", raw_instructions, parsed_swaps);
                println!("    ^ Check logs above for error details (e.g., ParserNotFound)");
                parsing_errors_count += raw_instructions;
            }

            if parsed_swaps > 0 || raw_instructions == 0 {
                processed_count += 1;
            }
        }
    }

    println!("\n=== Summary ===");
    println!("Transactions processed: {}", processed_count);
    println!("Parsing errors (Raw instructions): {}", parsing_errors_count);
    println!("\nNote: Raw instructions indicate parsing failures.");
    println!("Check warning logs for specific error types like:");
    println!("  - ParserNotFound: Program ID not registered in DexRegistry");
    println!("  - DiscriminatorNotFound: Unknown instruction discriminator");
    println!("  - TokenError: Issues parsing token transfers");
    println!("  - InvalidInstructionData: Malformed instruction data");
}