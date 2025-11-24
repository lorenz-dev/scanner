use std::{collections::HashMap, str::FromStr};

use anyhow::Result;
use chrono::offset;
use solana_sdk::{program_error::ACCOUNT_DATA_TOO_SMALL, pubkey::Pubkey, signature::Signature};
use yellowstone_grpc_proto::prelude::{
    SubscribeUpdateAccount, SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo, Transaction, TransactionStatusMeta
};
use tracing::{info, warn};

use crate::{ArbTransaction, ArbTransactionInstruction, MintContext, TokenAccounts, dex::{DexParserError, DexRegistry}, token::TokenInfo, utils::save_transaction_update};

pub struct Process {
}

pub struct ProcessedAccount {
    mint: Pubkey,
    owner: Pubkey,
    liquidity: u64,
}

pub enum ProcessedTransaction {
    SuccessfulTransaction(),
    FailedTransaction(ArbTransaction),
}

impl Process {
    pub fn process_account(account: &SubscribeUpdateAccount) -> MintContext {
        // SPL Token account structure:
        // - mint: Pubkey at offset 0 (32 bytes)
        // - owner: Pubkey at offset 32 (32 bytes)
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
        info!("Mint: {}", mint);

        let owner = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .unwrap()
        );
        offset += 32;
        info!("Owner: {}", owner);

        let liquidity = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .unwrap()
        );
        info!("Liquidity: {}", liquidity);

        MintContext {
            mint,
            owner, 
            liquidity,
        }
    }

    pub fn process_transaction<OnMintAccount>(tx_update: SubscribeUpdateTransaction, on_mint_account: OnMintAccount) -> ArbTransaction
    where
        OnMintAccount: Fn(&Pubkey),
    {
        let tx_info = tx_update.clone().transaction.unwrap();
        let tx = tx_info.clone().transaction.unwrap();
        let meta = tx_info.clone().meta.unwrap();

        let sig_str = bs58::encode(&tx_info.signature).into_string();
        let signature = Signature::from_str(&sig_str).unwrap();

        let accounts = Process::get_accounts(&tx_info);

        let token_accounts = Process::get_token_accounts(&meta, &accounts);

        let mut arb_instructions = Vec::new();

        for token_balance in meta.post_token_balances {
            let account = &accounts[token_balance.account_index as usize];
            on_mint_account(account);
        }

        for inner_instructions in meta.inner_instructions {
            let mut idx = 0;

            let instructions = inner_instructions.instructions;

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

                        match DexRegistry::global().parse(instruction, inner_instructions, &accounts, &token_accounts) {
                            Ok(swap) => {
                                arb_instructions.push(ArbTransactionInstruction::DexSwap(swap));
                            }
                            Err(err) => {
                                // warn!("Signature: {}", signature);
                                warn!("Err: {:?}", err);
                                save_transaction_update(tx_update.clone());
                                arb_instructions.push(ArbTransactionInstruction::Raw(instruction.clone()));

                                // info!("ArbTransaction: {:#?}", arb_instructions);
                            }
                        }
                    }
                }
            }
        }

        ArbTransaction {
            signature,
            is_success: meta.err.is_some(),
            instructions: arb_instructions,
            timestamp: None,
        }
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

            token_accounts.insert(
                account,
                crate::TokenAccount {
                    account,
                    token_info: TokenInfo {
                        mint,
                        decimals: token_balance.ui_token_amount.clone().unwrap().decimals as u8,
                    }
                }
            );
        }

        token_accounts
    }
}
