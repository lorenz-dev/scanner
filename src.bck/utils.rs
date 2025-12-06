use yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction;
use yellowstone_grpc_proto::prost::Message;

pub fn save_transaction_update(tx_update: SubscribeUpdateTransaction) {
    // Create transactions directory if it doesn't exist
    let dir = std::path::Path::new("transactions");
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::error!("Failed to create transactions directory: {}", e);
        return;
    }

    // Generate filename from signature
    let filename = if let Some(transaction) = &tx_update.transaction {
        if let Some(tx_data) = &transaction.transaction {
            if let Some(signature_bytes) = tx_data.signatures.first() {
                let signature_str = bs58::encode(signature_bytes).into_string();
                format!("{}.pb", signature_str)
            } else {
                format!("tx_slot_{}.pb", tx_update.slot)
            }
        } else {
            format!("tx_slot_{}.pb", tx_update.slot)
        }
    } else {
        format!("tx_slot_{}.pb", tx_update.slot)
    };

    let filepath = dir.join(&filename);

    // Encode to protobuf binary format
    let mut buf = Vec::new();
    if let Err(e) = tx_update.encode(&mut buf) {
        tracing::error!("Failed to encode transaction {}: {}", filename, e);
        return;
    }

    // Write to file
    if let Err(e) = std::fs::write(&filepath, buf) {
        tracing::error!("Failed to write transaction file {}: {}", filename, e);
    } else {
        tracing::debug!("Saved transaction to {}", filepath.display());
    }
}

pub fn load_transaction_update(filename: &str) -> SubscribeUpdateTransaction {
    let filepath = std::path::Path::new("transactions").join(filename);

    let bytes = std::fs::read(&filepath).unwrap();
    let tx_update = SubscribeUpdateTransaction::decode(&bytes[..]).unwrap();
    tx_update
}
