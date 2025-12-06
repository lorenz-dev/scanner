use solana_sdk::{inner_instruction, pubkey};
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::InnerInstruction;

use crate::{TokenAccounts, dex::{DexConfig, DexParser, DexParserError, SwapInstruction, DiscriminatorConfig}, token::Token};

pub struct RaydiumCLMMParser {
    config: DexConfig,
}

impl DexParser for RaydiumCLMMParser {
    fn config(&self) -> &DexConfig {
        &self.config
    }
}

impl RaydiumCLMMParser {
    pub fn new() -> Self {
        let config = DexConfig {
            name: "Raydium Concentrated Liquidity",
            program_id: pubkey!("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK"),
            discriminator_length: 16,
            discriminators: vec![
                DiscriminatorConfig {
                    name: "swap_v2",
                    discriminator: "2b04ed0b1ac91e62",
                    parse_fn: Self::parse_swap_v2,
                }
            ]
        };

        Self { config }
    }

    fn parse_swap_v2(
        instruction: &InnerInstruction,
        inner_instructions: &Vec<&InnerInstruction>,
        accounts: &Vec<Pubkey>,
        token_accounts: &TokenAccounts,
    ) -> Result<SwapInstruction, DexParserError> {
        let instruction_accounts = Self::get_instruction_accounts(instruction, accounts);

        // Swap program ID from the instruction
        let swap_program_id = accounts[instruction.program_id_index as usize];

        // Raydium CLMM pool is at account index 2
        let _pool = instruction_accounts[0];

        let pool_input= instruction_accounts[5];
        let pool_output = instruction_accounts[6];

        let input_token = instruction_accounts[11];
        let output_token = instruction_accounts[12];

        // let ins_data = Self::parse_instruction_data(instruction)?;

        // let (input_token, output_token) = if ins_data.is_base_input {
        //     (input_vault_mint, output_vault_mint)
        // } else {
        //     (output_vault_mint, input_vault_mint)
        // };

        let inner_instruction_0 = inner_instructions.get(0)
            .ok_or(DexParserError::InsufficientInnerInstructions)?;
        let transfer_in = Token::token_transfer(inner_instruction_0, accounts, token_accounts)?;

        let inner_instruction_1 = inner_instructions.get(1)
            .ok_or(DexParserError::InsufficientInnerInstructions)?;
        let transfer_out = Token::token_transfer(inner_instruction_1, accounts, token_accounts)?;

        Ok(SwapInstruction {
            swap_program_id,
            pools: vec![pool_input, pool_output],
            amount_in: transfer_in.amount,
            amount_out: transfer_out.amount,
            token_in: input_token.to_string(),
            token_out: output_token.to_string(),
            fees: Vec::new(),
        })
    }

    fn parse_instruction_data(instruction: &InnerInstruction) -> Result<SwapInstructionData, DexParserError> {
        let data = &instruction.data;

        // Skip discriminator (first 8 bytes)
        let mut offset = 8;

        // Parse amount (u64, 8 bytes)
        let amount_bytes: [u8; 8] = data[offset..offset + 8]
            .try_into()
            .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse amount".to_string()))?;
        let amount = u64::from_le_bytes(amount_bytes);
        offset += 8;

        // Parse other_amount_threshold (u64, 8 bytes)
        let threshold_bytes: [u8; 8] = data[offset..offset + 8]
            .try_into()
            .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse threshold".to_string()))?;
        let other_amount_threshold = u64::from_le_bytes(threshold_bytes);
        offset += 8;

        // Parse sqrt_price_limit_x64 (u128, 16 bytes)
        let sqrt_bytes: [u8; 16] = data[offset..offset + 16]
            .try_into()
            .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse sqrt price limit".to_string()))?;
        let sqrt_price_limit_x64 = u128::from_le_bytes(sqrt_bytes);
        offset += 16;

        // Parse is_base_input (bool, 1 byte)
        let is_base_input = data[offset] != 0;

        Ok(SwapInstructionData {
            amount,
            other_amount_threshold,
            sqrt_price_limit_x64,
            is_base_input,
        })
    }
}

#[derive(Debug)]
struct SwapInstructionData {
    amount: u64,
    other_amount_threshold: u64,
    sqrt_price_limit_x64: u128,
    is_base_input: bool,
}
