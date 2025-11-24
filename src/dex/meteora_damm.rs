use anyhow::Result;
use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::InnerInstruction;

use crate::{TokenAccount, TokenAccounts, dex::{DexConfig, DexParser, DexParserError, DexSwap, DiscriminatorConfig}, token::{Token, TokenAmount}};

#[derive(Debug)]
struct SwapParams {
    amount_in: u64,
    minimum_amount_out: u64,
}

#[derive(Debug)]
struct SwapResult {
    output_amount: u64,
    next_sqrt_price: u128,
    lp_fee: u64,
    protocol_fee: u64,
    partner_fee: u64,
    referral_fee: u64,
}

#[derive(Debug)]
struct SwapParams2 {
    amount0: u64,
    amount1: u64,
    swap_mode: u8,
}

#[derive(Debug)]
struct SwapResult2 {
    included_fee_input_amount: u64,
    excluded_fee_input_amount: u64,
    amount_left: u64,
    output_amount: u64,
    next_sqrt_price: u128,
    trading_fee: u64,
    protocol_fee: u64,
    partner_fee: u64,
    referral_fee: u64,
}

#[derive(Debug)]
struct CpiLog2 {
    pool: Pubkey,
    trade_direction: u8,
    collect_fee_mode: u8,
    has_referral: bool,
    params: SwapParams2,
    swap_result: SwapResult2,
    included_transfer_fee_amount_in: u64,
    included_transfer_fee_amount_out: u64,
    excluded_transfer_fee_amount_out: u64,
    current_timestamp: u64,
    reserve_a_amount: u64,
    reserve_b_amount: u64,
}

#[derive(Debug)]
struct CpiLog1 {
    pool: Pubkey,
    trade_direction: u8,
    has_referral: bool,
    params: SwapParams,
    swap_result: SwapResult,
    actual_amount_in: u64,
    current_timestamp: u64,
}

pub struct MeteoraDAMMParser {
    config: DexConfig,
}

impl DexParser for MeteoraDAMMParser {
    fn config(&self) -> &DexConfig {
        &self.config
    }
}

impl MeteoraDAMMParser {
    pub fn new() -> Self {
        let config = DexConfig {
            name: "Meteora DAMM v2",
            program_id: pubkey!("cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG"),
            discriminator_length: 16,
            discriminators: vec![
                DiscriminatorConfig {
                    name: "swap",
                    discriminator: "f8c69e91e17587c8",
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

        // let transfer_in = Token::token_transfer(inner_instructions[0], accounts, token_accounts)?;
        // swap.token_in = TokenAmount {
        //     mint: token_b_mint,
        //     amount: transfer_in.amount,
        // };

        // let transfer_out = Token::token_transfer(inner_instructions[1], accounts, token_accounts)?;
        // swap.token_out = TokenAmount {
        //     mint: token_a_mint,
        //     amount: transfer_out.amount,
        // };

        let instruction_acounts = Self::get_instruction_accounts(instruction, accounts);

        let token_a_mint = instruction_acounts[6];
        let token_b_mint = instruction_acounts[7];

        // let cpi_log = Self::parse_cpi_log_1(inner_instructions[2])?;
        let cpi_log = Self::parse_cpi_log_2(inner_instructions[3])?;

        let (token_in, token_out) = if cpi_log.trade_direction == 0 {
            (token_a_mint, token_b_mint)
        } else {
            (token_b_mint, token_a_mint)
        };

        swap.token_in = TokenAmount { mint: token_in, amount: cpi_log.included_transfer_fee_amount_in };
        swap.token_out = TokenAmount { mint: token_out, amount: cpi_log.excluded_transfer_fee_amount_out };

        // Fees from the CPI log
        if cpi_log.swap_result.trading_fee > 0 {
            swap.fees.push(TokenAmount { mint: token_in, amount: cpi_log.swap_result.trading_fee });
        }
        if cpi_log.swap_result.protocol_fee > 0 {
            swap.fees.push(TokenAmount { mint: token_in, amount: cpi_log.swap_result.protocol_fee });
        }
        if cpi_log.swap_result.partner_fee > 0{
            swap.fees.push(TokenAmount { mint: token_in, amount: cpi_log.swap_result.partner_fee });
        }
        if cpi_log.swap_result.referral_fee > 0{
            swap.fees.push(TokenAmount { mint: token_in, amount: cpi_log.swap_result.referral_fee });
        }
        
        Ok(swap)
    }

    fn _parse_cpi_log_1(instruction: &InnerInstruction) -> Result<CpiLog1, DexParserError> {
        let data = &instruction.data;

        if data.len() < 138 {
            return Err(DexParserError::InvalidInstructionData("CPI log data too short".to_string()));
        }

        let mut offset = 16;

        // Parse pool (32 bytes)
        let pool = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse pool".to_string()))?
        );
        offset += 32;

        // Parse trade direction (1 byte)
        let trade_direction = data[offset];
        offset += 1;

        // Parse has referral (1 byte)
        let has_referral = data[offset] != 0;
        offset += 1;

        // Parse amount in (8 bytes, u64 little endian)
        let amount_in = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse amount_in".to_string()))?
        );
        offset += 8;

        // Parse minimum amount out (8 bytes, u64 little endian)
        let minimum_amount_out = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse minimum_amount_out".to_string()))?
        );
        offset += 8;

        // Parse output amount (8 bytes, u64 little endian)
        let output_amount = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse output_amount".to_string()))?
        );
        offset += 8;

        // Parse next sqrt price (16 bytes, u128 little endian)
        let next_sqrt_price = u128::from_le_bytes(
            data[offset..offset + 16]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse next_sqrt_price".to_string()))?
        );
        offset += 16;

