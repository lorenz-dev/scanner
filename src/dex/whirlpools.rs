use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::InnerInstruction;

use crate::{TokenAccount, TokenAccounts, dex::{DexConfig, DexParser, DexParserError, DexSwap, DiscriminatorConfig}, token::{Token, TokenAmount}};

pub struct WhirlpoolsParser {
    config: DexConfig,
}

impl DexParser for WhirlpoolsParser {
    fn config(&self) -> &DexConfig {
        &self.config
    }
}

impl WhirlpoolsParser {
    pub fn new() -> Self {
        let config = DexConfig {
            name: "Whirlpools",
            program_id: pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc"),
            discriminator_length: 16,
            discriminators: vec![
                DiscriminatorConfig {
                    name: "swap",
                    discriminator: "2b04ed0b1ac91e62",
                    parse_fn: Self::parse_swap,
                }
            ]
        };

        Self { config }
    }

    fn parse_swap(
        instruction: &InnerInstruction,
        inner_instructions: &Vec<&InnerInstruction>,
        accounts: &Vec<Pubkey>,
        token_accounts: &TokenAccounts,
    ) -> Result<DexSwap, DexParserError> {
        let mut swap = DexSwap::default();

        let instruction_accounts = Self::get_instruction_accounts(instruction, accounts);

        let token_a = instruction_accounts[5];
        let token_b = instruction_accounts[6];

        let ins_data = Self::parse_instruction_data(instruction)?;

        let (input_token, output_token) = if ins_data.a_to_b {
            (token_a, token_b)
        } else {
            (token_b, token_a)
        };

        let transfer_in = Token::token_transfer(inner_instructions[0], accounts, token_accounts)?;
        swap.token_in = TokenAmount {
            mint: input_token,
            amount: transfer_in.amount
        };

        let transfer_out = Token::token_transfer(inner_instructions[1], accounts, token_accounts)?;
        swap.token_out = TokenAmount {
            mint: output_token,
            amount: transfer_out.amount
        };

        Ok(swap)
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

        // Parse otherAmountThreshold (u64, 8 bytes)
        let threshold_bytes: [u8; 8] = data[offset..offset + 8]
            .try_into()
            .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse threshold".to_string()))?;
        let other_amount_threshold = u64::from_le_bytes(threshold_bytes);
        offset += 8;

        // Parse sqrtPriceLimit (u128, 16 bytes)
        let sqrt_bytes: [u8; 16] = data[offset..offset + 16]
            .try_into()
            .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse sqrt price limit".to_string()))?;
        let sqrt_price_limit = u128::from_le_bytes(sqrt_bytes);
        offset += 16;

        // Parse amountSpecifiedIsInput (bool, 1 byte)
        let amount_specified_is_input = data[offset] != 0;
        offset += 1;

        // Parse aToB (bool, 1 byte)
        let a_to_b = data[offset] != 0;
        offset += 1;

        // Parse remainingAccountsInfo (Option, 1 byte discriminator)
        let remaining_accounts_info = if offset < data.len() && data[offset] != 0 {
            // There's more data, but we're treating it as None for now
            None
        } else {
            None
        };

        Ok(SwapInstructionData {
            amount,
            other_amount_threshold,
            sqrt_price_limit,
            amount_specified_is_input,
            a_to_b,
            remaining_accounts_info,
        })
    }
}

#[derive(Debug)]
struct SwapInstructionData {
    amount: u64,
    other_amount_threshold: u64,
    sqrt_price_limit: u128,
    amount_specified_is_input: bool,
    a_to_b: bool,
    remaining_accounts_info: Option<()>,
}
