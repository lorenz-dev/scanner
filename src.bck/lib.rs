pub mod cli;
pub mod config;
pub mod dex;
pub mod dashboard;
pub mod grpc;
pub mod process;
pub mod token;
pub mod utils;

use std::collections::HashMap;
use std::sync::Arc;
use std::str::FromStr;

use dashmap::DashMap;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use yellowstone_grpc_proto::geyser::SubscribeUpdateTransaction;
use yellowstone_grpc_proto::prelude::InnerInstruction;
use crate::dex::DexSwap;
use crate::process::ProgramInstruction;
use crate::token::TokenInfo;


pub struct ScannerContext {
    pub connections: Arc<DashMap<String, grpc::GrpcConnectionStatus>>,
    pub transactions: Arc<DashMap<Signature, ArbTransaction>>,
    pub temp_txs: Arc<DashMap<Signature, SubscribeUpdateTransaction>>,
    pub mints: Arc<DashMap<Pubkey, MintContext>>,
    pub known_pools: Arc<DashMap<Pubkey, ()>>,
    pub known_dex_programs: Arc<DashMap<Pubkey, ()>>,
    pub arb_programs: Vec<Pubkey>,
    pub arb_program_counts: Arc<DashMap<Pubkey, ArbProgramStats>>,
}

impl ScannerContext {
    pub fn new(arb_programs: Vec<Pubkey>) -> Self {
        let arb_program_counts = Arc::new(DashMap::new());

        // Initialize counters for all arb programs
        for program in &arb_programs {
            arb_program_counts.insert(*program, ArbProgramStats::default());
        }

        Self {
            connections: Arc::new(DashMap::new()),
            transactions: Arc::new(DashMap::new()),
            temp_txs: Arc::new(DashMap::new()),
            mints: Arc::new(DashMap::new()),
            known_pools: Arc::new(DashMap::new()),
            known_dex_programs: Arc::new(DashMap::new()),
            arb_programs,
            arb_program_counts,
        }
    }

    pub fn insert_transaction(&self, tx: ArbTransaction) {
        // Only increment counters if this transaction belongs to a configured arb program
        for program_instruction in &tx.program_instructions {
            if let Some(mut stats) = self.arb_program_counts.get_mut(&program_instruction.program_id) {
                if tx.is_success {
                    stats.success_count += 1;
                } else {
                    stats.fail_count += 1;
                }
            }
        }

        self.transactions.insert(tx.signature, tx);
    }

    pub fn insert_received_transaction(&self, tx: SubscribeUpdateTransaction) {    
        let tx_info = tx.clone().transaction.unwrap();
        let sig_str = bs58::encode(&tx_info.signature).into_string();
        let signature = Signature::from_str(&sig_str).unwrap();

        self.temp_txs.insert(signature, tx);
    }

    pub fn remove_received_transaction(&self, tx: SubscribeUpdateTransaction) {
        let tx_info = tx.clone().transaction.unwrap();
        let sig_str = bs58::encode(&tx_info.signature).into_string();
        let signature = Signature::from_str(&sig_str).unwrap();

        self.temp_txs.remove(&signature);
    }

    pub fn insert_mint(&self, ctx: MintContext) {
        self.mints.insert(ctx.mint, ctx);
    }

    pub fn upsert_pool_liquidity(&self, mint: Pubkey, owner: Pubkey, liquidity: u64) {
        use std::time::SystemTime;

        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let mint_ctx = self.mints.entry(mint)
            .or_insert_with(|| MintContext {
                mint,
                decimals: None,
                pools: Arc::new(DashMap::new()),
            });

        mint_ctx.pools.insert(owner, PoolLiquidity {
            owner,
            liquidity,
            last_updated: timestamp,
        });
    }

    pub fn upsert_mint_decimals(&self, mint: Pubkey, decimals: u8) {
        let mut mint_ctx = self.mints.entry(mint)
            .or_insert_with(|| MintContext {
                mint,
                decimals: None,
                pools: Arc::new(DashMap::new()),
            });

        // Update decimals if not already set
        if mint_ctx.decimals.is_none() {
            mint_ctx.decimals = Some(decimals);
        }
    }
}

pub struct ArbTransaction {
    pub signature: Signature,
    pub is_success: bool,
    pub instructions: Vec<ArbTransactionInstruction>,
    pub program_instructions: Vec<ProgramInstruction>,
    pub timestamp: i64,
}

#[derive(Default, Clone)]
pub struct ArbProgramStats {
    pub success_count: usize,
    pub fail_count: usize,
}

#[derive(Debug)]
pub enum ArbTransactionInstruction {
    Raw(InnerInstruction, Vec<InnerInstruction>),
    DexSwap(DexSwap),
}

#[derive(Default)]
pub struct SwapInfo {
    pub program_id: Pubkey,
    pub fees: Vec<token::TokenAmount>,
    pub amount_out: token::TokenAmount,
    pub amount_in: token::TokenAmount,
    pub mint_in: Pubkey,
    pub mint_out: Pubkey,
}

pub struct SwapInstruction {
    pub instruction: InnerInstruction,
    pub inner_instructions: Vec<InnerInstruction>,
}

pub struct MintContext {
    pub mint: Pubkey,
    pub decimals: Option<u8>,
    pub pools: Arc<DashMap<Pubkey, PoolLiquidity>>,
}

pub struct PoolLiquidity {
    pub owner: Pubkey,
    pub liquidity: u64,
    pub last_updated: i64,
}

#[derive(Debug, Clone)]
pub struct TokenAccount {
    pub account: Pubkey,
    pub token_info: TokenInfo,
    pub owner: Option<Pubkey>,  // Owner/authority from TokenBalance (PDA)
}

pub type TokenAccounts = HashMap<Pubkey, TokenAccount>;

pub mod prelude {
    pub use super::{ArbTransactionInstruction, TokenAccount};
    pub use super::token::TokenWithAmount;
}