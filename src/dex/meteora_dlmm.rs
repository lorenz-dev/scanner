use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::InnerInstruction;

use crate::{TokenAccounts, dex::{DexConfig, DexParser, DexParserError, DexSwap, DiscriminatorConfig}, token::TokenAmount};

#[derive(Debug)]
struct CpiLog {
    lb_pair: Pubkey,
    from: Pubkey,
    start_bin_id: i32,
    end_bin_id: i32,
    amount_in: u64,
    amount_out: u64,
    swap_for_y: bool,
    fee: u64,
    protocol_fee: u64,
    fee_bps: u64,
    host_fee: u64,
}

pub struct MeteoraDelmmParser {
    config: DexConfig,
}

impl DexParser for MeteoraDelmmParser {
    fn config(&self) -> &DexConfig {
        &self.config
    }
}

impl MeteoraDelmmParser {
    pub fn new() -> Self {
        let config = DexConfig {
            name: "Meteora DLMM",
            program_id: pubkey!("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo"),
            discriminator_length: 16,
            discriminators: vec![
                DiscriminatorConfig {
                    name: "swap",
                    discriminator: "f8c69e91e17587c8",
                    parse_fn: Self::parse_swap,
                },
                DiscriminatorConfig {
                    name: "swap2",
                    discriminator: "414b3f4ceb5b5b88",
                    parse_fn: Self::parse_swap2,
                }
            ]
        };

        Self { config }
    }

    fn parse_swap(
        instruction: &InnerInstruction,
        inner_instructions: &Vec<&InnerInstruction>,
        accounts: &Vec<Pubkey>,
        _token_accounts: &TokenAccounts,
    ) -> Result<DexSwap, DexParserError> {
        let instruction_accounts = Self::get_instruction_accounts(instruction, accounts);

        let swap_program_id = accounts[instruction.program_id_index as usize];

        let pool_a = instruction_accounts[2];
        let pool_b = instruction_accounts[3];

        let token_x_mint = instruction_accounts[6];
        let token_y_mint = instruction_accounts[7];

        for inner_instruction in inner_instructions {
            if inner_instruction.data.len() <= 16 {
                continue;
            }
            let cpi_log_discriminator = hex::encode(&inner_instruction.data[0..8]);
            if cpi_log_discriminator == "e445a52e51cb9a1d" {
                let cpi_log = Self::parse_cpi_log(inner_instruction)?;

                // Determine token_in and token_out based on swap_for_y
                let (token_in_mint, token_out_mint) = if cpi_log.swap_for_y {
                    (token_x_mint, token_y_mint)
                } else {
                    (token_y_mint, token_x_mint)
                };

                // Collect fees
                let mut fees = Vec::new();
                if cpi_log.fee > 0 {
                    fees.push(TokenAmount {
                        mint: token_in_mint,
                        amount: cpi_log.fee,
                    });
                }
                if cpi_log.protocol_fee > 0 {
                    fees.push(TokenAmount {
                        mint: token_in_mint,
                        amount: cpi_log.protocol_fee,
                    });
                }
                if cpi_log.host_fee > 0 {
                    fees.push(TokenAmount {
                        mint: token_in_mint,
                        amount: cpi_log.host_fee,
                    });
                }
                if cpi_log.fee_bps > 0 {
                    fees.push(TokenAmount {
                        mint: token_in_mint,
                        amount: cpi_log.fee_bps,
                    });
                }

                return Ok(DexSwap {
                    swap_program_id,
                    pools: vec![pool_a, pool_b],
                    pool_owner: cpi_log.lb_pair,  // lb_pair is the pool owner
                    token_in: TokenAmount {
                        mint: token_in_mint,
                        amount: cpi_log.amount_in,
                    },
                    token_out: TokenAmount {
                        mint: token_out_mint,
                        amount: cpi_log.amount_out,
                    },
                    fees,
                    vault_accounts: Vec::new(),
                });
            }
        }

        // Fallback
        let ins_data: SwapInstructionData = Self::parse_instruction_data(instruction)?;

        let lb_pair = instruction_accounts[0];

        Ok(DexSwap {
            swap_program_id,
            pools: vec![pool_a, pool_b],
            pool_owner: lb_pair,  // lb_pair is the pool owner
            token_in: TokenAmount {
                mint: token_x_mint,
                amount: ins_data.amount_in
            },
            token_out: TokenAmount {
                mint: token_y_mint,
                amount: ins_data.min_amount_out
            },
            fees: Vec::new(),
            vault_accounts: Vec::new(),
        })
    }

