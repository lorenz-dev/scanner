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
        let instruction_accounts = Self::get_instruction_accounts(instruction, accounts);

        // Swap program ID from the instruction
        let swap_program_id = accounts[instruction.program_id_index as usize];

        // Raydium V4 pool is at account index 1 (AMM ID)
        let pool = instruction_accounts[1];

        let pool_a = instruction_accounts[4];
        let pool_b = instruction_accounts[5];

        let inner_instruction_0 = inner_instructions.get(0)
            .ok_or(DexParserError::InsufficientInnerInstructions)?;
        let transfer_in = Token::token_transfer(inner_instruction_0, accounts, token_accounts)?;

        let inner_instruction_1 = inner_instructions.get(1)
            .ok_or(DexParserError::InsufficientInnerInstructions)?;
        let transfer_out = Token::token_transfer(inner_instruction_1, accounts, token_accounts)?;

        // Collect vault accounts from transfers
        let mut vault_accounts = Vec::new();
        if let Some(source) = transfer_in.source_account {
            vault_accounts.push(source);
        }
        if let Some(source) = transfer_out.source_account {
            vault_accounts.push(source);
        }

        Ok(DexSwap {
            swap_program_id,
            pools: vec![pool_a, pool_b],
            pool_owner: pool,  // pool is the pool owner
            token_in: TokenAmount {
                mint: transfer_in.token_info.mint,
                amount: transfer_in.amount,
            },
            token_out: TokenAmount {
                mint: transfer_out.token_info.mint,
                amount: transfer_out.amount,
            },
            fees: Vec::new(),
            vault_accounts,
        })
    }
}
