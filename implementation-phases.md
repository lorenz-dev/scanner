# Solana Scanner Refactoring - Implementation Phases

This document provides a step-by-step guide for refactoring the Solana scanner application from scratch in a clean project. Follow these phases sequentially to ensure proper foundation and dependencies.

---

## Pre-Implementation Checklist

Before starting, ensure you have:
- [ ] Clean Rust project initialized (`cargo new scanner --bin`)
- [ ] Required dependencies in Cargo.toml (see Appendix A)
- [ ] Access to the original codebase for reference
- [ ] Test configuration files (config.toml)

---

## Phase 1: Error Handling Foundation

**Duration:** 2-3 days
**Goal:** Establish robust error handling before implementing any business logic.

### Step 1.1: Create Error Module

**File:** `src/error.rs`

```rust
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ScannerError {
    #[error("Account parsing error: {0}")]
    AccountParsing(#[from] AccountParsingError),

    #[error("Transaction parsing error: {0}")]
    TransactionParsing(#[from] TransactionParsingError),

    #[error("gRPC connection error: {0}")]
    GrpcConnection(#[from] GrpcConnectionError),

    #[error("DEX parsing error: {0}")]
    DexParsing(#[from] DexParserError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),
}

#[derive(Error, Debug)]
pub enum AccountParsingError {
    #[error("Account info is missing")]
    MissingAccountInfo,

    #[error("Invalid data length: expected {expected}, got {actual}")]
    InvalidDataLength { expected: usize, actual: usize },

    #[error("Invalid mint pubkey: {0}")]
    InvalidMintPubkey(String),

    #[error("Invalid authority pubkey: {0}")]
    InvalidAuthorityPubkey(String),

    #[error("Malformed token account structure")]
    MalformedTokenAccount,
}

#[derive(Error, Debug)]
pub enum TransactionParsingError {
    #[error("Transaction info is missing")]
    MissingTransactionInfo,

    #[error("Transaction is missing")]
    MissingTransaction,

    #[error("Transaction metadata is missing")]
    MissingMeta,

    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    #[error("Instruction index {index} out of bounds (max: {max})")]
    InstructionIndexOutOfBounds { index: usize, max: usize },

    #[error("Invalid format: {0}")]
    InvalidFormat(String),
}

#[derive(Error, Debug)]
pub enum GrpcConnectionError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(#[from] tonic::Status),

    #[error("Subscription failed: {0}")]
    SubscriptionFailed(String),

    #[error("Empty subscription filters")]
    EmptyFilters,
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Configuration file not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Invalid configuration format: {0}")]
    InvalidFormat(String),

    #[error("Missing required field: {0}")]
    MissingRequiredField(String),
}

#[derive(Error, Debug)]
pub enum DexParserError {
    #[error("Parser not found for program: {0}")]
    ParserNotFound(String),

    #[error("Invalid discriminator: {0}")]
    InvalidDiscriminator(String),

    #[error("Invalid program ID index: {index} (accounts length: {accounts_len})")]
    InvalidProgramIdIndex { index: u8, accounts_len: usize },

    #[error("Account index {index} out of bounds (accounts length: {accounts_len})")]
    AccountIndexOutOfBounds { index: u8, accounts_len: usize },

    #[error("Insufficient instruction data: expected at least {expected} bytes, got {actual}")]
    InsufficientData { expected: usize, actual: usize },
}

// Re-export for convenience
pub type Result<T> = std::result::Result<T, ScannerError>;
```

**Add to lib.rs:**
```rust
pub mod error;
pub use error::{Result, ScannerError};
```

### Step 1.2: Verification

- [ ] `cargo check` passes
- [ ] All error types compile
- [ ] Error messages provide context

---

## Phase 2: Core Data Structures

**Duration:** 1-2 days
**Goal:** Define all core types with proper error handling built-in.

### Step 2.1: Create lib.rs with Core Types

**File:** `src/lib.rs`

