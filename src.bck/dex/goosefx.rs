use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;
use crate::dex::{DexConfig, DexParser};

pub struct GooseFXParser {
    config: DexConfig,
}

impl DexParser for GooseFXParser {
    fn config(&self) -> &DexConfig {
        &self.config
    }
}

impl GooseFXParser {
    pub fn new() -> Self {
        let config = DexConfig {
            name: "GooseFX: GAMMA",
            program_id: pubkey!("GAMMA7meSFWaBXF25oSUgmGRwaW6sCMFLmBNiMSdbHVT"),
            discriminator_length: 16,
            discriminators: vec![]
        };

        Self { config }
    }
}
