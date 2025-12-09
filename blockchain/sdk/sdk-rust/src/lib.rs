use ed25519_dalek::SigningKey;
use serde_json;
use uuid;
use runtime::{
    sign_bytes, tx_signing_bytes, CrossDomainMessage, DomainCall, Tx, TxPayload,
};

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

pub fn build_domain_execute_signed(
    chain_id: &str,
    call: DomainCall,
    signing_key: &SigningKey,
    nonce: u64,
    gas_limit: u64,
) -> anyhow::Result<Tx> {
    let public_key = signing_key.verifying_key().to_bytes().to_vec();
    let mut tx = Tx {
        chain_id: chain_id.to_string(),
        nonce,
        gas_limit,
        max_fee: Some(1),
        max_priority_fee: Some(0),
        gas_price: None,
        payload: TxPayload::DomainExecute(call),
        public_key: public_key.clone(),
        signature: vec![],
    };
    let bytes = tx_signing_bytes(&tx)?;
    tx.signature = sign_bytes(signing_key, &bytes);
    Ok(tx)
}

pub fn build_cross_domain_send_signed(
    chain_id: &str,
    from_domain: uuid::Uuid,
    to_domain: uuid::Uuid,
    payload: serde_json::Value,
    fee: u128,
    signing_key: &SigningKey,
    nonce: u64,
) -> anyhow::Result<Tx> {
    let public_key = signing_key.verifying_key().to_bytes().to_vec();
    let mut tx = Tx {
        chain_id: chain_id.to_string(),
        nonce,
        gas_limit: 90_000,
        max_fee: Some(1),
        max_priority_fee: Some(0),
        gas_price: None,
        payload: TxPayload::CrossDomainSend {
            from_domain,
            to_domain,
            payload,
            fee,
        },
        public_key: public_key.clone(),
        signature: vec![],
    };
    let bytes = tx_signing_bytes(&tx)?;
    tx.signature = sign_bytes(signing_key, &bytes);
    Ok(tx)
}

pub fn build_cross_domain_relay_signed(
    chain_id: &str,
    message: CrossDomainMessage,
    signing_key: &SigningKey,
    nonce: u64,
) -> anyhow::Result<Tx> {
    let public_key = signing_key.verifying_key().to_bytes().to_vec();
    let mut tx = Tx {
        chain_id: chain_id.to_string(),
        nonce,
        gas_limit: 50_000,
        max_fee: Some(1),
        max_priority_fee: Some(0),
        gas_price: None,
        payload: TxPayload::CrossDomainRelay { message },
        public_key: public_key.clone(),
        signature: vec![],
    };
    let bytes = tx_signing_bytes(&tx)?;
    tx.signature = sign_bytes(signing_key, &bytes);
    Ok(tx)
}