        // Parse lp fee (8 bytes, u64 little endian)
        let lp_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse lp_fee".to_string()))?
        );
        offset += 8;

        // Parse protocol fee (8 bytes, u64 little endian)
        let protocol_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse protocol_fee".to_string()))?
        );
        offset += 8;

        // Parse partner fee (8 bytes, u64 little endian)
        let partner_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse partner_fee".to_string()))?
        );
        offset += 8;

        // Parse referral fee (8 bytes, u64 little endian)
        let referral_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse referral_fee".to_string()))?
        );
        offset += 8;

        // Parse actual amount in (8 bytes, u64 little endian)
        let actual_amount_in = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse actual_amount_in".to_string()))?
        );
        offset += 8;

        // Parse current timestamp (8 bytes, u64 little endian)
        let current_timestamp = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse current_timestamp".to_string()))?
        );

        Ok(CpiLog1 {
            pool,
            trade_direction,
            has_referral,
            params: SwapParams {
                amount_in,
                minimum_amount_out,
            },
            swap_result: SwapResult {
                output_amount,
                next_sqrt_price,
                lp_fee,
                protocol_fee,
                partner_fee,
                referral_fee,
            },
            actual_amount_in,
            current_timestamp,
        })
    }

    fn parse_cpi_log_2(instruction: &InnerInstruction) -> Result<CpiLog2, DexParserError> {
        let data = &instruction.data;

        // Minimum size check - should be at least 16 (discriminator) + 32 (pool) + basic fields
        if data.len() < 180 {
            return Err(DexParserError::InvalidInstructionData("CPI log 2 data too short".to_string()));
        }

        let mut offset = 16; // Skip discriminator

        // Parse pool (32 bytes)
        let pool = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse pool".to_string()))?
        );
        offset += 32;

        // Parse trade direction (1 byte)
        let trade_direction = data[offset];
        offset += 1;

        // Parse collect fee mode (1 byte)
        let collect_fee_mode = data[offset];
        offset += 1;

        // Parse has referral (1 byte)
        let has_referral = data[offset] != 0;
        offset += 1;

        // Parse SwapParams2
        // amount0 (8 bytes, u64 little endian)
        let amount0 = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse amount0".to_string()))?
        );
        offset += 8;

        // amount1 (8 bytes, u64 little endian)
        let amount1 = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse amount1".to_string()))?
        );
        offset += 8;

        // swap_mode (1 byte)
        let swap_mode = data[offset];
        offset += 1;

        // Parse SwapResult2
        // included_fee_input_amount (8 bytes, u64 little endian)
        let included_fee_input_amount = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse included_fee_input_amount".to_string()))?
        );
        offset += 8;

        // excluded_fee_input_amount (8 bytes, u64 little endian)
        let excluded_fee_input_amount = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse excluded_fee_input_amount".to_string()))?
        );
        offset += 8;

        // amount_left (8 bytes, u64 little endian)
        let amount_left = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse amount_left".to_string()))?
        );
        offset += 8;

        // output_amount (8 bytes, u64 little endian)
        let output_amount = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse output_amount".to_string()))?
        );
        offset += 8;

        // next_sqrt_price (16 bytes, u128 little endian)
        let next_sqrt_price = u128::from_le_bytes(
            data[offset..offset + 16]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse next_sqrt_price".to_string()))?
        );
        offset += 16;

        // trading_fee (8 bytes, u64 little endian)
        let trading_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse trading_fee".to_string()))?
        );
        offset += 8;

        // protocol_fee (8 bytes, u64 little endian)
        let protocol_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse protocol_fee".to_string()))?
        );
        offset += 8;

        // partner_fee (8 bytes, u64 little endian)
        let partner_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse partner_fee".to_string()))?
        );
        offset += 8;

        // referral_fee (8 bytes, u64 little endian)
        let referral_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse referral_fee".to_string()))?
        );
        offset += 8;

        // Parse remaining fields
        // included_transfer_fee_amount_in (8 bytes, u64 little endian)
        let included_transfer_fee_amount_in = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse included_transfer_fee_amount_in".to_string()))?
        );
        offset += 8;

        // included_transfer_fee_amount_out (8 bytes, u64 little endian)
        let included_transfer_fee_amount_out = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse included_transfer_fee_amount_out".to_string()))?
        );
        offset += 8;

        // excluded_transfer_fee_amount_out (8 bytes, u64 little endian)
        let excluded_transfer_fee_amount_out = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse excluded_transfer_fee_amount_out".to_string()))?
        );
        offset += 8;

        // current_timestamp (8 bytes, u64 little endian)
        let current_timestamp = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse current_timestamp".to_string()))?
        );
        offset += 8;

        // reserve_a_amount (16 bytes, u128 little endian)
        let reserve_a_amount = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse reserve_a_amount".to_string()))?
        );
        offset += 8;

        // reserve_b_amount (8 bytes, u64 little endian)
        let reserve_b_amount = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse reserve_b_amount".to_string()))?
        );

        Ok(CpiLog2 {
            pool,
            trade_direction,
            collect_fee_mode,
            has_referral,
            params: SwapParams2 {
                amount0,
                amount1,
                swap_mode,
            },
            swap_result: SwapResult2 {
                included_fee_input_amount,
                excluded_fee_input_amount,
                amount_left,
                output_amount,
                next_sqrt_price,
                trading_fee,
                protocol_fee,
                partner_fee,
                referral_fee,
            },
            included_transfer_fee_amount_in,
            included_transfer_fee_amount_out,
            excluded_transfer_fee_amount_out,
            current_timestamp,
            reserve_a_amount,
            reserve_b_amount,
        })
    }
}