```rust
pub mod error;
pub mod config;
pub mod cli;

use std::sync::Arc;
use dashmap::DashMap;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use yellowstone_grpc_proto::geyser::SubscribeUpdateTransaction;
use yellowstone_grpc_proto::prelude::InnerInstruction;

// Re-exports
pub use error::{Result, ScannerError};

// Constants
pub const TOKEN_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const TOKEN_2022_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

/// Resource limits for DashMaps
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    pub max_known_pools: usize,      // Default: 10,000
    pub max_known_dex_programs: usize, // Default: 100
    pub max_mints: usize,             // Default: 5,000
    pub max_transactions: usize,      // Default: 1,000
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_known_pools: 10_000,
            max_known_dex_programs: 100,
            max_mints: 5_000,
            max_transactions: 1_000,
        }
    }
}

/// Main application context with thread-safe shared state
pub struct ScannerContext {
    pub connections: Arc<DashMap<String, crate::grpc::GrpcConnectionStatus>>,
    pub transactions: Arc<DashMap<Signature, ParsedTransaction>>,
    pub pending_transactions: Arc<DashMap<Signature, SubscribeUpdateTransaction>>,
    pub mints: Arc<DashMap<Pubkey, MintContext>>,
    pub pool_registry: Arc<DashMap<Pubkey, ()>>,
    pub dex_program_registry: Arc<DashMap<Pubkey, ()>>,
    pub arb_programs: Vec<Pubkey>,
    pub arb_program_stats: Arc<DashMap<Pubkey, ProgramStatistics>>,
    pub limits: ResourceLimits,
}

impl ScannerContext {
    pub fn new(arb_programs: Vec<Pubkey>) -> Self {
        let arb_program_stats = Arc::new(DashMap::new());

        // Initialize statistics for all arb programs
        for program in &arb_programs {
            arb_program_stats.insert(*program, ProgramStatistics::default());
        }

        Self {
            connections: Arc::new(DashMap::new()),
            transactions: Arc::new(DashMap::new()),
            pending_transactions: Arc::new(DashMap::new()),
            mints: Arc::new(DashMap::new()),
            pool_registry: Arc::new(DashMap::new()),
            dex_program_registry: Arc::new(DashMap::new()),
            arb_programs,
            arb_program_stats,
            limits: ResourceLimits::default(),
        }
    }

    /// Insert a parsed transaction and update statistics
    pub fn insert_transaction(&self, tx: ParsedTransaction) {
        // Update program statistics
        for program_instruction in &tx.program_instructions {
            if let Some(mut stats) = self.arb_program_stats.get_mut(&program_instruction.program_id) {
                if tx.is_success {
                    stats.success_count += 1;
                } else {
                    stats.fail_count += 1;
                }
            }
        }

        self.transactions.insert(tx.signature, tx);
        self.enforce_transaction_limit();
    }

    /// Insert a pending transaction (safe version without unwrap)
    pub fn insert_pending_transaction(&self, tx: SubscribeUpdateTransaction) -> Result<()> {
        let tx_info = tx.transaction.as_ref()
            .ok_or(error::TransactionParsingError::MissingTransactionInfo)?;

        let sig_str = bs58::encode(&tx_info.signature).into_string();
        let signature = solana_sdk::signature::Signature::from_str(&sig_str)
            .map_err(|_| error::TransactionParsingError::InvalidSignature(sig_str))?;

        self.pending_transactions.insert(signature, tx);
        Ok(())
    }

    /// Remove a pending transaction (safe version without unwrap)
    pub fn remove_pending_transaction(&self, tx: SubscribeUpdateTransaction) -> Result<()> {
        let tx_info = tx.transaction.as_ref()
            .ok_or(error::TransactionParsingError::MissingTransactionInfo)?;

        let sig_str = bs58::encode(&tx_info.signature).into_string();
        let signature = solana_sdk::signature::Signature::from_str(&sig_str)
            .map_err(|_| error::TransactionParsingError::InvalidSignature(sig_str))?;

        self.pending_transactions.remove(&signature);
        Ok(())
    }

    /// Update pool liquidity with timestamp
    pub fn upsert_pool_liquidity(&self, mint: Pubkey, owner: Pubkey, liquidity: u64) {
        use std::time::SystemTime;

        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("System time before UNIX epoch")
            .as_secs() as i64;

        let mut mint_ctx = self.mints.entry(mint)
            .or_insert_with(|| MintContext {
                mint,
                decimals: None,
                pools: Arc::new(DashMap::new()),
                last_accessed: timestamp,
            });

        mint_ctx.last_accessed = timestamp;
        mint_ctx.pools.insert(owner, PoolLiquidity {
            owner,
            liquidity,
            last_updated: timestamp,
        });
    }

    /// Update mint decimals
    pub fn upsert_mint_decimals(&self, mint: Pubkey, decimals: u8) {
        use std::time::SystemTime;

        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("System time before UNIX epoch")
            .as_secs() as i64;

        let mut mint_ctx = self.mints.entry(mint)
            .or_insert_with(|| MintContext {
                mint,
                decimals: None,
                pools: Arc::new(DashMap::new()),
                last_accessed: timestamp,
            });

        mint_ctx.last_accessed = timestamp;
        if mint_ctx.decimals.is_none() {
            mint_ctx.decimals = Some(decimals);
        }
    }

    /// Enforce transaction limit with LRU eviction
    fn enforce_transaction_limit(&self) {
        if self.transactions.len() > self.limits.max_transactions {
            let target = self.limits.max_transactions * 9 / 10;

            // Collect entries with timestamps
            let mut entries: Vec<_> = self.transactions.iter()
                .map(|entry| (*entry.key(), entry.value().timestamp))
                .collect();

            // Sort by oldest first
            entries.sort_by_key(|(_, time)| *time);

            // Remove oldest entries
            let to_remove = entries.len().saturating_sub(target);
            for (sig, _) in entries.iter().take(to_remove) {
                self.transactions.remove(sig);
                tracing::debug!(signature = %sig, "Evicted transaction (LRU)");
            }
        }
    }

    /// Enforce all resource limits
    pub fn enforce_limits(&self) {
        // Mints with LRU
        if self.mints.len() > self.limits.max_mints {
            let target = self.limits.max_mints * 9 / 10;

            let mut entries: Vec<_> = self.mints.iter()
                .map(|entry| (*entry.key(), entry.value().last_accessed))
                .collect();

            entries.sort_by_key(|(_, time)| *time);

            let to_remove = entries.len().saturating_sub(target);
            for (mint, _) in entries.iter().take(to_remove) {
                self.mints.remove(mint);
                tracing::debug!(mint = %mint, "Evicted mint (LRU)");
            }
        }

        // Pool registry (simple truncation)
        if self.pool_registry.len() > self.limits.max_known_pools {
            let target = self.limits.max_known_pools * 9 / 10;
            while self.pool_registry.len() > target {
                if let Some(entry) = self.pool_registry.iter().next() {
                    let key = *entry.key();
                    drop(entry);
                    self.pool_registry.remove(&key);
                }
            }
        }

        // DEX program registry
        if self.dex_program_registry.len() > self.limits.max_known_dex_programs {
            let target = self.limits.max_known_dex_programs * 9 / 10;
            while self.dex_program_registry.len() > target {
                if let Some(entry) = self.dex_program_registry.iter().next() {
                    let key = *entry.key();
                    drop(entry);
                    self.dex_program_registry.remove(&key);
                }
            }
        }
    }
}

/// Parsed transaction (renamed from ArbTransaction)
pub struct ParsedTransaction {
    pub signature: Signature,
    pub is_success: bool,
    pub instructions: Vec<ParsedInstruction>,
    pub program_instructions: Vec<ProgramInstruction>,
    pub timestamp: i64,
}

#[derive(Default, Clone)]
pub struct ProgramStatistics {
    pub success_count: usize,
    pub fail_count: usize,
}

#[derive(Debug)]
pub enum ParsedInstruction {
    Raw(InnerInstruction, Vec<InnerInstruction>),
    DexSwap(crate::dex::DexSwap),
}

pub struct MintContext {
    pub mint: Pubkey,
    pub decimals: Option<u8>,
    pub pools: Arc<DashMap<Pubkey, PoolLiquidity>>,
    pub last_accessed: i64,  // For LRU eviction
}

pub struct PoolLiquidity {
    pub owner: Pubkey,
    pub liquidity: u64,
    pub last_updated: i64,
}

pub struct ProgramInstruction {
    pub program_id: Pubkey,
    pub arb_instructions: Vec<ParsedInstruction>,
}

// Token-related types
pub mod token {
    use solana_sdk::pubkey::Pubkey;

    #[derive(Debug, Clone)]
    pub struct TokenInfo {
        pub mint: Pubkey,
        pub amount: u64,
    }

    #[derive(Debug, Clone)]
    pub struct TokenAmount {
        pub mint: Pubkey,
        pub amount: u64,
    }
}

pub use token::{TokenInfo, TokenAmount};
```

### Step 2.2: Verification

