use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::InnerInstruction;

use crate::{TokenAccount, TokenAccounts, dex::{DexConfig, DexParser, DexParserError, DexSwap, DiscriminatorConfig}};

pub struct MeteoraPoolsParser {
    config: DexConfig,
}

impl DexParser for MeteoraPoolsParser {
    fn config(&self) -> &DexConfig {
        &self.config
    }
}

impl MeteoraPoolsParser {
    pub fn new() -> Self {
        let config = DexConfig {
            name: "Meteora Pools",
            program_id: pubkey!("Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB"),
            discriminator_length: 16,
            discriminators: vec![
                DiscriminatorConfig {
                    name: "swap_variant1",
                    discriminator: "414b3f4ceb5b5b88",
                    parse_fn: Self::parse_swap_variant1,
                },
                DiscriminatorConfig {
                    name: "swap_variant2",
                    discriminator: "f8c69e91e17587c8",
                    parse_fn: Self::parse_swap_variant2,
                }
            ]
        };

        Self { config }
    }

    fn parse_swap_variant1(
        _instruction: &InnerInstruction,
        _inner_instructions: &Vec<&InnerInstruction>,
        _accounts: &Vec<Pubkey>,
        token_accounts: &TokenAccounts,
    ) -> Result<DexSwap, DexParserError> {
        // TODO: Implement Meteora Pools swap variant 1 parsing
        Ok(DexSwap::default())
    }

    fn parse_swap_variant2(
        _instruction: &InnerInstruction,
        _inner_instructions: &Vec<&InnerInstruction>,
        _accounts: &Vec<Pubkey>,
        token_accounts: &TokenAccounts,
    ) -> Result<DexSwap, DexParserError> {
        // TODO: Implement Meteora Pools swap variant 2 parsing
        Ok(DexSwap::default())
    }
}
