use consensus::{
    build_block, sign_proposal, sign_vote, ConsensusEngine, HotStuffEngine, SignedProposal,
    SignedVote,
};
use ed25519_dalek::SigningKey;
use proptest::prelude::*;
use runtime::{address_from_pubkey, hash_block, Block, BlockHeader};
use state::{Validator, ValidatorStatus};
use uuid::Uuid;

fn make_validator(seed: u8, stake: u128) -> (Validator, SigningKey) {
    let sk = SigningKey::from_bytes(&[seed; 32]);
    let pk = sk.verifying_key().to_bytes().to_vec();
    let owner = address_from_pubkey(&pk);
    let v = Validator {
        owner,
        id: Uuid::new_v4(),
        pubkey: pk,
        stake: stake.max(1),
        status: ValidatorStatus::Active,
        commission_rate: 0,
    };
    (v, sk)
}

fn empty_block_for(proposer: &Validator, height: u64) -> Block {
    let header = BlockHeader {
        parent_hash: [0u8; 32],
        height,
        timestamp: 0,
        proposer_id: proposer.owner,
        state_root: [0u8; 32],
        l1_tx_root: [0u8; 32],
        da_commitment: None,
        domain_roots: vec![],
        gas_used: 0,
        gas_limit: 30_000_000,
        base_fee: 1,
        consensus_metadata: serde_json::json!({}),
    };
    build_block(header, vec![], vec![])
}

proptest! {
    #[test]
    fn leader_rotation_always_chooses_known_validator(inputs in prop::collection::vec((1u8..=32u8, 1u128..=50_000u128), 1..6)) {
        let validators: Vec<Validator> = inputs.iter().enumerate().map(|(i, (seed, stake))| {
            let (v, _) = make_validator(seed.saturating_add(i as u8), *stake);
            v
        }).collect();

        let engine = HotStuffEngine::new(validators.clone());
        for view in 0..32 {
            let leader = engine.leader_for_view(view).expect("leader exists");
            prop_assert!(validators.iter().any(|v| v.id == leader.id));
        }
    }
}

#[tokio::test]
async fn quorum_commit_survives_timeout_and_late_votes() {
    let (v1, sk1) = make_validator(1, 10);
    let (v2, sk2) = make_validator(2, 15);
    let (v3, sk3) = make_validator(3, 25);
    let engine = HotStuffEngine::new(vec![v1.clone(), v2.clone(), v3.clone()]);

    let block = empty_block_for(&v1, 1);
    let proposal = SignedProposal {
        public_key: v1.pubkey.clone(),
        signature: sign_proposal(&block, &sk1),
        block: block.clone(),
    };
    engine.propose(proposal).await.unwrap();
    let block_id = hash_block(&block);

    // Simulate a timeout bumping the view before votes land.
    let view_before = engine.current_view();
    engine.on_timeout(view_before).await.unwrap();
    assert!(engine.current_view() >= view_before + 1);

    // Late votes for the original view should still accumulate and reach quorum.
    let vote0 = SignedVote {
        block_id,
        view: 0,
        voter: v1.clone(),
        signature: sign_vote(&block_id, 0, &sk1),
    };
    engine.vote(vote0).await.unwrap();

    let vote1 = SignedVote {
        block_id,
        view: 0,
        voter: v2.clone(),
        signature: sign_vote(&block_id, 0, &sk2),
    };
    engine.vote(vote1).await.unwrap();

    let vote2 = SignedVote {
        block_id,
        view: 0,
        voter: v3.clone(),
        signature: sign_vote(&block_id, 0, &sk3),
    };
    engine.vote(vote2).await.unwrap();

    assert!(engine.pop_commit().is_some());
}
