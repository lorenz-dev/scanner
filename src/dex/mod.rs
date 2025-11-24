pub mod goosefx;
pub mod meteora_damm;
pub mod meteora_dlmm;
pub mod meteora_pools;
pub mod pump_fun;
pub mod raydium_v4;
pub mod raydium_clmm;
pub mod raydium_cpmm;
pub mod whirlpools;

use std::collections::HashMap;
use std::sync::OnceLock;

use yellowstone_grpc_proto::prelude::{
    InnerInstruction
};
use solana_sdk::pubkey::Pubkey;

use crate::{ArbTransactionInstruction, TokenAccount, TokenAccounts, token::{TokenAmount, TokenWithAmount}};

#[derive(Default, Debug)]
pub struct DexSwap {
    pub token_in: TokenAmount,
    pub token_out: TokenAmount,
    pub fees: Vec<TokenAmount>,
}

pub struct DexConfig {
    pub name: &'static str,
    pub program_id: Pubkey,
    pub discriminator_length: usize,
    pub discriminators: Vec<DiscriminatorConfig>
}

pub struct DiscriminatorConfig {
    pub name: &'static str,
    pub discriminator: &'static str,
    pub parse_fn: ParserFn,
}

pub type ParserFn = fn(
    &InnerInstruction,
    &Vec<&InnerInstruction>,
    &Vec<Pubkey>,
    &TokenAccounts,
) -> Result<DexSwap, DexParserError>;

#[derive(Debug)]
pub enum DexParserError {
    ParserNotFound(String),
    DiscriminatorNotFound(String, String),
    TokenError(crate::token::TokenError),
    TokenWithAmountError(crate::token::TokenWithAmountError),
    InvalidInstructionData(String),
}

impl From<crate::token::TokenError> for DexParserError {
    fn from(error: crate::token::TokenError) -> Self {
        DexParserError::TokenError(error)
    }
}

impl From<crate::token::TokenWithAmountError> for DexParserError {
    fn from(error: crate::token::TokenWithAmountError) -> Self {
        DexParserError::TokenWithAmountError(error)
    }
}

pub trait DexParser: Send + Sync {
    fn config(&self) -> &DexConfig;

    fn parse(
        &self,
        instruction: &InnerInstruction,
        inner_instructions: Vec<&InnerInstruction>,
        accounts: &Vec<Pubkey>,
        token_accounts: &TokenAccounts,
    ) -> Result<DexSwap, DexParserError> {
        let config = self.config();
        let discriminator_len = config.discriminator_length;

        let data = instruction.data.clone();
        let hex_data = hex::encode(data);
        let discriminator = &hex_data[..discriminator_len];

        let program_id = accounts.get(instruction.program_id_index as usize).unwrap();

        // Find matching discriminator
        for disc_config in &config.discriminators {
            if disc_config.discriminator == discriminator {
                let swap = (disc_config.parse_fn)(
                    instruction,
                    &inner_instructions,
                    accounts,
                    token_accounts,
                )?;
                return Ok(swap);
            }
        }

        Err(DexParserError::DiscriminatorNotFound(
            program_id.to_string(),
            discriminator.to_string(),
        ))
    }

    fn get_instruction_accounts(instruction: &InnerInstruction, accounts: &Vec<Pubkey>) -> Vec<Pubkey> where Self: Sized {
        instruction.accounts
            .iter()
            .map(|&idx| accounts[idx as usize])
            .collect()
    }
}

pub struct DexRegistry {
    parsers: HashMap<Pubkey, Box<dyn DexParser>>,
}

static DEX_REGISTRY: OnceLock<DexRegistry> = OnceLock::new();

impl DexRegistry {
    /// Get the global singleton instance of DexRegistry
    pub fn global() -> &'static DexRegistry {
        DEX_REGISTRY.get_or_init(|| DexRegistry::new())
    }

    fn new() -> Self {
        let mut registry = Self {
            parsers: HashMap::new(),
        };

        // registry.register(Box::new(goosefx::GooseFXParser::new()));
        registry.register(Box::new(meteora_damm::MeteoraDAMMParser::new()));
        registry.register(Box::new(meteora_dlmm::MeteoraDelmmParser::new()));
        // registry.register(Box::new(meteora_pools::MeteoraPoolsParser::new()));
        registry.register(Box::new(pump_fun::PumpFunParser::new()));
        registry.register(Box::new(raydium_clmm::RaydiumCLMMParser::new()));
        registry.register(Box::new(raydium_cpmm::RaydiumCPMMParser::new()));
        registry.register(Box::new(raydium_v4::RaydiumV4Parser::new()));
        registry.register(Box::new(whirlpools::WhirlpoolsParser::new()));

        registry
    }

    fn register(&mut self, parser: Box<dyn DexParser>) {
        self.parsers.insert(parser.config().program_id.clone(), parser);
    }

    /// Parse a DEX instruction by looking up the appropriate parser
    pub fn parse(&self, instruction: &InnerInstruction, inner_instructions: Vec<&InnerInstruction>, accounts: &Vec<Pubkey>, token_accounts: &TokenAccounts) -> Result<DexSwap, DexParserError> {
        let program_id = accounts.get(instruction.program_id_index as usize).unwrap();
        if let Some(parser) = self.parsers.get(&program_id) {
            parser.parse(instruction, inner_instructions, &accounts, token_accounts)
        } else {
            Err(DexParserError::ParserNotFound(program_id.to_string()))
        }
    }
}