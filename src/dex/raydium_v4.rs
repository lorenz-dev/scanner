use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::InnerInstruction;

use crate::{TokenAccount, TokenAccounts, dex::{DexConfig, DexParser, DexParserError, DexSwap, DiscriminatorConfig}, token::{Token, TokenAmount}};

pub struct RaydiumV4Parser {
    config: DexConfig,
}

impl DexParser for RaydiumV4Parser {
    fn config(&self) -> &DexConfig {
        &self.config
    }
}

impl RaydiumV4Parser {
    pub fn new() -> Self {
        let config = DexConfig {
            name: "Raydium Liquidity Pool V4",
            program_id: pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"),
            discriminator_length: 2,
            discriminators: vec![
                DiscriminatorConfig {
                    name: "swapBaseIn",
                    discriminator: "09",
                    parse_fn: Self::parse_swap_base_in,
                }
            ]
        };

        Self { config }
    }

    fn parse_swap_base_in(
        instruction: &InnerInstruction,
        inner_instructions: &Vec<&InnerInstruction>,
        accounts: &Vec<Pubkey>,
        token_accounts: &TokenAccounts,
    ) -> Result<DexSwap, DexParserError> {
        let mut swap = DexSwap::default();

        // let instruction_accounts = Self::get_instruction_accounts(instruction, accounts);

        // let input_token = instruction_accounts[5];
        // let output_token = instruction_accounts[6];

        let transfer_in = Token::token_transfer(inner_instructions[0], accounts, token_accounts)?;
        swap.token_in = TokenAmount {
            mint: transfer_in.token_info.mint,
            amount: transfer_in.amount,
        };

        let transfer_out = Token::token_transfer(inner_instructions[1], accounts, token_accounts)?;
        swap.token_out = TokenAmount {
            mint: transfer_out.token_info.mint,
            amount: transfer_out.amount,
        };

        Ok(swap)
    }
}