    fn parse_swap2(
        instruction: &InnerInstruction,
        inner_instructions: &Vec<&InnerInstruction>,
        accounts: &Vec<Pubkey>,
        token_accounts: &TokenAccounts,
    ) -> Result<DexSwap, DexParserError> {
        // let mut swap = DexSwap::default();

        // let instruction_accounts = Self::get_instruction_accounts(instruction, accounts);

        // // Get token mints from instruction accounts
        // // Typical Meteora DLMM layout has token mints at specific indices
        // let token_x_mint = instruction_accounts[6];
        // let token_y_mint = instruction_accounts[7];

        // if inner_instructions.get(0).is_none() {
        //     let ins_data: SwapInstructionData = Self::parse_instruction_data(instruction)?;

        //     swap.token_in = TokenAmount {
        //         mint: token_x_mint,
        //         amount: ins_data.amount_in
        //     };

        //     swap.token_out = TokenAmount {
        //         mint: token_y_mint,
        //         amount: ins_data.min_amount_out
        //     };

        //     return Ok(swap);
        // }

        // // Find and parse the CPI log from inner instructions
        // // let cpi_log = Self::parse_cpi_log(inner_instructions.last().ok_or_else(|| {
        // //     DexParserError::InvalidInstructionData("No inner instructions found".to_string())
        // // })?)?;

        // let cpi_log = Self::parse_cpi_log(inner_instructions[0])?;

        // // Determine token_in and token_out based on swap_for_y
        // let (token_in_mint, token_out_mint) = if cpi_log.swap_for_y {
        //     (token_x_mint, token_y_mint)
        // } else {
        //     (token_y_mint, token_x_mint)
        // };

        // swap.token_in = TokenAmount {
        //     mint: token_in_mint,
        //     amount: cpi_log.amount_in,
        // };

        // swap.token_out = TokenAmount {
        //     mint: token_out_mint,
        //     amount: cpi_log.amount_out,
        // };

        // // Add fees
        // if cpi_log.fee > 0 {
        //     swap.fees.push(TokenAmount {
        //         mint: token_in_mint,
        //         amount: cpi_log.fee,
        //     });
        // }

        // if cpi_log.protocol_fee > 0 {
        //     swap.fees.push(TokenAmount {
        //         mint: token_in_mint,
        //         amount: cpi_log.protocol_fee,
        //     });
        // }

        // if cpi_log.host_fee > 0 {
        //     swap.fees.push(TokenAmount {
        //         mint: token_in_mint,
        //         amount: cpi_log.host_fee,
        //     });
        // }
        // if cpi_log.fee_bps > 0 {
        //     swap.fees.push(TokenAmount {
        //         mint: token_in_mint,
        //         amount: cpi_log.fee_bps,
        //     });
        // }

        // Ok(swap)

        Self::parse_swap(instruction, inner_instructions, accounts, token_accounts)
    }

