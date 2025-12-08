use ed25519_dalek::SigningKey;
use runtime::{sign_bytes, tx_signing_bytes, Tx, TxPayload};

pub async fn send_raw_tx(endpoint: &str, tx: &Tx) -> anyhow::Result<()> {
    let _ = (endpoint, tx);
    // Placeholder: serialize and POST to node RPC.
    Ok(())
}

pub fn build_transfer_signed(
    chain_id: &str,
    to: [u8; 32],
    amount: u128,
    signing_key: &SigningKey,
    nonce: u64,
) -> anyhow::Result<Tx> {
    let public_key = signing_key.verifying_key().to_bytes().to_vec();
    let mut tx = Tx {
        chain_id: chain_id.to_string(),
        nonce,
        gas_limit: 21_000,
        max_fee: Some(1),
        max_priority_fee: Some(0),
        gas_price: None,
        payload: TxPayload::Transfer { to, amount },
        public_key: public_key.clone(),
        signature: vec![],
    };
    let bytes = tx_signing_bytes(&tx)?;
    tx.signature = sign_bytes(signing_key, &bytes);
    Ok(tx)
}

