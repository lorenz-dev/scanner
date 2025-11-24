use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::InnerInstruction;

use crate::{TokenAccounts, dex::{DexConfig, DexParser, DexParserError, DexSwap, DiscriminatorConfig}, token::{Token, TokenAmount}};

pub struct RaydiumCPMMParser {
    config: DexConfig,
}

impl DexParser for RaydiumCPMMParser {
    fn config(&self) -> &DexConfig {
        &self.config
    }
}

impl RaydiumCPMMParser {
    pub fn new() -> Self {
        let config = DexConfig {
            name: "Raydium CPMM",
            program_id: pubkey!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C"),
            discriminator_length: 16,
            discriminators: vec![
                DiscriminatorConfig {
                    name: "swap_base_input",
                    discriminator: "8fbe5adac41e33de",
                    parse_fn: Self::parse_swap_base_input,
                }
            ]
        };

        Self { config }
    }

    fn parse_swap_base_input(
        instruction: &InnerInstruction,
        inner_instructions: &Vec<&InnerInstruction>,
        accounts: &Vec<Pubkey>,
        token_accounts: &TokenAccounts,
    ) -> Result<DexSwap, DexParserError> {
        let mut swap = DexSwap::default();

        let instruction_accounts = Self::get_instruction_accounts(instruction, accounts);

        let input_token = instruction_accounts[10];
        let output_token = instruction_accounts[11];

        // Parse the first inner instruction as token input transfer
        let transfer_in = Token::token_transfer(inner_instructions[0], accounts, token_accounts)?;
        swap.token_in = TokenAmount {
            mint: input_token,
            amount: transfer_in.amount,
        };

        // Parse the second inner instruction as token output transfer
        let transfer_out = Token::token_transfer(inner_instructions[1], accounts, token_accounts)?;
        swap.token_out = TokenAmount {
            mint: output_token,
            amount: transfer_out.amount,
        };

        Ok(swap)
    }
}
