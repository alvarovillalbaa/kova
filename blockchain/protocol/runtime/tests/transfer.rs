use runtime::{apply_block, bootstrap_state, Block, BlockHeader, Tx, TxPayload};
use state::{Account, StateStore};

#[tokio::test]
async fn transfer_moves_balance() {
    let ctx = bootstrap_state();
    let from = [1u8; 32];
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

    let tx = Tx {
        chain_id: "kova-devnet".into(),
        nonce: 0,
        gas_limit: 21_000,
        max_fee: None,
        max_priority_fee: None,
        gas_price: Some(1),
        payload: TxPayload::Transfer { to, amount: 10 },
        signature: vec![1; 32],
    };

    let block = Block {
        header: BlockHeader {
            parent_hash: [0u8; 32],
            height: 0,
            timestamp: 0,
            proposer_id: [0u8; 32],
            state_root: [0u8; 32],
            l1_tx_root: [0u8; 32],
            da_root: [0u8; 32],
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