    fn parse_instruction_data(instruction: &InnerInstruction) -> Result<SwapInstructionData, DexParserError> {
        // 414b3f4ceb5b5b880b16809b2e010000000000000000000000000000

        // {
        // "amount_in": {
        //     "type": "u64",
        //     "data": "1299688986123"
        // },
        // "min_amount_out": {
        //     "type": "u64",
        //     "data": "0"
        // },
        // "remaining_accounts_info": {
        //     "type": {
        //     "defined": {
        //         "name": "RemainingAccountsInfo"
        //     }
        //     },
        //     "data": {
        //     "slices": {
        //         "type": {
        //         "vec": {
        //             "defined": {
        //             "name": "RemainingAccountsSlice"
        //             }
        //         }
        //         },
        //         "data": []
        //     }
        //     }
        // }
        // }
        let data = &instruction.data;

        // Skip discriminator (first 8 bytes)
        let mut offset = 8;

        // Parse amount_in (u64, 8 bytes)
        let amount_in_bytes: [u8; 8] = data[offset..offset + 8]
            .try_into()
            .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse amount_in".to_string()))?;
        let amount_in = u64::from_le_bytes(amount_in_bytes);
        offset += 8;

        // Parse min_amount_out (u64, 8 bytes)
        let min_amount_out_bytes: [u8; 8] = data[offset..offset + 8]
            .try_into()
            .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse min_amount_out".to_string()))?;
        let min_amount_out = u64::from_le_bytes(min_amount_out_bytes);
        offset += 8;

        // Parse remaining_accounts_info (optional)
        // For now, we'll treat this as opaque data since it's a complex nested structure
        // and may not be needed for basic swap parsing
        let remaining_accounts_info = if offset < data.len() {
            Some(data[offset..].to_vec())
        } else {
            None
        };

        Ok(SwapInstructionData {
            amount_in,
            min_amount_out,
            remaining_accounts_info,
        })
    }

    fn parse_cpi_log(instruction: &InnerInstruction) -> Result<CpiLog, DexParserError> {
        // e445a52e51cb9a1d
        let data = &instruction.data;

        if data.len() >= 290 {
            return Err(DexParserError::InvalidInstructionData("CPI log data too short".to_string()));
        }

        let mut offset = 16; // Skip discriminator

        // Parse lb_pair (32 bytes)
        let lb_pair = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse lb_pair".to_string()))?
        );
        offset += 32;

        // Parse from (32 bytes)
        let from = Pubkey::new_from_array(
            data[offset..offset + 32]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse from".to_string()))?
        );
        offset += 32;

        // Parse start_bin_id (4 bytes, i32 little endian)
        let start_bin_id = i32::from_le_bytes(
            data[offset..offset + 4]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse start_bin_id".to_string()))?
        );
        offset += 4;

        // Parse end_bin_id (4 bytes, i32 little endian)
        let end_bin_id = i32::from_le_bytes(
            data[offset..offset + 4]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse end_bin_id".to_string()))?
        );
        offset += 4;

        // Parse amount_in (8 bytes, u64 little endian)
        let amount_in = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse amount_in".to_string()))?
        );
        offset += 8;

        // Parse amount_out (8 bytes, u64 little endian)
        let amount_out = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse amount_out".to_string()))?
        );
        offset += 8;

        // Parse swap_for_y (1 byte, bool)
        let swap_for_y = data[offset] != 0;
        offset += 1;

        // Parse fee (8 bytes, u64 little endian)
        let fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse fee".to_string()))?
        );
        offset += 8;

        // Parse protocol_fee (8 bytes, u64 little endian)
        let protocol_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse protocol_fee".to_string()))?
        );
        offset += 8;

        // Parse fee_bps (8 bytes, u64 little endian)
        let fee_bps = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse fee_bps".to_string()))?
        );
        offset += 8;

        // Parse host_fee (8 bytes, u64 little endian)
        let host_fee = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .map_err(|_| DexParserError::InvalidInstructionData("Failed to parse host_fee".to_string()))?
        );

        Ok(CpiLog {
            lb_pair,
            from,
            start_bin_id,
            end_bin_id,
            amount_in,
            amount_out,
            swap_for_y,
            fee,
            protocol_fee,
            fee_bps,
            host_fee,
        })
    }
}

#[derive(Debug)]
struct SwapInstructionData {
    amount_in: u64,
    min_amount_out: u64,
    remaining_accounts_info: Option<Vec<u8>>,
}
