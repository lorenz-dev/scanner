use tracing::info;
use yellowstone_grpc_proto::geyser::SubscribeUpdateAccount;

use crate::{MintData, ScannerContext};

pub fn process_account(ctx: &ScannerContext, account_update: SubscribeUpdateAccount) -> () {
    // SPL Token account structure:
    // - mint: Pubkey at offset 0 (32 bytes)
    // - authority: Pubkey at offset 32 (32 bytes) - the PDA/authority
    // - balance: u64 at offset 64 (8 bytes)
    let account_info = account_update.account.clone().unwrap();

    let program_id = bs58::encode(account_info.pubkey).into_string();

    let data = account_info.data;
    let mut offset = 0;

    let mint: String = bs58::encode(&data[offset..offset + 32]).into_string();
    offset += 32;

    // authority
    offset += 32;

    let liquidity = u64::from_le_bytes(
        data[offset..offset + 8]
            .try_into()
            .unwrap()
    );

    info!("liquidity: {}", liquidity);

    ctx.add_mint_data(program_id, MintData {
        mint,
        liquidity,
    })
}