- [ ] `cargo check` passes
- [ ] All types compile
- [ ] No unwrap() calls in ScannerContext methods
- [ ] Resource limits properly enforced

---

## Phase 3: Configuration & CLI

**Duration:** 1 day
**Goal:** Set up configuration loading and command-line parsing with proper error handling.

### Step 3.1: Create Config Module

**File:** `src/config.rs`

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;
use crate::error::{ConfigError, Result};

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub grpc: Vec<GrpcConfig>,
    pub arb_programs: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GrpcConfig {
    pub name: String,
    pub endpoint: String,
    pub x_token: Option<String>,
}

impl Config {
    /// Load configuration from file with proper error handling
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // Check file existence
        if !path.exists() {
            return Err(ConfigError::FileNotFound(path.to_path_buf()).into());
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::InvalidFormat(e.to_string()))?;

        // Determine format by extension
        let config = if path.extension().and_then(|s| s.to_str()) == Some("toml") {
            toml::from_str(&content)
                .map_err(|e| ConfigError::InvalidFormat(e.to_string()))?
        } else {
            serde_json::from_str(&content)
                .map_err(|e| ConfigError::InvalidFormat(e.to_string()))?
        };

        Ok(config)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.grpc.is_empty() {
            return Err(ConfigError::MissingRequiredField("grpc".to_string()).into());
        }

        if self.arb_programs.is_empty() {
            return Err(ConfigError::MissingRequiredField("arb_programs".to_string()).into());
        }

        Ok(())
    }
}
```

### Step 3.2: Create CLI Module

**File:** `src/cli.rs`

```rust
use clap::Parser;

#[derive(Parser, Debug)]
#[clap(name = "scanner")]
#[clap(about = "Solana transaction scanner with multi-gRPC support")]
pub struct Args {
    /// Path to configuration file (TOML or JSON)
    #[clap(short, long, default_value = "config.toml")]
    pub config: String,
}
```

### Step 3.3: Verification

- [ ] Config loads from both TOML and JSON
- [ ] Validation catches missing fields
- [ ] Errors provide helpful messages

---

## Phase 4: Logging Setup

**Duration:** 0.5 days
**Goal:** Configure structured logging with proper levels.

### Step 4.1: Initialize Logging in main.rs

**File:** `src/main.rs` (partial)

```rust
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

fn init_logging() {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(true)
                .with_line_number(true)
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(
                    std::fs::File::create("scanner.log")
                        .expect("Failed to create log file")
                )
                .with_ansi(false)
        )
        .init();

    tracing::info!("Logging initialized");
}
```

### Step 4.2: Logging Guidelines Reference

Create a quick reference:

```rust
// ERROR: Application cannot continue
tracing::error!("Failed to connect to gRPC: {}", err);

// WARN: Recoverable errors
tracing::warn!(signature = %sig, "Failed to parse transaction, skipping");

// INFO: Important state changes only
tracing::info!("Connected to gRPC endpoint: {}", name);

// DEBUG: Processing details (hot loops should use this)
tracing::debug!(mint = %mint, account_count = count, "Subscribed to mint accounts");

// TRACE: Full data dumps
tracing::trace!(?transaction, "Raw transaction data");
```

---

## Phase 5: Parsing Module (Account & Transaction)

**Duration:** 3-4 days
**Goal:** Implement all parsing logic with proper error handling, no panics.

### Step 5.1: Create Parsing Module Structure

```bash
mkdir -p src/parsing
touch src/parsing/mod.rs
touch src/parsing/account.rs
touch src/parsing/transaction.rs
touch src/parsing/instructions.rs
```

### Step 5.2: Account Parsing

**File:** `src/parsing/account.rs`

```rust
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::SubscribeUpdateAccount;
use crate::error::{AccountParsingError, Result};

/// Parsed account information
pub struct ParsedAccount {
    pub mint: Pubkey,
    pub authority: Pubkey,  // The authority from token account data (PDA)
    pub owner: Pubkey,      // The program owner from account metadata
    pub liquidity: u64,
}

