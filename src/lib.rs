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
use crate::token::TokenInfo;


pub struct ScannerContext {
    pub connections: Arc<DashMap<String, grpc::GrpcConnectionStatus>>,
    pub transactions: Arc<DashMap<Signature, ArbTransaction>>,
    pub temp_txs: Arc<DashMap<Signature, SubscribeUpdateTransaction>>,
    pub mints: Arc<DashMap<Pubkey, MintContext>>,
}

impl ScannerContext {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(DashMap::new()),
            transactions: Arc::new(DashMap::new()), 
            temp_txs: Arc::new(DashMap::new()),
            mints: Arc::new(DashMap::new()),
        }
    }

    pub fn insert_transaction(&self, tx: ArbTransaction) {
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
}

pub struct ArbTransaction {
    pub signature: Signature,
    pub is_success: bool,
    pub instructions: Vec<ArbTransactionInstruction>,
    pub timestamp:  Option<i64>,
}

#[derive(Debug)]
pub enum ArbTransactionInstruction {
    Raw(InnerInstruction),
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
    pub owner: Pubkey,
    pub liquidity: u64,
}

#[derive(Debug)]
pub struct TokenAccount {
    account: Pubkey,
    token_info: TokenInfo,
}

pub type TokenAccounts = HashMap<Pubkey, TokenAccount>;

pub mod prelude {
    pub use super::{ArbTransactionInstruction, TokenAccount};
    pub use super::token::TokenWithAmount;
}