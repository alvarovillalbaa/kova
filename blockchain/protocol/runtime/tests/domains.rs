use runtime::{
    address_from_pubkey, apply_tx, bootstrap_state, tx_signing_bytes, DomainCall, Tx, TxPayload,
};
use ed25519_dalek::SigningKey;
use uuid::Uuid;

fn signer() -> SigningKey {
    SigningKey::from_bytes(&[3u8; 32])
}

fn build_tx(payload: TxPayload, sk: &SigningKey, nonce: u64) -> Tx {
    let pk = sk.verifying_key().to_bytes().to_vec();
    let mut tx = Tx {
        chain_id: "kova-devnet".into(),
        nonce,
        gas_limit: 300_000,
        max_fee: Some(1),
        max_priority_fee: Some(0),
        gas_price: None,
        payload,
        public_key: pk.clone(),
        signature: vec![],
    };
    let msg = tx_signing_bytes(&tx).unwrap();
    tx.signature = runtime::sign_bytes(sk, &msg);
    tx
}

#[tokio::test]
async fn domain_execute_and_cross_domain_flow() {
    let sk = signer();
    let mut ctx = bootstrap_state();

    // Register a wasm domain entry via DomainCreate
    let domain_id = Uuid::new_v4();
    let create_tx = build_tx(
        TxPayload::DomainCreate {
            domain_id,
            params: serde_json::json!({"kind": "wasm"}),
        },
        &sk,
        0,
    );
    apply_tx(&ctx, &create_tx, 0).await.unwrap();

    // Execute a wasm deployment call
    let wasm_payload = serde_json::json!({
        "action": "deploy",
        "module_id": "m1",
        "code_b64": base64::encode("00"),
    });
    let call = DomainCall {
        domain_id,
        payload: wasm_payload,
        raw: None,
        max_gas: Some(50_000),
    };
    let exec_tx = build_tx(TxPayload::DomainExecute(call), &sk, 1);
    let result = apply_tx(&ctx, &exec_tx, 1).await.unwrap();
    assert!(result.events.contains(&"domain_execute".into()));

    // Cross-domain send/relay roundtrip
    let dest_domain = Uuid::new_v4();
    let dest_tx = build_tx(
        TxPayload::DomainCreate {
            domain_id: dest_domain,
            params: serde_json::json!({"kind": "wasm"}),
        },
        &sk,
        2,
    );
    apply_tx(&ctx, &dest_tx, 2).await.unwrap();

    let send_tx = build_tx(
        TxPayload::CrossDomainSend {
            from_domain: domain_id,
            to_domain: dest_domain,
            payload: serde_json::json!({"hello": "world"}),
            fee: 1,
        },
        &sk,
        3,
    );
    apply_tx(&ctx, &send_tx, 3).await.unwrap();

    let msg = ctx.domains.outbox(&domain_id).last().cloned();
    assert!(msg.is_some());
    let relay_tx = build_tx(
        TxPayload::CrossDomainRelay {
            message: msg.unwrap(),
        },
        &sk,
        4,
    );
    apply_tx(&ctx, &relay_tx, 4).await.unwrap();

    // Fraud challenge path should accept a dummy witness for now.
    let fraud_tx = build_tx(
        TxPayload::FraudChallenge {
            domain_id,
            claimed_root: [1u8; 32],
            witness: serde_json::json!({"reason": "test"}),
        },
        &sk,
        5,
    );
    apply_tx(&ctx, &fraud_tx, 5).await.unwrap();

    let chain = ctx.state.get_chain_state().await.unwrap();
    let sender = address_from_pubkey(&sk.verifying_key().to_bytes());
    let account = chain.accounts.get(&sender).unwrap();
    assert!(account.nonce >= 6);
}