/// Parse SPL Token account with proper error handling
pub fn parse_token_account(account: &SubscribeUpdateAccount) -> Result<ParsedAccount> {
    // Get account info with proper error handling
    let account_info = account.account.as_ref()
        .ok_or(AccountParsingError::MissingAccountInfo)?;

    let data = &account_info.data;

    // Validate data length (SPL Token account = 165 bytes)
    if data.len() < 72 {
        return Err(AccountParsingError::InvalidDataLength {
            expected: 165,
            actual: data.len(),
        }.into());
    }

    // Parse mint (offset 0-31)
    let mint_bytes: [u8; 32] = data[0..32]
        .try_into()
        .map_err(|_| AccountParsingError::InvalidMintPubkey("Invalid byte length".to_string()))?;
    let mint = Pubkey::new_from_array(mint_bytes);

    // Parse authority (offset 32-63)
    let authority_bytes: [u8; 32] = data[32..64]
        .try_into()
        .map_err(|_| AccountParsingError::InvalidAuthorityPubkey("Invalid byte length".to_string()))?;
    let authority = Pubkey::new_from_array(authority_bytes);

    // Parse liquidity (offset 64-71)
    let liquidity_bytes: [u8; 8] = data[64..72]
        .try_into()
        .map_err(|_| AccountParsingError::MalformedTokenAccount)?;
    let liquidity = u64::from_le_bytes(liquidity_bytes);

    // Parse program owner
    let owner_bytes: [u8; 32] = account_info.owner
        .as_slice()
        .try_into()
        .map_err(|_| AccountParsingError::MalformedTokenAccount)?;
    let owner = Pubkey::new_from_array(owner_bytes);

    Ok(ParsedAccount {
        mint,
        authority,
        owner,
        liquidity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_token_account_invalid_length() {
        // Test with data that's too short
        // ...
    }
}
```

### Step 5.3: Transaction Parsing

**File:** `src/parsing/transaction.rs`

```rust
use std::str::FromStr;
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction;
use crate::error::{TransactionParsingError, Result};
use crate::ParsedTransaction;

/// Parse transaction with error handling
pub fn parse_transaction<F>(
    tx_update: SubscribeUpdateTransaction,
    on_mint_account: F,
) -> Result<ParsedTransaction>
where
    F: Fn(&Pubkey, u8, &[Pubkey]),
{
    // Extract transaction info with proper error handling
    let tx_info = tx_update.transaction.as_ref()
        .ok_or(TransactionParsingError::MissingTransactionInfo)?;

    let tx = tx_info.transaction.as_ref()
        .ok_or(TransactionParsingError::MissingTransaction)?;

    let meta = tx_info.meta.as_ref()
        .ok_or(TransactionParsingError::MissingMeta)?;

    // Parse signature safely
    let sig_str = bs58::encode(&tx_info.signature).into_string();
    let signature = Signature::from_str(&sig_str)
        .map_err(|_| TransactionParsingError::InvalidSignature(sig_str.clone()))?;

    // Extract accounts
    let accounts = extract_accounts(tx_info)?;

    // Extract token accounts
    let token_accounts = extract_token_accounts(meta, &accounts)?;

    // Build program instructions (separate module)
    let program_instructions = crate::parsing::instructions::build_program_instructions(
        tx.message.as_ref()
            .ok_or(TransactionParsingError::MissingTransaction)?
            .instructions.clone(),
        meta.inner_instructions.clone(),
        meta.log_messages.clone(),
        accounts.clone(),
        token_accounts.clone(),
    )?;

    // Determine success status
    let is_success = meta.err.is_none();

    // Get timestamp
    let timestamp = chrono::Utc::now().timestamp();

    // Process intermediate mints (call callback)
    process_intermediate_mints(&program_instructions, &on_mint_account);

    Ok(ParsedTransaction {
        signature,
        is_success,
        instructions: vec![], // Populated from program_instructions
        program_instructions,
        timestamp,
    })
}

fn extract_accounts(tx_info: &yellowstone_grpc_proto::prelude::SubscribeUpdateTransactionInfo) -> Result<Vec<Pubkey>> {
    // Implementation with error handling
    todo!("Extract accounts from transaction")
}

fn extract_token_accounts(
    meta: &yellowstone_grpc_proto::prelude::TransactionStatusMeta,
    accounts: &[Pubkey],
) -> Result<std::collections::HashMap<Pubkey, crate::token::TokenInfo>> {
    // Implementation with error handling
    todo!("Extract token accounts from metadata")
}

fn process_intermediate_mints<F>(
    program_instructions: &[crate::ProgramInstruction],
    on_mint_account: &F,
)
where
    F: Fn(&Pubkey, u8, &[Pubkey]),
{
    // Implementation
    todo!("Process intermediate mints and call callback")
}
```

### Step 5.4: Verification

- [ ] No `.unwrap()` or `.expect()` in parsing code
- [ ] All parsing functions return `Result<T, E>`
- [ ] Error messages include context
- [ ] Test with malformed data

---

## Phase 6: DEX Parsing Module

**Duration:** 2-3 days
**Goal:** Implement DEX parser registry with bounds checking.

### Step 6.1: Create DEX Module

**File:** `src/dex/mod.rs`

```rust
use std::collections::HashMap;
use std::sync::OnceLock;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::InnerInstruction;
use crate::error::{DexParserError, Result};

pub mod raydium_cpmm;
pub mod raydium_clmm;
// ... other DEX modules

pub struct DexSwap {
    pub pools: Vec<Pubkey>,
    pub pool_owner: Pubkey,
    pub token_in: crate::TokenAmount,
    pub token_out: crate::TokenAmount,
    pub fees: Vec<crate::TokenAmount>,
}

pub trait DexParser: Send + Sync {
    fn parse(
        &self,
        instruction: &InnerInstruction,
        accounts: &[Pubkey],
        inner_instructions: &[InnerInstruction],
        token_accounts: &std::collections::HashMap<Pubkey, crate::token::TokenInfo>,
    ) -> Result<DexSwap>;
}

pub struct DexRegistry {
    parsers: HashMap<Pubkey, Box<dyn DexParser>>,
}

static DEX_REGISTRY: OnceLock<DexRegistry> = OnceLock::new();

impl DexRegistry {
    pub fn global() -> &'static DexRegistry {
        DEX_REGISTRY.get_or_init(|| DexRegistry::new())
    }

    pub fn new() -> Self {
        let mut parsers: HashMap<Pubkey, Box<dyn DexParser>> = HashMap::new();

        // Register all DEX parsers
        // parsers.insert(raydium_cpmm::PROGRAM_ID, Box::new(raydium_cpmm::RaydiumCpmm));

        Self { parsers }
    }

    pub fn parse(
        &self,
        instruction: &InnerInstruction,
        accounts: &[Pubkey],
        inner_instructions: &[InnerInstruction],
        token_accounts: &std::collections::HashMap<Pubkey, crate::token::TokenInfo>,
    ) -> Result<DexSwap> {
        // Bounds check before accessing accounts
        if instruction.program_id_index as usize >= accounts.len() {
            return Err(DexParserError::InvalidProgramIdIndex {
                index: instruction.program_id_index,
                accounts_len: accounts.len(),
            }.into());
        }

        let program_id = &accounts[instruction.program_id_index as usize];

        if let Some(parser) = self.parsers.get(program_id) {
            parser.parse(instruction, accounts, inner_instructions, token_accounts)
        } else {
            Err(DexParserError::ParserNotFound(program_id.to_string()).into())
        }
    }
}
```

### Step 6.2: Create DEX Parser Helper

**File:** `src/dex/helpers.rs`

```rust
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::InnerInstruction;
use crate::error::{DexParserError, Result};

/// Safely resolve account indices with bounds checking
pub fn resolve_account_references(
    instruction: &InnerInstruction,
    accounts: &[Pubkey],
) -> Result<Vec<Pubkey>> {
    instruction.accounts
        .iter()
        .map(|&idx| {
            accounts.get(idx as usize)
                .copied()
                .ok_or_else(|| DexParserError::AccountIndexOutOfBounds {
                    index: idx,
                    accounts_len: accounts.len(),
                }.into())
        })
        .collect()
}

/// Validate instruction data length
pub fn validate_instruction_data(data: &[u8], expected: usize) -> Result<()> {
    if data.len() < expected {
        return Err(DexParserError::InsufficientData {
            expected,
            actual: data.len(),
        }.into());
    }
    Ok(())
}
```

### Step 6.3: Example DEX Parser Implementation

**File:** `src/dex/raydium_cpmm.rs`

```rust
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::InnerInstruction;
use crate::dex::{DexParser, DexSwap};
use crate::error::Result;

pub const PROGRAM_ID: Pubkey = solana_sdk::pubkey!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");

pub struct RaydiumCpmm;

impl DexParser for RaydiumCpmm {
    fn parse(
        &self,
        instruction: &InnerInstruction,
        accounts: &[Pubkey],
        inner_instructions: &[InnerInstruction],
        token_accounts: &std::collections::HashMap<Pubkey, crate::token::TokenInfo>,
    ) -> Result<DexSwap> {
        // Resolve accounts with bounds checking
        let instruction_accounts = super::helpers::resolve_account_references(instruction, accounts)?;

        // Validate instruction data
        super::helpers::validate_instruction_data(&instruction.data, 16)?;

        // Extract discriminator (first 8 bytes)
        let discriminator = hex::encode(&instruction.data[0..8]);

        // Match discriminator and parse accordingly
        match discriminator.as_str() {
            "8fbe5adac41e33de" => self.parse_swap_base_input(&instruction_accounts, inner_instructions, token_accounts),
            _ => Err(crate::error::DexParserError::InvalidDiscriminator(discriminator).into()),
        }
    }
}

impl RaydiumCpmm {
    fn parse_swap_base_input(
        &self,
        accounts: &[Pubkey],
        inner_instructions: &[InnerInstruction],
        token_accounts: &std::collections::HashMap<Pubkey, crate::token::TokenInfo>,
    ) -> Result<DexSwap> {
        // Validate we have enough accounts
        if accounts.len() < 12 {
            return Err(crate::error::DexParserError::AccountIndexOutOfBounds {
                index: 11,
                accounts_len: accounts.len(),
            }.into());
        }

        // Implementation...
        todo!("Parse Raydium CPMM swap")
    }
}
```

### Step 6.4: Verification

- [ ] All array accesses have bounds checking
- [ ] No panics in DEX parsers
- [ ] Discriminator matching is safe
- [ ] Test with various DEX transactions

---

## Phase 7: gRPC Client Module

**Duration:** 2-3 days
**Goal:** Implement resilient multi-gRPC client with proper error handling.

### Step 7.1: Create gRPC Module Structure

```bash
mkdir -p src/grpc
touch src/grpc/mod.rs
touch src/grpc/client.rs
touch src/grpc/subscription.rs
touch src/grpc/retry.rs
```

### Step 7.2: Retry Logic

**File:** `src/grpc/retry.rs`

```rust
use std::time::Duration;
use tracing::{debug, warn};

const BASE_DELAY_MS: u64 = 1000;      // 1 second
const MAX_DELAY_MS: u64 = 60_000;     // 60 seconds

/// Resilient retry with exponential backoff
pub async fn resilient_retry<Operation, OnError, Fut, T, E>(
    mut operation: Operation,
    on_error: OnError,
) -> crate::Result<T>
where
    Operation: FnMut() -> Fut,
    OnError: Fn(&E),
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Debug,
{
    let mut attempt = 0u32;

    loop {
        match operation().await {
            Ok(result) => {
                if attempt > 0 {
                    debug!("Operation succeeded after {} attempts", attempt + 1);
                }
                return Ok(result);
            }
            Err(err) => {
                on_error(&err);

                let exponential_delay = BASE_DELAY_MS * 2_u64.pow(attempt.min(10));
                let delay = Duration::from_millis(exponential_delay.min(MAX_DELAY_MS));

                warn!(
                    attempt = attempt + 1,
                    delay_ms = delay.as_millis(),
                    "Operation failed, retrying"
                );

                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}
```

### Step 7.3: Subscription Management

**File:** `src/grpc/subscription.rs`

```rust
use std::sync::Arc;
use dashmap::DashMap;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::*;

pub struct SubscriptionFilters {
    pub transactions: Arc<DashMap<String, SubscribeRequestFilterTransactions>>,
    pub accounts: Arc<DashMap<String, SubscribeRequestFilterAccounts>>,
}

impl SubscriptionFilters {
    pub fn new() -> Self {
        Self {
            transactions: Arc::new(DashMap::new()),
            accounts: Arc::new(DashMap::new()),
        }
    }

    pub fn add_transaction_filter(&self, pubkey: Pubkey) {
        let key = format!("transaction_{}", pubkey);
        self.transactions.insert(
            key,
            SubscribeRequestFilterTransactions {
                account_required: vec![pubkey.to_string()],
                ..Default::default()
            },
        );
    }

    pub fn add_account_filter(&self, pubkey: Pubkey) {
        let key = format!("account_{}", pubkey);
        self.accounts.insert(
            key,
            SubscribeRequestFilterAccounts {
                account: vec![pubkey.to_string()],
                ..Default::default()
            },
        );
    }

    pub fn build_subscribe_request(&self) -> SubscribeRequest {
        let transactions_map: std::collections::HashMap<_, _> = self.transactions
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();

        let accounts_map: std::collections::HashMap<_, _> = self.accounts
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();

        SubscribeRequest {
            transactions: transactions_map,
            accounts: accounts_map,
            commitment: Some(CommitmentLevel::Confirmed.into()),
            ..Default::default()
        }
    }
}
```

### Step 7.4: Main gRPC Client

**File:** `src/grpc/client.rs`

```rust
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tonic::transport::Channel;
use yellowstone_grpc_proto::prelude::*;
use tracing::{info, warn, error};

use crate::config::GrpcConfig;
use crate::error::{GrpcConnectionError, Result};
use super::subscription::SubscriptionFilters;
use super::retry::resilient_retry;

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Connected,
    Connecting,
    Disconnected,
}

pub struct GrpcConnectionStatus {
    pub name: String,
    pub status: ConnectionStatus,
    pub ping: u64,  // milliseconds
}

pub struct Grpc {
    config: GrpcConfig,
    filters: SubscriptionFilters,
    connection_status: Arc<Mutex<GrpcConnectionStatus>>,
}

impl Grpc {
    pub fn new(config: &GrpcConfig) -> Self {
        Self {
            config: config.clone(),
            filters: SubscriptionFilters::new(),
            connection_status: Arc::new(Mutex::new(GrpcConnectionStatus {
                name: config.name.clone(),
                status: ConnectionStatus::Disconnected,
                ping: 0,
            })),
        }
    }

    /// Connect and stream updates with resilient retry
    pub async fn connect(&self, subscribe_tx: &mpsc::UnboundedSender<SubscribeUpdate>) -> Result<()> {
        resilient_retry(
            || self.connect_once(subscribe_tx),
            |err| {
                warn!(
                    endpoint = %self.config.endpoint,
                    error = ?err,
                    "Connection failed, will retry"
                );
            },
        ).await
    }

    async fn connect_once(&self, subscribe_tx: &mpsc::UnboundedSender<SubscribeUpdate>) -> Result<()> {
        // Update status to connecting
        {
            let mut status = self.connection_status.lock().unwrap();
            status.status = ConnectionStatus::Connecting;
        }

        info!(endpoint = %self.config.endpoint, "Connecting to gRPC");

        // Create channel
        let mut endpoint = Channel::from_shared(self.config.endpoint.clone())
            .map_err(|e| GrpcConnectionError::ConnectionFailed(
                tonic::Status::invalid_argument(e.to_string())
            ))?;

        // Add token if present
        if let Some(token) = &self.config.x_token {
            endpoint = endpoint.metadata(tonic::metadata::MetadataMap::new());
        }

        let channel = endpoint.connect().await
            .map_err(|e| GrpcConnectionError::ConnectionFailed(
                tonic::Status::unavailable(e.to_string())
            ))?;

        let mut client = GeyserClient::new(channel);

        // Update status to connected
        {
            let mut status = self.connection_status.lock().unwrap();
            status.status = ConnectionStatus::Connected;
        }

        info!(endpoint = %self.config.endpoint, "Connected to gRPC");

        // Subscribe
        let request = self.filters.build_subscribe_request();
        let (mut sink, mut stream) = client.subscribe().await
            .map_err(|e| GrpcConnectionError::ConnectionFailed(e))?
            .into_inner()
            .split();

        // Send initial subscription
        sink.send(request).await
            .map_err(|e| GrpcConnectionError::SubscriptionFailed(e.to_string()))?;

        // Stream updates
        while let Some(message) = stream.next().await {
            match message {
                Ok(update) => {
                    if let Some(update_oneof) = update.update_oneof {
                        if subscribe_tx.send(SubscribeUpdate {
                            update_oneof: Some(update_oneof),
                            ..Default::default()
                        }).is_err() {
                            error!("Failed to send update, channel closed");
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!(error = ?e, "Stream error");
                    return Err(GrpcConnectionError::ConnectionFailed(e).into());
                }
            }
        }

        // Update status to disconnected
        {
            let mut status = self.connection_status.lock().unwrap();
            status.status = ConnectionStatus::Disconnected;
        }

        Err(GrpcConnectionError::ConnectionFailed(
            tonic::Status::unavailable("Stream ended")
        ).into())
    }

    pub async fn subscribe_accounts(&self, accounts: Vec<Pubkey>) -> Result<()> {
        for account in accounts {
            self.filters.add_account_filter(account);
        }
        Ok(())
    }

    pub async fn subscribe_transactions(&self, pubkeys: Vec<Pubkey>) -> Result<()> {
        for pubkey in pubkeys {
            self.filters.add_transaction_filter(pubkey);
        }
        Ok(())
    }

    pub fn get_status(&self) -> GrpcConnectionStatus {
        self.connection_status.lock().unwrap().clone()
    }
}
```

### Step 7.5: Verification

- [ ] Reconnection works with exponential backoff
- [ ] Connection status properly tracked
- [ ] Subscriptions can be dynamically updated
- [ ] No panics on connection failures

---

## Phase 8: Subscription Manager

**Duration:** 1-2 days
**Goal:** Centralize subscription logic with throttling and queueing.

### Step 8.1: Create Subscription Manager

**File:** `src/subscription/mod.rs`

```rust
use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};
use solana_sdk::pubkey::Pubkey;
use tracing::{debug, warn};

use crate::grpc::Grpc;
use crate::Result;

pub struct SubscriptionManager {
    grpc_connections: Vec<Arc<Grpc>>,
    semaphore: Arc<Semaphore>,
    queue_tx: mpsc::UnboundedSender<Vec<Pubkey>>,
}

impl SubscriptionManager {
    pub fn new(grpc_connections: Vec<Arc<Grpc>>, max_concurrent: usize) -> Self {
        let (queue_tx, mut queue_rx) = mpsc::unbounded_channel::<Vec<Pubkey>>();
        let semaphore = Arc::new(Semaphore::new(max_concurrent));

        // Spawn queue processor
        let grpc_conns_clone = grpc_connections.clone();
        let semaphore_clone = Arc::clone(&semaphore);

        tokio::spawn(async move {
            while let Some(accounts) = queue_rx.recv().await {
                let permit = semaphore_clone.acquire_owned().await
                    .expect("Semaphore closed");

                let grpc_conns = grpc_conns_clone.clone();
                tokio::spawn(async move {
                    let _permit = permit;  // Hold until task completes

                    for grpc in &grpc_conns {
                        if let Err(e) = grpc.subscribe_accounts(accounts.clone()).await {
                            warn!("Subscription failed: {:?}", e);
                        }
                    }
                });
            }
        });

        Self {
            grpc_connections,
            semaphore,
            queue_tx,
        }
    }

    /// Subscribe to accounts with throttling
    pub async fn subscribe_to_accounts(&self, accounts: Vec<Pubkey>) {
        match self.semaphore.clone().try_acquire_owned() {
            Ok(permit) => {
                // Process immediately
                let grpc_conns = self.grpc_connections.clone();
                tokio::spawn(async move {
                    let _permit = permit;

                    for grpc in &grpc_conns {
                        if let Err(e) = grpc.subscribe_accounts(accounts.clone()).await {
                            warn!("Subscription failed: {:?}", e);
                        }
                    }

                    debug!(count = accounts.len(), "Subscribed to accounts");
                });
            }
            Err(_) => {
                // Queue for later
                debug!(count = accounts.len(), "Throttle limit reached, queueing");
                let _ = self.queue_tx.send(accounts);
            }
        }
    }

    /// Subscribe to authority account
    pub async fn subscribe_to_authority(&self, authority: Pubkey) {
        self.subscribe_to_accounts(vec![authority]).await;
    }

    /// Subscribe to mint token accounts
    pub async fn subscribe_to_mint_accounts(&self, _mint: Pubkey, accounts: Vec<Pubkey>) {
        self.subscribe_to_accounts(accounts).await;
    }
}
```

### Step 8.2: Add to lib.rs

```rust
pub mod subscription;
```

---

## Phase 9: Cleanup Manager

**Duration:** 1 day
**Goal:** Implement comprehensive TTL cleanup for all data structures.

### Step 9.1: Create Cleanup Module

**File:** `src/cleanup/mod.rs`

```rust
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::debug;

use crate::ScannerContext;

pub struct CleanupManager {
    context: Arc<ScannerContext>,
    ttl_seconds: i64,
    interval_seconds: u64,
}

impl CleanupManager {
    pub fn new(context: Arc<ScannerContext>, ttl_seconds: i64) -> Self {
        Self {
            context,
            ttl_seconds,
            interval_seconds: 60,
        }
    }

    pub async fn run(self) {
        let mut interval = time::interval(Duration::from_secs(self.interval_seconds));

        loop {
            interval.tick().await;

            let now = chrono::Utc::now().timestamp();

            // 1. Clean transactions
            self.context.transactions.retain(|_, tx| {
                now - tx.timestamp < self.ttl_seconds
            });

            // 2. Clean pending transactions that are no longer in main transactions
            self.context.pending_transactions.retain(|sig, _| {
                self.context.transactions.contains_key(sig)
            });

            // 3. Clean mints with no pools or old access time
            self.context.mints.retain(|_, mint_ctx| {
                !mint_ctx.pools.is_empty() ||
                (now - mint_ctx.last_accessed < self.ttl_seconds * 2)
            });

            // 4. Enforce resource limits
            self.context.enforce_limits();

            debug!(
                transactions = self.context.transactions.len(),
                pending = self.context.pending_transactions.len(),
                mints = self.context.mints.len(),
                pools = self.context.pool_registry.len(),
                "Cleanup completed"
            );
        }
    }
}
```

---

## Phase 10: Main Application Loop

**Duration:** 2-3 days
**Goal:** Orchestrate all components in main.rs.

### Step 10.1: Main Structure

**File:** `src/main.rs`

```rust
use std::sync::Arc;
use tokio::sync::mpsc;
use clap::Parser;
use tracing::{info, debug, warn};

mod error;
mod config;
mod cli;
mod parsing;
mod dex;
mod grpc;
mod subscription;
mod cleanup;
mod dashboard;

use scanner::{ScannerContext, TOKEN_PROGRAM_ID};

#[tokio::main]
async fn main() -> scanner::Result<()> {
    // Initialize logging
    init_logging();

    // Parse CLI arguments
    let args = cli::Args::parse();

    // Load and validate configuration
    let config = config::Config::load(&args.config)?;
    config.validate()?;

    info!("Configuration loaded successfully");

    // Parse arb programs
    let arb_programs: Vec<_> = config.arb_programs
        .iter()
        .filter_map(|p| p.parse().ok())
        .collect();

    // Create scanner context
    let scanner_context = Arc::new(ScannerContext::new(arb_programs.clone()));

    // Create gRPC connections
    let grpc_connections: Vec<Arc<grpc::Grpc>> = config.grpc
        .iter()
        .map(|cfg| Arc::new(grpc::Grpc::new(cfg)))
        .collect();

    // Create unbounded channel for updates
    let (subscribe_tx, mut subscribe_rx) = mpsc::unbounded_channel();

    // Create subscription manager
    let subscription_manager = Arc::new(subscription::SubscriptionManager::new(
        grpc_connections.clone(),
        50,  // Max 50 concurrent subscriptions
    ));

    // Spawn connection tasks
    for grpc in &grpc_connections {
        let grpc_clone = Arc::clone(grpc);
        let tx_clone = subscribe_tx.clone();

        tokio::spawn(async move {
            if let Err(e) = grpc_clone.connect(&tx_clone).await {
                warn!("Connection task ended: {:?}", e);
            }
        });
    }

    info!("All gRPC connections initiated");

    // Spawn receiver task
    let receiver_task = tokio::spawn({
        let scanner_ctx = Arc::clone(&scanner_context);
        let sub_mgr = Arc::clone(&subscription_manager);

        async move {
            receiver_loop(subscribe_rx, scanner_ctx, sub_mgr).await
        }
    });

    // Spawn cleanup task
    let cleanup_task = tokio::spawn({
        let scanner_ctx = Arc::clone(&scanner_context);

        async move {
            let cleanup_mgr = cleanup::CleanupManager::new(scanner_ctx, 300);
            cleanup_mgr.run().await;
        }
    });

    // Spawn subscription initialization task
    tokio::spawn({
        let sub_mgr = Arc::clone(&subscription_manager);
        let arb_progs = arb_programs.clone();

        async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            info!("Initializing subscriptions for arb programs");
            sub_mgr.subscribe_to_accounts(arb_progs).await;
        }
    });

    // Start dashboard (blocking)
    dashboard::Dashboard::new(scanner_context, config);

    // Wait for tasks
    let _ = tokio::join!(receiver_task, cleanup_task);

    Ok(())
}

async fn receiver_loop(
    mut subscribe_rx: mpsc::UnboundedReceiver<yellowstone_grpc_proto::prelude::SubscribeUpdate>,
    scanner_context: Arc<ScannerContext>,
    subscription_manager: Arc<subscription::SubscriptionManager>,
) {
    use yellowstone_grpc_proto::prelude::update_oneof::UpdateOneof;

    while let Some(update) = subscribe_rx.recv().await {
        if let Some(update_oneof) = update.update_oneof {
            match update_oneof {
                UpdateOneof::Account(account) => {
                    // Parse account update
                    match parsing::account::parse_token_account(&account) {
                        Ok(parsed) => {
                            debug!(
                                mint = %parsed.mint,
                                authority = %parsed.authority,
                                liquidity = parsed.liquidity,
                                "Token account update"
                            );

                            // Update liquidity
                            scanner_context.upsert_pool_liquidity(
                                parsed.mint,
                                parsed.authority,
                                parsed.liquidity,
                            );

                            // Subscribe to authority if new
                            // ...
                        }
                        Err(e) => {
                            warn!("Failed to parse account: {:?}", e);
                        }
                    }
                }

                UpdateOneof::Transaction(tx_update) => {
                    // Parse transaction
                    match parsing::transaction::parse_transaction(tx_update, |mint, decimals, accounts| {
                        // Handle intermediate mint discovery
                        debug!(mint = %mint, decimals, "Discovered intermediate mint");
                    }) {
                        Ok(parsed_tx) => {
                            debug!(signature = %parsed_tx.signature, "Parsed transaction");
                            scanner_context.insert_transaction(parsed_tx);
                        }
                        Err(e) => {
                            warn!("Failed to parse transaction: {:?}", e);
                        }
                    }
                }

                UpdateOneof::Ping(_) => {
                    debug!("Received ping");
                }

                UpdateOneof::Pong(pong) => {
                    debug!(id = pong.id, "Received pong");
                }

                _ => {}
            }
        }
    }
}

fn init_logging() {
    // Implementation from Phase 4
}
```

---

## Phase 11: Dashboard UI

**Duration:** 3-4 days
**Goal:** Implement TUI dashboard with aggressive modularization.

### Step 11.1: Create UI Module Structure

```bash
mkdir -p src/ui/components
touch src/ui/mod.rs
touch src/ui/dashboard.rs
touch src/ui/metrics.rs
touch src/ui/formatting.rs
touch src/ui/components/mod.rs
touch src/ui/components/transactions.rs
touch src/ui/components/mints.rs
touch src/ui/components/connections.rs
touch src/ui/components/programs.rs
```

### Step 11.2: Metrics Calculation

**File:** `src/ui/metrics.rs`

```rust
use std::collections::HashMap;
use solana_sdk::pubkey::Pubkey;
use crate::{ParsedTransaction, ScannerContext};

pub struct MintMetrics {
    pub rank: usize,
    pub mint: Pubkey,
    pub arbs_count: usize,
    pub fails_count: usize,
    pub profit: i64,
    pub net_volume: i64,
    pub total_volume: u64,
    pub fees: u64,
    pub liquidity: u64,
}

/// Calculate PnL for a transaction
/// Formula: last_swap.token_out.amount - first_swap.token_in.amount
pub fn calculate_pnl(tx: &ParsedTransaction) -> i64 {
    // Implementation from dashboard.rs:757
    0  // Placeholder
}

/// Calculate total fees for a transaction
pub fn calculate_total_fees(tx: &ParsedTransaction) -> u64 {
    // Implementation from dashboard.rs:782
    0  // Placeholder
}

/// Aggregate metrics for all mints
pub fn aggregate_mint_metrics(
    context: &ScannerContext,
) -> Vec<MintMetrics> {
    // Implementation from dashboard.rs:443-506
    vec![]  // Placeholder
}
```

### Step 11.3: Dashboard Main

**File:** `src/ui/dashboard.rs`

```rust
use std::io;
use std::sync::Arc;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{ScannerContext, config::Config};

pub struct Dashboard {
    scanner_context: Arc<ScannerContext>,
    config: Config,
}

impl Dashboard {
    pub fn new(scanner_context: Arc<ScannerContext>, config: Config) -> Self {
        let dashboard = Self {
            scanner_context,
            config,
        };

        if let Err(e) = dashboard.run() {
            eprintln!("Dashboard error: {}", e);
        }

        dashboard
    }

    fn run(&self) -> io::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Main loop
        loop {
            terminal.draw(|f| self.render(f))?;

            // Handle input
            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.code == KeyCode::Char('q') {
                        break;
                    }
                }
            }
        }

        // Cleanup
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(())
    }

    fn render(&self, f: &mut ratatui::Frame) {
        // Render all components
        // Use components from src/ui/components/
    }
}
```

---

## Phase 12: Documentation

**Duration:** 1-2 days
**Goal:** Create comprehensive documentation.

### Step 12.1: Create Documentation Structure

```bash
mkdir -p doc/metrics
touch doc/README.md
touch doc/architecture.md
touch doc/metrics/pnl_calculation.md
touch doc/metrics/volume_calculation.md
touch doc/metrics/liquidity_calculation.md
```

### Step 12.2: Copy Documentation

Copy the metric documentation from the plan file:
- PnL calculation formula
- Volume calculation formulas
- Liquidity calculation formula

---

## Appendix A: Required Dependencies

**Cargo.toml:**

```toml
[package]
name = "scanner"
version = "0.1.0"
edition = "2021"

