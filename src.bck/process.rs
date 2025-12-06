use std::{collections::{HashMap, HashSet}, str::FromStr};

use anyhow::Result;
use chrono::offset;
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use yellowstone_grpc_proto::prelude::{
    CompiledInstruction, InnerInstruction, InnerInstructions, SubscribeUpdateAccount, SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo, TransactionStatusMeta
};
use tracing::{info, warn};

use crate::{ArbTransaction, ArbTransactionInstruction, MintContext, TokenAccounts, dex::{DexParserError, DexRegistry}, token::TokenInfo, utils::save_transaction_update};

pub struct Process {
}

pub struct ProcessedAccount {
    pub mint: Pubkey,
    pub authority: Pubkey,  // The authority from token account data (PDA)
    pub owner: Pubkey,  // The program owner from account metadata
    pub liquidity: u64,
}

pub enum ProcessedTransaction {
    SuccessfulTransaction(),
    FailedTransaction(ArbTransaction),
}

impl Process {
    pub fn process_account(account: &SubscribeUpdateAccount) -> ProcessedAccount {
        // SPL Token account structure:
        // - mint: Pubkey at offset 0 (32 bytes)
        // - authority: Pubkey at offset 32 (32 bytes) - the PDA/authority
        // - balance: u64 at offset 64 (8 bytes)
        let account_info: yellowstone_grpc_proto::prelude::SubscribeUpdateAccountInfo = account.account.clone().unwrap();

        let data = account_info.data;
        let mut offset = 0;

        let mint = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .unwrap()
        );
        offset += 32;

