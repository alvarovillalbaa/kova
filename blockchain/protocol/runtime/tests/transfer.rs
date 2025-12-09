use ed25519_dalek::SigningKey;
use runtime::{
    address_from_pubkey, apply_block, bootstrap_state, sign_bytes, tx_signing_bytes, Block,
    BlockHeader, Tx, TxPayload,
};
use state::{Account, StateStore};

#[tokio::test]
async fn transfer_moves_balance() {
    let ctx = bootstrap_state();
    let sk = SigningKey::from_bytes(&[3u8; 32]);
    let public_key = sk.verifying_key().to_bytes().to_vec();
    let from = address_from_pubkey(&public_key);
    let to = [2u8; 32];

    // fund sender
    ctx.state
        .put_account(Account {
            address: from,
            nonce: 0,
            balance_x: 1_000_000,
            code_hash: None,
            storage_root: None,
        })
        .await
        .unwrap();

    let mut tx = Tx {
        chain_id: "kova-devnet".into(),
        nonce: 0,
        gas_limit: 21_000,
        max_fee: None,
        max_priority_fee: None,
        gas_price: Some(1),
        payload: TxPayload::Transfer { to, amount: 10 },
        public_key: public_key.clone(),
        signature: vec![],
    };
    let msg = tx_signing_bytes(&tx).unwrap();
    tx.signature = sign_bytes(&sk, &msg);

    let block = Block {
        header: BlockHeader {
            parent_hash: [0u8; 32],
            height: 0,
            timestamp: 0,
            proposer_id: [0u8; 32],
            state_root: [0u8; 32],
            l1_tx_root: [0u8; 32],
        da_commitment: None,
            domain_roots: vec![],
            gas_used: 0,
            gas_limit: 30_000_000,
            base_fee: 0,
            consensus_metadata: serde_json::json!({}),
        },
        transactions: vec![tx],
        da_blobs: vec![],
    };

    let result = apply_block(&ctx, &block).await.unwrap();
    assert_ne!(result.state_root, [0u8; 32]);
}
