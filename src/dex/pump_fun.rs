use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::InnerInstruction;
use anyhow::Result;

use crate::{TokenAccount, TokenAccounts, dex::{DexConfig, DexParser, DexParserError, DexSwap, DiscriminatorConfig}, token::{self, Token, TokenAmount, TokenInfo, TokenWithAmount}};

#[derive(Debug)]
struct PumpFunCpiLog {
    timestamp: u64,
    base_amount_in: u64,
    min_quote_amount_out: u64,
    user_base_token_reserves: u64,
    user_quote_token_reserves: u64,
    pool_base_token_reserves: u64,
    pool_quote_token_reserves: u64,
    quote_amount_out: u64,
    lp_fee_basis_points: u64,
    lp_fee: u64,
    protocol_fee_basis_points: u64,
    protocol_fee: u64,
    quote_amount_out_without_lp_fee: u64,
    user_quote_amount_out: u64,
    pool: Pubkey,
    user: Pubkey,
    user_base_token_account: Pubkey,
    user_quote_token_account: Pubkey,
    protocol_fee_recipient: Pubkey,
    protocol_fee_recipient_token_account: Pubkey,
    coin_creator: Pubkey,
    coin_creator_fee_basis_points: u64,
    coin_creator_fee: u64,
}

pub struct PumpFunParser {
    config: DexConfig,
}

impl DexParser for PumpFunParser {
    fn config(&self) -> &DexConfig {
        &self.config
    }
}

impl PumpFunParser {
    pub fn new() -> Self {
        let config = DexConfig {
            name: "Pump.fun AMM",
            program_id: pubkey!("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"),
            discriminator_length: 16,
            discriminators: vec![
                DiscriminatorConfig {
                    name: "buy",
                    discriminator: "66063d1201daebea",
                    parse_fn: Self::parse_buy,
                },
                DiscriminatorConfig {
                    name: "sell",
                    discriminator: "33e685a4017f83ad",
                    parse_fn: Self::parse_sell,
                }
            ]
        };

        Self { config }
    }

    fn parse_buy(
        instruction: &InnerInstruction,
        inner_instructions: &Vec<&InnerInstruction>,
        accounts: &Vec<Pubkey>,
        token_accounts: &TokenAccounts,
    ) -> Result<DexSwap, DexParserError> {
        // TODO: Implement buy parsing
        Ok(DexSwap::default())
    }

    fn parse_sell(
        instruction: &InnerInstruction,
        inner_instructions: &Vec<&InnerInstruction>,
        accounts: &Vec<Pubkey>,
        token_accounts: &TokenAccounts,
    ) -> Result<DexSwap, DexParserError> {
        let mut swap = DexSwap::default();

        let instruction_acounts = Self::get_instruction_accounts(instruction, accounts);

        let base_mint: Pubkey = instruction_acounts[3];
        let quote_mint = instruction_acounts[4];

        // let transfer = Token::token_transfer(inner_instructions[1], accounts, token_accounts)?;
        // swap.token_out.add(transfer)?;

        // let transfer = Token::token_transfer(inner_instructions[2], accounts, token_accounts)?;
        // swap.token_in.add(transfer)?;

        // let mut idx = 0;
        // while idx < inner_instructions.len() {
        //     if inner_instructions[idx].data.len() >= 8 {
        //         let hex_str = hex::encode(&inner_instructions[idx].data[..8]);
        //         if hex_str == "e445a52e51cb9a1d" {
        //             cpi_log_instruction = Some(inner_instructions[idx]);
        //             break;
        //         }
        //     }
        //     idx += 1;
        // }

        // Parse CPI log and extract fees
        let cpi_log = Self::parse_cpi_log(inner_instructions[inner_instructions.len() - 1])?;

        swap.token_in = TokenAmount {
            amount: cpi_log.base_amount_in,
            mint: base_mint,
        };

        swap.token_out = TokenAmount {
            amount: cpi_log.user_quote_amount_out,
            mint: quote_mint,
        };

        // Add LP fee
        if cpi_log.lp_fee > 0 {
            swap.fees.push(TokenAmount {
                amount: cpi_log.lp_fee,
                mint: base_mint,
            });
        }

        // Add protocol fee
        if cpi_log.protocol_fee > 0 {
            swap.fees.push(TokenAmount {
                amount: cpi_log.protocol_fee,
                mint: quote_mint,
            });
        }

        // Add coin creator fee
        if cpi_log.coin_creator_fee > 0 {
            swap.fees.push(TokenAmount {
                amount: cpi_log.coin_creator_fee,
                mint: quote_mint,
            });
        }

        Ok(swap)
    }

