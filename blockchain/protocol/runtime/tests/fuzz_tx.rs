use proptest::prelude::*;
use runtime::{
    address_from_pubkey, sign_bytes, tx_signing_bytes, CrossDomainMessage, DomainCall, Tx,
    TxPayload,
};
use uuid::Uuid;

fn arb_address() -> impl Strategy<Value = [u8; 32]> {
    prop::array::uniform32(any::<u8>())
}

fn arb_uuid() -> impl Strategy<Value = Uuid> {
    prop::array::uniform16(any::<u8>()).prop_map(Uuid::from_bytes)
}

fn arb_json() -> impl Strategy<Value = serde_json::Value> {
    let small_str = proptest::string::string_regex(".{0,16}").unwrap();
    prop_oneof![
        Just(serde_json::json!({})),
        any::<u64>().prop_map(|n| serde_json::json!(n)),
        small_str.prop_map(|s| serde_json::json!(s)),
    ]
}

fn arb_domain_call() -> impl Strategy<Value = DomainCall> {
    (arb_uuid(), arb_json(), prop::collection::vec(any::<u8>(), 0..64), prop::option::of(any::<u64>()))
        .prop_map(|(domain_id, payload, raw, max_gas)| DomainCall {
            domain_id,
            payload,
            raw,
            max_gas,
        })
}

fn arb_payload() -> impl Strategy<Value = TxPayload> {
    prop_oneof![
        (arb_address(), any::<u128>()).prop_map(|(to, amount)| TxPayload::Transfer { to, amount }),
        arb_domain_call().prop_map(TxPayload::DomainExecute),
        (arb_uuid(), arb_uuid(), arb_json(), any::<u128>()).prop_map(|(from_domain, to_domain, payload, fee)| TxPayload::CrossDomainSend {
            from_domain,
            to_domain,
            payload,
            fee,
        }),
        (
            arb_uuid(),
            arb_uuid(),
            any::<u64>(),
            any::<u128>(),
            arb_json()
        )
            .prop_map(|(from, to, nonce, fee, payload)| TxPayload::CrossDomainRelay {
                message: CrossDomainMessage {
                    from,
                    to,
                    nonce,
                    fee,
                    payload,
                },
            }),
        (arb_uuid(), arb_json()).prop_map(|(domain_id, params)| TxPayload::DomainCreate { domain_id, params }),
        (arb_uuid(), arb_json()).prop_map(|(domain_id, params)| TxPayload::DomainConfigUpdate { domain_id, params }),
        (arb_uuid(), any::<u128>()).prop_map(|(domain_id, amount)| TxPayload::RollupBridgeDeposit { domain_id, amount }),
    ]
}

prop_compose! {
    fn arb_signed_tx()(sk_bytes in prop::array::uniform32(any::<u8>()),
                       payload in arb_payload(),
                       nonce in 0u64..8,
                       gas_limit in 21_000u64..2_000_000u64) -> Tx {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&sk_bytes);
        let public_key = signing_key.verifying_key().to_bytes().to_vec();

        let mut tx = Tx {
            chain_id: "kova-devnet".into(),
            nonce,
            gas_limit,
            max_fee: Some(1),
            max_priority_fee: Some(0),
            gas_price: Some(1),
            payload,
            public_key: public_key.clone(),
            signature: vec![],
        };
        let msg = tx_signing_bytes(&tx).expect("signable bytes");
        tx.signature = sign_bytes(&signing_key, &msg);
        tx
    }
}

proptest! {
    #[test]
    fn bincode_roundtrip_preserves_tx_and_signature(tx in arb_signed_tx()) {
        let encoded = bincode::serialize(&tx).unwrap();
        let decoded: Tx = bincode::deserialize(&encoded).unwrap();

        let orig_payload = bincode::serialize(&tx.payload).unwrap();
        let decoded_payload = bincode::serialize(&decoded.payload).unwrap();
        prop_assert_eq!(orig_payload, decoded_payload);

        let orig_signing = tx_signing_bytes(&tx).unwrap();
        let decoded_signing = tx_signing_bytes(&decoded).unwrap();
        prop_assert_eq!(orig_signing, decoded_signing);

        let addr = address_from_pubkey(&decoded.public_key);
        prop_assert_eq!(addr, runtime::verify_tx_signature(&decoded).unwrap());
    }
}
