use runtime::Tx;

pub async fn send_raw_tx(endpoint: &str, tx: &Tx) -> anyhow::Result<()> {
    let _ = (endpoint, tx);
    // Placeholder: serialize and POST to node RPC.
    Ok(())
}

pub fn build_transfer(chain_id: &str, to: [u8; 32], amount: u128) -> Tx {
    Tx {
        chain_id: chain_id.to_string(),
        nonce: 0,
        gas_limit: 21_000,
        max_fee: None,
        max_priority_fee: None,
        gas_price: Some(1),
        payload: runtime::TxPayload::Transfer { to, amount },
        signature: vec![],
    }
}

