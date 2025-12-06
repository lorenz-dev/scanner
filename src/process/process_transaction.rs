use anyhow::Result;
use tracing::info;
use std::{collections::HashMap, str::FromStr};
use solana_sdk::{inner_instruction, pubkey::Pubkey, signature::Signature, sysvar::instructions};
use yellowstone_grpc_proto::{geyser::SubscribeUpdateTransaction, prelude::{CompiledInstruction, InnerInstruction, InnerInstructions, TransactionStatusMeta}};

use crate::{Mint, ParseInnerInstructionEnum, ParsedInstruction, ProcessedTransaction, ScannerContext, dex::DexRegistry};

pub fn process_transaction<F>(ctx: &ScannerContext, tx_update: SubscribeUpdateTransaction, on_mint: F) -> Result<()>
where
    F: Fn(String, Mint),
{
    let tx_info = tx_update.transaction.unwrap();

    let signature: String = bs58::encode(&tx_info.signature).into_string();

    let mut err = None;

    let mut account_pubkeys: Vec<String> = Vec::new();
    let mut token_account_pubkeys = HashMap::new();

    let mut compiled_instructions = Vec::new();
    let mut instructions = Vec::new();

    if let Some(tx) = tx_info.transaction {
        
        if let Some(message) = tx.message {
            for pubkey in &message.account_keys {
                account_pubkeys.push(bs58::encode(pubkey).into_string());
            }

            compiled_instructions = message.instructions;
        }
    }

    if let Some(meta) = tx_info.meta {
        err = meta.err;

        for pubkey in &meta.loaded_writable_addresses {
            account_pubkeys.push(bs58::encode(pubkey).into_string());
        }
        for pubkey in &meta.loaded_readonly_addresses {
            account_pubkeys.push(bs58::encode(pubkey).into_string());
        }

        for token_balance in &meta.post_token_balances {
            let decimals = token_balance.ui_token_amount.as_ref().map(|ui| ui.decimals).unwrap_or(0);
            let owner = account_pubkeys[token_balance.account_index as usize].clone();
            token_account_pubkeys.insert(owner, Mint {
                program_id: token_balance.mint.clone(),
                decimals,
            });
        }

        if let Ok(parsed_instructions) = parse_instructions(compiled_instructions, meta.inner_instructions, account_pubkeys, token_account_pubkeys.clone()) {
            // Collect all swaps
            let mut all_swaps = Vec::new();
            for instruction in &parsed_instructions {
                for inner in &instruction.inner_instrucions {
                    if let ParseInnerInstructionEnum::Swap(swap) = inner {
                        all_swaps.push(swap);
                    }
                }
            }

            // Collect all pool addresses from swaps
            let mut all_pools = std::collections::HashSet::new();
            for swap in &all_swaps {
                for pool in &swap.pools {
                    all_pools.insert(pool.to_string());
                }
            }

            // Find intermediate mints - mints that appear between swaps
            for i in 0..all_swaps.len().saturating_sub(1) {
                let current_swap = all_swaps[i];
                let next_swap = all_swaps[i + 1];

                // If the output of current swap is the input of next swap, it's an intermediate mint
                if current_swap.token_out == next_swap.token_in {
                    let intermediate_mint_pubkey = &current_swap.token_out;

                    info!("Intermediate mint: {}", intermediate_mint_pubkey);
                    info!("All pools: {:#?}", &all_pools);
                    info!("Token account pubkeys: {:#?}", &token_account_pubkeys);

                    // Find all pool token accounts with this intermediate mint from token balance changes
                    for (token_account_owner, mint) in &token_account_pubkeys {
                        if all_pools.contains(token_account_owner) && mint.program_id == *intermediate_mint_pubkey {
                            info!("Found pool token account: {} with intermediate mint: {}", token_account_owner, mint.program_id);
                            on_mint(token_account_owner.clone(), mint.clone());
                        }
                    }
                }
            }

            instructions = parsed_instructions;
        }
    }
    
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    ctx.add_transaction(ProcessedTransaction {
        signature,
        err,
        instructions,
        timestamp
    });

    Ok(())
}

fn parse_instructions(
    compiled_instructions: Vec<CompiledInstruction>, 
    inner_instructions: Vec<InnerInstructions>,
    account_pubkeys: Vec<String>,
    token_account_pubkeys: HashMap<String, Mint>,
) -> Result<Vec<ParsedInstruction>> {
    let mut parsed_instructions = Vec::new();

    // Process each compiled instruction and its corresponding inner instructions
    for (idx, compiled_instruction) in compiled_instructions.iter().enumerate() {
        let program_id = account_pubkeys
            .get(compiled_instruction.program_id_index as usize)
            .ok_or_else(|| anyhow::anyhow!("Invalid program_id_index: {}", compiled_instruction.program_id_index))?;

       let inner_instructions: Vec<InnerInstruction> = inner_instructions
            .iter()
            .filter(|inner_group| inner_group.index as usize == idx)
            .flat_map(|inner_group| inner_group.instructions.clone())
            .collect();

        let mut parsed_inner_instructions = Vec::new();

        let mut idx = 0;
        while idx < inner_instructions.len() {
            if let Some(instruction) = inner_instructions.get(idx) {
                let stack_height = instruction.stack_height.unwrap_or(1);

                let mut child_inner_instructions = Vec::new();

                if stack_height == 2 {
                    idx += 1;
                    while idx < inner_instructions.len() {
                        if let Some(inner_instruction) = inner_instructions.get(idx) {
                            let child_stack_height = inner_instruction.stack_height.unwrap_or(1);

                            if child_stack_height == 2 {
                                break;
                            }

                            child_inner_instructions.push(inner_instruction.clone());
                            idx += 1;
                        }
                    }

                    match DexRegistry::global().parse(instruction, &child_inner_instructions, &account_pubkeys, &token_account_pubkeys) {
                        Ok(parse_inner_instruction) => {
                            parsed_inner_instructions.push(ParseInnerInstructionEnum::Swap(parse_inner_instruction));
                        }
                        Err(_) => {
                            parsed_inner_instructions.push(ParseInnerInstructionEnum::Other);
                        }
                    }
                }
            }
        }

        parsed_instructions.push(ParsedInstruction {
            program_id: program_id.clone(),
            inner_instrucions: parsed_inner_instructions,
        });
    }

    Ok(parsed_instructions)
}