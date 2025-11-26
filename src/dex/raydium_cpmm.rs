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
        let instruction_accounts = Self::get_instruction_accounts(instruction, accounts);

        // Swap program ID from the instruction
        let swap_program_id = accounts[instruction.program_id_index as usize];

        // Raydium CPMM pool is at account index 1
        let pool = instruction_accounts[0];

        let pool_input = instruction_accounts[6];
        let pool_output = instruction_accounts[7];

        let input_token = instruction_accounts[10];
        let output_token = instruction_accounts[11];

        // Parse the first inner instruction as token input transfer
        let inner_instruction_0 = inner_instructions.get(0)
            .ok_or(DexParserError::InsufficientInnerInstructions)?;
        let transfer_in = Token::token_transfer(inner_instruction_0, accounts, token_accounts)?;

        // Parse the second inner instruction as token output transfer
        let inner_instruction_1 = inner_instructions.get(1)
            .ok_or(DexParserError::InsufficientInnerInstructions)?;
        let transfer_out = Token::token_transfer(inner_instruction_1, accounts, token_accounts)?;

        Ok(DexSwap {
            swap_program_id,
            pools: vec![pool_input, pool_output],
            pool_owner: pool,  // pool is the pool owner
            token_in: TokenAmount {
                mint: input_token,
                amount: transfer_in.amount,
            },
            token_out: TokenAmount {
                mint: output_token,
                amount: transfer_out.amount,
            },
            fees: Vec::new(),
            vault_accounts: Vec::new(),
        })
    }
}