        let authority = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .unwrap()
        );
        offset += 32;

        let liquidity = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .unwrap()
        );

        // Get the program owner from account metadata
        let owner = Pubkey::new_from_array(
            account_info.owner
                .try_into()
                .unwrap()
        );

        ProcessedAccount {
            mint,
            authority,
            owner,
            liquidity,
        }
    }

    pub fn process_transaction<OnMintAccount>(tx_update: SubscribeUpdateTransaction, on_mint_account: OnMintAccount) -> Result<ArbTransaction>
    where
        OnMintAccount: Fn(&Pubkey, u8, &Vec<Pubkey>),  // Accept mint, decimals, and token account addresses
    {
        let tx_info = tx_update.clone().transaction.unwrap();
        let tx = tx_info.clone().transaction.unwrap();
        let meta = tx_info.clone().meta.unwrap();

        let sig_str = bs58::encode(&tx_info.signature).into_string();
        let signature = Signature::from_str(&sig_str).unwrap();

        let accounts = Process::get_accounts(&tx_info);

        let token_accounts = Process::get_token_accounts(&meta, &accounts);
        
        let compiled_instructions = tx.message.clone().unwrap().instructions;
        let inner_instructions_groups = meta.inner_instructions.clone();
        let log_messages = meta.log_messages;

        let program_instructions: Vec<ProgramInstruction> = Self::build_program_instructions(
            compiled_instructions,
            inner_instructions_groups,
            log_messages,
            accounts.clone(),
            token_accounts.clone(),
        ).unwrap_or_else(|_| {
            vec![]
        });

        // Call on_mint_account only for intermediate mints
        // Intermediate mints are those between the first input and last output in the swap route
        let all_swaps: Vec<_> = program_instructions
            .iter()
            .flat_map(|pi| &pi.arb_instructions)
            .filter_map(|inst| match inst {
                ArbTransactionInstruction::DexSwap(swap) => Some(swap),
                _ => None,
            })
            .collect();

        all_swaps.iter().for_each(|swaps| {
            info!("Processing Signature: {}", sig_str);
            info!("{:?}", &swaps);
            info!("{} - {}", swaps.token_in.mint, swaps.token_out.mint);
        });

        // Collect all pool addresses and pool owners from all swaps
        let all_pools: HashSet<Pubkey> = all_swaps
            .iter()
            .flat_map(|swap| &swap.pools)
            .copied()
            .collect();

        let all_pool_owners: HashSet<Pubkey> = all_swaps
            .iter()
            .map(|swap| swap.pool_owner)
            .filter(|owner| *owner != Pubkey::default())  // Skip default values
            .collect();

        // Collect intermediate mints (excluding first in and last out)
        let mut intermediate_mints = HashSet::new();

        if !all_swaps.is_empty() {
            let first_in = all_swaps.first().map(|s| s.token_in.mint);
            let last_out = all_swaps.last().map(|s| s.token_out.mint);

            for swap in &all_swaps {
                // Add token_out if it's not the last output
                if Some(swap.token_out.mint) != last_out {
                    intermediate_mints.insert(swap.token_out.mint);
                }

                // Add token_in if it's not the first input
                if Some(swap.token_in.mint) != first_in {
                    intermediate_mints.insert(swap.token_in.mint);
                }
            }
        }

        // Use post_token_balances to find token accounts for intermediate mints
        // Group by mint to collect all token account addresses
        let mut mint_to_accounts: HashMap<Pubkey, Vec<Pubkey>> = HashMap::new();
        let mut mint_to_decimals: HashMap<Pubkey, u8> = HashMap::new();

        for program_instruction in &program_instructions {
            for arb_instruction in &program_instruction.arb_instructions {
                if let ArbTransactionInstruction::DexSwap(dex_swap) = arb_instruction {
                    for token_balance in &meta.post_token_balances {
                        // Extract mint from token_balance
                        if let Ok(mint) = Pubkey::try_from(token_balance.mint.as_bytes()) {
                            // Only process intermediate mints
                            if !intermediate_mints.contains(&mint) {
                                continue;
                            }

                            // Extract token account address
                            if let Some(account_address) = accounts.get(token_balance.account_index as usize) {
                                // Check if this token account's owner matches a pool
                                if dex_swap.pools.iter().any(|pool| pool.as_ref() == token_balance.owner.as_bytes()) {
                                    // Add to the map
                                    mint_to_accounts
                                        .entry(mint)
                                        .or_insert_with(Vec::new)
                                        .push(*account_address);

                                    // Store decimals if not already stored
                                    if !mint_to_decimals.contains_key(&mint) {
                                        let decimals = token_accounts.values()
                                            .find(|ta| ta.token_info.mint == mint)
                                            .map(|ta| ta.token_info.decimals)
                                            .unwrap_or(token_balance.ui_token_amount.as_ref()
                                                .map(|amount| amount.decimals as u8)
                                                .unwrap_or(0));
                                        mint_to_decimals.insert(mint, decimals);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Call on_mint_account for each intermediate mint with collected token accounts
        for mint in intermediate_mints {
            let token_account_addresses = mint_to_accounts.get(&mint).cloned().unwrap_or_else(Vec::new);
            let decimals = mint_to_decimals.get(&mint).copied().unwrap_or(0);

            on_mint_account(&mint, decimals, &token_account_addresses);
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        Ok(ArbTransaction {
            signature,
            is_success: meta.err.is_none(),
            instructions: vec![],
            program_instructions,
            timestamp,
        })
    }

    fn get_accounts(tx_info: &SubscribeUpdateTransactionInfo) -> Vec<Pubkey> {
        let mut account_keys = Vec::new();

        if let Some(transaction) = tx_info.transaction.as_ref() {
            if let Some(message) = transaction.message.as_ref() {
                for key in &message.account_keys {
                    if let Ok(pubkey) = Pubkey::try_from(&key[..]) {
                        account_keys.push(pubkey);
                    }
                }
            }
        }

        // Add loaded addresses from address lookup tables (for versioned transactions)
        if let Some(meta) = tx_info.meta.as_ref() {
            for key in &meta.loaded_writable_addresses {
                if let Ok(pubkey) = Pubkey::try_from(&key[..]) {
                    account_keys.push(pubkey);
                }
            }
            for key in &meta.loaded_readonly_addresses {
                if let Ok(pubkey) = Pubkey::try_from(&key[..]) {
                    account_keys.push(pubkey);
                }
            }
        }

        account_keys
    }

    fn get_token_accounts(meta: &TransactionStatusMeta, accounts: &Vec<Pubkey>) -> TokenAccounts {
        let mut token_accounts: TokenAccounts = HashMap::new();

        for token_balance in &meta.pre_token_balances  {
            let account_index = token_balance.account_index;
            let mint = Pubkey::from_str(token_balance.mint.as_str()).unwrap();
            let account: Pubkey = accounts[account_index as usize];
            let owner = Pubkey::from_str(token_balance.owner.as_str()).ok();  // Parse owner/authority

            token_accounts.insert(
                account,
                crate::TokenAccount {
                    account,
                    token_info: TokenInfo {
                        mint,
                        decimals: token_balance.ui_token_amount.clone().unwrap().decimals as u8,
                    },
                    owner,
                }
            );
        }

        token_accounts
    }
    
    fn build_program_instructions(
        compiled_instructions: Vec<CompiledInstruction>,
        inner_instructions_groups: Vec<InnerInstructions>,
        _log_messages: Vec<String>,
        accounts: Vec<Pubkey>,
        token_accounts: TokenAccounts,
    ) -> anyhow::Result<Vec<ProgramInstruction>> {
        let mut program_instructions = Vec::new();

        // Process each compiled instruction and its corresponding inner instructions
        for (idx, compiled_instruction) in compiled_instructions.iter().enumerate() {
            let program_id = accounts
                .get(compiled_instruction.program_id_index as usize)
                .ok_or_else(|| anyhow::anyhow!("Invalid program_id_index: {}", compiled_instruction.program_id_index))?;

            // Find inner instructions for this compiled instruction by matching the index
            let instructions: Vec<InnerInstruction> = inner_instructions_groups
                .iter()
                .filter(|inner_group| inner_group.index as usize == idx)
                .flat_map(|inner_group| inner_group.instructions.clone())
                .collect();

            // Build the instruction tree from inner instructions
            // let children = Self::build_instruction_tree_from_inner(&inner_instructions, &accounts)?;

            let mut arb_instructions = Vec::new();

            let mut idx = 0;
            while idx < instructions.len() {
                if let Some(instruction) = instructions.get(idx) {
                    let stack_height = instruction.stack_height.unwrap_or(1);

                    let mut inner_instructions = Vec::new();

                    if stack_height == 2 {
                        idx += 1;
                        while idx < instructions.len() {
                            if let Some(child_inner_instruction) = instructions.get(idx) {
                                let child_stack_height = child_inner_instruction.stack_height.unwrap_or(1);

                                if child_stack_height == 2 {
                                    break;
                                }

                                inner_instructions.push(child_inner_instruction);
                                idx += 1;
                            }
                        }

                        // Clone inner_instructions for parse to avoid move
                        let inner_instructions_for_parse = inner_instructions.clone();

                        match DexRegistry::global().parse(instruction, inner_instructions_for_parse, &accounts, &token_accounts) {
                            Ok(swap) => {
                                arb_instructions.push(ArbTransactionInstruction::DexSwap(swap));
                            }
                            Err(err) => {
                                // warn!("Signature: {}", signature);
                                warn!("Err: {:?}", err);
                                // save_transaction_update(tx_update.clone());

                                // Clone inner instructions to convert Vec<&InnerInstruction> to Vec<InnerInstruction>
                                let inner_instructions_owned: Vec<_> = inner_instructions.iter().map(|i| (*i).clone()).collect();
                                arb_instructions.push(ArbTransactionInstruction::Raw(instruction.clone(), inner_instructions_owned));
                            }
                        }
                    }
                }
            }

            program_instructions.push(ProgramInstruction {
                program_id: *program_id,
                arb_instructions,
            });
        }

        Ok(program_instructions)
    }

    // fn build_instruction_tree_from_inner(
    //     inner_instructions: &Vec<InnerInstruction>,
    //     accounts: &Vec<Pubkey>,
    // ) -> anyhow::Result<Vec<ProgramInstruction>> {
    //     let mut result = Vec::new();
    //     let mut idx = 0;

    //     while idx < inner_instructions.len() {
    //         let instruction = &inner_instructions[idx];
    //         let stack_height = instruction.stack_height.unwrap_or(1);

    //         // Only process instructions at stack height 2 (direct CPIs)
    //         if stack_height == 2 {
    //             let program_id = accounts
    //                 .get(instruction.program_id_index as usize)
    //                 .ok_or_else(|| anyhow::anyhow!("Invalid program_id_index: {}", instruction.program_id_index))?;

    //             // Collect all child instructions (stack_height > 2) following this instruction
    //             let mut child_instructions = Vec::new();
    //             let mut child_idx = idx + 1;

    //             while child_idx < inner_instructions.len() {
    //                 let child = &inner_instructions[child_idx];
    //                 let child_stack_height = child.stack_height.unwrap_or(1);

    //                 // Stop when we hit another instruction at the same level or higher
    //                 if child_stack_height <= 2 {
    //                     break;
    //                 }

    //                 child_instructions.push(child.clone());
    //                 child_idx += 1;
    //             }

    //             // Recursively build children tree (normalize stack heights)
    //             let children = if !child_instructions.is_empty() {
    //                 Self::build_nested_instruction_tree(&child_instructions, 3, accounts)?
    //             } else {
    //                 Vec::new()
    //             };

    //             result.push(ProgramInstruction {
    //                 program_id: *program_id,
    //                 // program_instruction: children,
    //             });

    //             // Move to the next instruction after all children
    //             idx = child_idx;
    //         } else {
    //             idx += 1;
    //         }
    //     }

    //     Ok(result)
    // }

    // fn build_nested_instruction_tree(
    //     instructions: &Vec<InnerInstruction>,
    //     current_height: u32,
    //     accounts: &Vec<Pubkey>,
    // ) -> anyhow::Result<Vec<ProgramInstruction>> {
    //     let mut result = Vec::new();
    //     let mut idx = 0;

    //     while idx < instructions.len() {
    //         let instruction = &instructions[idx];
    //         let stack_height = instruction.stack_height.unwrap_or(1);

    //         // Process instructions at the current height level
    //         if stack_height == current_height {
    //             let program_id = accounts
    //                 .get(instruction.program_id_index as usize)
    //                 .ok_or_else(|| anyhow::anyhow!("Invalid program_id_index: {}", instruction.program_id_index))?;

    //             // Collect children at deeper nesting levels
    //             let mut child_instructions = Vec::new();
    //             let mut child_idx = idx + 1;

    //             while child_idx < instructions.len() {
    //                 let child = &instructions[child_idx];
    //                 let child_stack_height = child.stack_height.unwrap_or(1);

    //                 // Stop when we hit an instruction at the same level or higher
    //                 if child_stack_height <= current_height {
    //                     break;
    //                 }

    //                 child_instructions.push(child.clone());
    //                 child_idx += 1;
    //             }

    //             // Recursively build deeper nested children
    //             let children = if !child_instructions.is_empty() {
    //                 Self::build_nested_instruction_tree(&child_instructions, current_height + 1, accounts)?
    //             } else {
    //                 Vec::new()
    //             };

    //             result.push(ProgramInstruction {
    //                 program_id: *program_id,
    //                 // program_instruction: children,
    //             });

    //             idx = child_idx;
    //         } else {
    //             idx += 1;
    //         }
    //     }

    //     Ok(result)
    // }
}

#[derive(Debug)]
pub struct ProgramInstruction {
    pub program_id: Pubkey,
    pub arb_instructions: Vec<ArbTransactionInstruction>,
}
