use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_proto::prelude::InnerInstruction;

use crate::{TokenAccount, TokenAccounts};

const TOKEN_PROGRAM: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const TOKEN_2022_PROGRAM: Pubkey = pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

pub struct Token {}

#[derive(Default, Debug)]
pub struct TokenAmount {
    pub mint: Pubkey,
    pub amount: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TokenWithAmount {
    pub amount: u64,
    pub token_info: TokenInfo,
    pub source_account: Option<Pubkey>,  // The token account (vault) this came from
}

#[derive(Debug)]
pub enum TokenWithAmountError {
    CannotAddDifferentToken(String, String),
}

impl std::fmt::Display for TokenWithAmountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenWithAmountError::CannotAddDifferentToken(mint1, mint2) => {
                write!(f, "Cannot add different tokens: {} and {}", mint1, mint2)
            }
        }
    }
}

impl std::error::Error for TokenWithAmountError {}

impl TokenWithAmount {
    pub fn add(&mut self, token: TokenWithAmount) -> Result<(), TokenWithAmountError> {
        if self.token_info.mint == Pubkey::default() {
            self.token_info.mint = token.token_info.mint;
        }
        if self.token_info.mint != token.token_info.mint {
            return Err(TokenWithAmountError::CannotAddDifferentToken(
                self.token_info.mint.to_string(),
                token.token_info.mint.to_string()
            ));
        }
        self.amount += token.amount;
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct TokenInfo {
    pub mint: Pubkey,
    pub decimals: u8,
}

#[derive(Debug)]
pub enum TokenError {
    InvalidProgramId(Pubkey),
    InvalidInstructionData(String),
    InvalidAccountIndex(usize),
    ParseError(String),
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenError::InvalidProgramId(pubkey) => write!(f, "Invalid program ID: {}", pubkey),
            TokenError::InvalidInstructionData(msg) => write!(f, "Invalid instruction data: {}", msg),
            TokenError::InvalidAccountIndex(idx) => write!(f, "Invalid account index: {}", idx),
            TokenError::ParseError(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

impl std::error::Error for TokenError {}

impl Token {
    pub fn token_transfer(instruction: &InnerInstruction, accounts: &Vec<Pubkey>, token_accounts: &TokenAccounts) -> Result<TokenWithAmount, TokenError> {
        let program_id = accounts
            .get(instruction.program_id_index as usize)
            .ok_or(TokenError::InvalidAccountIndex(instruction.program_id_index as usize))?;

        match *program_id {
            TOKEN_PROGRAM => TokenProgram::parse_transfer(instruction, accounts, token_accounts),
            TOKEN_2022_PROGRAM => Token2022Program::parse_transfer(instruction, accounts, token_accounts),
            _ => Err(TokenError::InvalidProgramId(*program_id)),
        }
    }

    pub fn parse_transfer_amount(instruction: &InnerInstruction) -> Result<u64, TokenError> {
        let data = &instruction.data;

        let amount: u64 = u64::from_le_bytes(
            data[1..9].try_into()
                .map_err(|_| TokenError::InvalidInstructionData("Failed to parse amount".to_string()))?
        );

        Ok(amount)
    }
}

struct TokenProgram;

impl TokenProgram {
    fn parse_transfer(instruction: &InnerInstruction, accounts: &Vec<Pubkey>, token_accounts: &TokenAccounts) -> Result<TokenWithAmount, TokenError> {
        // SPL Token Transfer instruction format:
        // [0]: instruction type (3 = Transfer, 12 = TransferChecked)
        // For Transfer:
        //   [1-8]: amount (u64, little-endian)
        // For TransferChecked:
        //   [1-8]: amount (u64, little-endian)
        //   [9]: decimals (u8)

        let data = &instruction.data;

        if data.is_empty() {
            return Err(TokenError::InvalidInstructionData("Empty instruction data".to_string()));
        }

        let account_indices: Vec<usize> = instruction.accounts
            .iter()
            .map(|idx| *idx as usize)
            .collect();

        let instruction_type = data[0];

        match instruction_type {
            3 => {
                // Transfer instruction
                // Account layout: [source, destination, authority]
                if data.len() < 9 {
                    return Err(TokenError::InvalidInstructionData("Transfer instruction data too short".to_string()));
                }

                let amount: u64 = u64::from_le_bytes(
                    data[1..9].try_into()
                        .map_err(|_| TokenError::InvalidInstructionData("Failed to parse amount".to_string()))?
                );

                let source_account_pubkey = accounts[account_indices[0]];

                let token_info = token_accounts.get(&source_account_pubkey)
                    .ok_or_else(|| TokenError::ParseError(format!("Token account not found: {}", source_account_pubkey)))?
                    .token_info.clone();

                Ok(TokenWithAmount {
                    amount,
                    token_info,
                    source_account: Some(source_account_pubkey),
                })
            }
            12 => {
                // TransferChecked instruction
                if data.len() < 10 {
                    return Err(TokenError::InvalidInstructionData("TransferChecked instruction data too short".to_string()));
                }

                let amount = u64::from_le_bytes(
                    data[1..9].try_into()
                        .map_err(|_| TokenError::InvalidInstructionData("Failed to parse amount".to_string()))?
                );

                let decimals = data[9];

                // Get mint from accounts (index 3 for TransferChecked)
                let mint = accounts.get(instruction.program_id_index as usize + 3)
                    .copied()
                    .unwrap_or_default();

                // Get source account for TransferChecked (index 0)
                let source_account = accounts.get(account_indices[0]).copied();

                Ok(TokenWithAmount {
                    amount,
                    token_info: TokenInfo {
                        mint,
                        decimals,
                    },
                    source_account,
                })
            }
            _ => Err(TokenError::InvalidInstructionData(format!("Unknown instruction type: {}", instruction_type))),
        }
    }
}

struct Token2022Program;

impl Token2022Program {
    fn parse_transfer(instruction: &InnerInstruction, accounts: &Vec<Pubkey>, token_accounts: &TokenAccounts) -> Result<TokenWithAmount, TokenError> {
        // Token-2022 uses the same instruction format as SPL Token
        // Reuse the same parsing logic
        TokenProgram::parse_transfer(instruction, accounts, token_accounts)
    }
}