    fn parse_cpi_log(instruction: &InnerInstruction) -> Result<PumpFunCpiLog, DexParserError> {
        let data = &instruction.data;

        // Minimum size check: 16 (discriminator) + 14*8 (u64 fields) + 7*32 (Pubkey fields) = 352 bytes
        if data.len() < 352 {
            return Err(DexParserError::InvalidInstructionData("CPI log data too short".to_string()));
        }

        let mut offset = 16;

        // Parse timestamp (8 bytes)
        let timestamp = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse timestamp".to_string()))?
        );
        offset += 8;

        // Parse baseAmountIn (8 bytes)
        let base_amount_in = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse base_amount_in".to_string()))?
        );
        offset += 8;

        // Parse minQuoteAmountOut (8 bytes)
        let min_quote_amount_out = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse min_quote_amount_out".to_string()))?
        );
        offset += 8;

        // Parse userBaseTokenReserves (8 bytes)
        let user_base_token_reserves = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse user_base_token_reserves".to_string()))?
        );
        offset += 8;

        // Parse userQuoteTokenReserves (8 bytes)
        let user_quote_token_reserves = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse user_quote_token_reserves".to_string()))?
        );
        offset += 8;

        // Parse poolBaseTokenReserves (8 bytes)
        let pool_base_token_reserves = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse pool_base_token_reserves".to_string()))?
        );
        offset += 8;

        // Parse poolQuoteTokenReserves (8 bytes)
        let pool_quote_token_reserves = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse pool_quote_token_reserves".to_string()))?
        );
        offset += 8;

        // Parse quoteAmountOut (8 bytes)
        let quote_amount_out = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse quote_amount_out".to_string()))?
        );
        offset += 8;

        // Parse lpFeeBasisPoints (8 bytes)
        let lp_fee_basis_points = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse lp_fee_basis_points".to_string()))?
        );
        offset += 8;

        // Parse lpFee (8 bytes)
        let lp_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse lp_fee".to_string()))?
        );
        offset += 8;

        // Parse protocolFeeBasisPoints (8 bytes)
        let protocol_fee_basis_points = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse protocol_fee_basis_points".to_string()))?
        );
        offset += 8;

        // Parse protocolFee (8 bytes)
        let protocol_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse protocol_fee".to_string()))?
        );
        offset += 8;

        // Parse quoteAmountOutWithoutLpFee (8 bytes)
        let quote_amount_out_without_lp_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse quote_amount_out_without_lp_fee".to_string()))?
        );
        offset += 8;

        // Parse userQuoteAmountOut (8 bytes)
        let user_quote_amount_out = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse user_quote_amount_out".to_string()))?
        );
        offset += 8;

        // Parse pool (32 bytes)
        let pool = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse pool".to_string()))?
        );
        offset += 32;

        // Parse user (32 bytes)
        let user = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse user".to_string()))?
        );
        offset += 32;

        // Parse userBaseTokenAccount (32 bytes)
        let user_base_token_account = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse user_base_token_account".to_string()))?
        );
        offset += 32;

        // Parse userQuoteTokenAccount (32 bytes)
        let user_quote_token_account = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse user_quote_token_account".to_string()))?
        );
        offset += 32;

        // Parse protocolFeeRecipient (32 bytes)
        let protocol_fee_recipient = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse protocol_fee_recipient".to_string()))?
        );
        offset += 32;

        // Parse protocolFeeRecipientTokenAccount (32 bytes)
        let protocol_fee_recipient_token_account = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse protocol_fee_recipient_token_account".to_string()))?
        );
        offset += 32;

        // Parse coinCreator (32 bytes)
        let coin_creator = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse coin_creator".to_string()))?
        );
        offset += 32;

        // Parse coinCreatorFeeBasisPoints (8 bytes)
        let coin_creator_fee_basis_points = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse coin_creator_fee_basis_points".to_string()))?
        );
        offset += 8;

        // Parse coinCreatorFee (8 bytes)
        let coin_creator_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse coin_creator_fee".to_string()))?
        );

        Ok(PumpFunCpiLog {
            timestamp,
            base_amount_in,
            min_quote_amount_out,
            user_base_token_reserves,
            user_quote_token_reserves,
            pool_base_token_reserves,
            pool_quote_token_reserves,
            quote_amount_out,
            lp_fee_basis_points,
            lp_fee,
            protocol_fee_basis_points,
            protocol_fee,
            quote_amount_out_without_lp_fee,
            user_quote_amount_out,
            pool,
            user,
            user_base_token_account,
            user_quote_token_account,
            protocol_fee_recipient,
            protocol_fee_recipient_token_account,
            coin_creator,
            coin_creator_fee_basis_points,
            coin_creator_fee,
        })
    }
}
