use std::collections::HashMap;
use std::sync::Arc;
use dashmap::DashMap;
use parking_lot::{RwLock};
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::TransactionError;
use crate::grpc::ConnectionContext;
use crate::token::TokenInfo;

pub mod dashboard;
pub mod dex;
pub mod grpc;
pub mod process;
pub mod token;

pub mod config;
pub mod logger;
pub mod utils;

pub struct ScannerContext {
    pub connections: Arc<RwLock<Vec<ConnectionContext>>>,
    pub transactions: Arc<RwLock<Vec<ProcessedTransaction>>>,
    pub mints: Arc<DashMap<String, Mint>>,
    pub token_account_data: Arc<DashMap<String, MintData>>,
    pub arb_programs: DashMap<String, Arc<(u64, u64)>>,
}

impl ScannerContext {
    pub fn new(arb_programs: Vec<String>) -> Self {
        let arb_programs_map = DashMap::new();
        for program in arb_programs {
            arb_programs_map.insert(program, Arc::new((0u64, 0u64)));
        }

        Self {
            connections: Arc::new(RwLock::new(Vec::new())),
            transactions: Arc::new(RwLock::new(Vec::new())),
            mints: Arc::new(DashMap::new()),
            token_account_data: Arc::new(DashMap::new()),
            arb_programs: arb_programs_map,
        }
    }

    pub fn add_transaction(&self, tx: ProcessedTransaction) {
        // Update arb program counters based on transaction success/failure
        for ins in &tx.instructions {
            if let Some(mut arb_program) = self.arb_programs.get_mut(&ins.program_id) {
                let (success_count, fail_count) = &**arb_program;
                if tx.err.is_none() {
                    *arb_program = Arc::new((*success_count + 1, *fail_count));
                } else {
                    *arb_program = Arc::new((*success_count, *fail_count + 1));
                }
            }
        }
        self.transactions.write().push(tx);
    }

    pub fn remove_transactions(&self, tx_signatures: Vec<String>) {
        self.transactions.write().retain(|tx| !tx_signatures.contains(&tx.signature));
    }

    pub fn add_intermediate_mint(&self, mint: Mint) {
        self.mints.insert(mint.program_id.clone(), mint);
    }

    pub fn add_mint_data(&self, token_account: String, data: MintData) {
        self.token_account_data.insert(token_account, data);
    }
}

pub struct ProcessedTransaction {
    pub signature: String,
    pub err: Option<TransactionError>,
    pub instructions: Vec<ParsedInstruction>,
    pub timestamp: u64,
}

pub struct ParsedInstruction {
    pub program_id: String,
    pub inner_instrucions: Vec<ParseInnerInstructionEnum>,
}

pub enum ParseInnerInstructionEnum {
    Swap(SwapInstruction),
    Other,
}

pub struct SwapInstruction {
    pub swap_program_id: Pubkey,
    pub pools: Vec<Pubkey>,
    pub amount_in: u64,
    pub amount_out: u64,
    pub token_in: String, // mint program_id
    pub token_out: String, // mint program_id
    pub fees: Vec<(String, u64)>,
}

#[derive(Clone, Debug)]
pub struct Mint {
    pub program_id: String,
    pub decimals: u32,
}

pub struct MintData {
    pub mint: String,
    pub liquidity:  u64,
}

#[derive(Debug, Clone)]
pub struct TokenAccount {
    pub account: Pubkey,
    pub token_info: TokenInfo,
    pub owner: Option<Pubkey>,
}

pub type TokenAccounts = HashMap<Pubkey, TokenAccount>;