[dependencies]
# Async runtime
tokio = { version = "1", features = ["full"] }
tokio-stream = "0.1"

# Error handling
anyhow = "1"
thiserror = "1"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# CLI
clap = { version = "4", features = ["derive"] }

# Config
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# gRPC
tonic = "0.10"
prost = "0.12"
yellowstone-grpc-proto = "1.11"

# Solana
solana-sdk = "1.17"
bs58 = "0.5"

# Concurrency
dashmap = "5"

# TUI
ratatui = "0.25"
crossterm = "0.27"

# Utils
chrono = "0.4"
hex = "0.4"
```

---

## Appendix B: Testing Strategy

Even though unit tests are not required, manual testing should cover:

1. **Error handling:**
   - Feed malformed protobuf data
   - Simulate connection failures
   - Test with invalid configuration

2. **Resource limits:**
   - Monitor memory usage over 1 hour
   - Verify LRU eviction works
   - Check cleanup runs every 60s

3. **Performance:**
   - Log file should be < 50KB/hour
   - No performance regression vs baseline
   - Channel depth stays < 1000

---

## Appendix C: Implementation Checklist

### Phase 1: Error Handling
- [ ] error.rs created with all error types
- [ ] thiserror dependency added
- [ ] All error types compile

### Phase 2: Core Types
- [ ] lib.rs with ScannerContext
- [ ] ResourceLimits with defaults
- [ ] No unwrap() in context methods
- [ ] LRU eviction implemented

### Phase 3: Config & CLI
- [ ] config.rs loads TOML/JSON
- [ ] Validation implemented
- [ ] cli.rs with clap

### Phase 4: Logging
- [ ] tracing-subscriber configured
- [ ] Log levels documented
- [ ] File and stderr output

### Phase 5: Parsing
- [ ] account.rs with safe parsing
- [ ] transaction.rs with error handling
- [ ] No panics in parsing code

### Phase 6: DEX Parsing
- [ ] dex/mod.rs with registry
- [ ] helpers.rs with bounds checking
- [ ] All DEX parsers safe

### Phase 7: gRPC Client
- [ ] retry.rs with exponential backoff
- [ ] subscription.rs with filters
- [ ] client.rs with resilient connection

### Phase 8: Subscription Manager
- [ ] Throttling with semaphore
- [ ] Queueing for overflow
- [ ] Centralized subscription logic

### Phase 9: Cleanup
- [ ] TTL cleanup for all structures
- [ ] Runs every 60 seconds
- [ ] Resource limit enforcement

### Phase 10: Main Loop
- [ ] All tasks spawned correctly
- [ ] Receiver loop processes updates
- [ ] Error handling throughout

### Phase 11: Dashboard
- [ ] UI module structure
- [ ] Metrics calculations
- [ ] Components split properly

### Phase 12: Documentation
- [ ] doc/ directory created
- [ ] All metric formulas documented
- [ ] Architecture documented

---

## Final Notes

- **Follow phases sequentially** - each builds on previous
- **Compile frequently** - catch errors early with `cargo check`
- **Test error paths** - don't just test happy paths
- **Use logging** - debug issues with proper log levels
- **Monitor resources** - watch memory and log file size

This is a ground-up refactoring. Take your time with each phase and verify before moving to the next.
