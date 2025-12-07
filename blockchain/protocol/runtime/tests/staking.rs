use runtime::{apply_block, bootstrap_state, Block, BlockHeader, Tx, TxPayload};
use state::{Account, StateStore};

#[tokio::test]
async fn stake_creates_validator_and_updates_balance() {
    let ctx = bootstrap_state();
    let owner = [7u8; 32];

    // fund owner
    ctx.state
        .put_account(Account {
            address: owner,
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
        gas_limit: 50_000,
        max_fee: None,
        max_priority_fee: None,
        gas_price: Some(1),
        payload: TxPayload::Stake { amount: 100_000 },
        signature: owner.to_vec(), // derives sender
    };

    let block = Block {
        header: BlockHeader {
            parent_hash: [0u8; 32],
            height: 0,
            timestamp: 0,
            proposer_id: owner,
            state_root: [0u8; 32],
            l1_tx_root: [0u8; 32],
            da_root: [0u8; 32],
            domain_roots: vec![],
            gas_used: 0,
            gas_limit: 30_000_000,
            base_fee: 1,
            consensus_metadata: serde_json::json!({}),
        },
        transactions: vec![tx],
        da_blobs: vec![],
    };

    let result = apply_block(&ctx, &block).await.unwrap();
    assert_ne!(result.state_root, [0u8; 32]);

    let chain = ctx.state.get_chain_state().await.unwrap();
    assert_eq!(chain.validators.len(), 1);
    let validator = chain.validators.values().next().unwrap();
    assert_eq!(validator.owner, owner);
    assert!(validator.stake >= 100_000);
}
