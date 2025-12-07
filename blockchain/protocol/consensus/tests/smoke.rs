use consensus::{ConsensusEngine, HotStuffEngine};
use state::{Validator, ValidatorStatus};
use uuid::Uuid;

#[tokio::test]
async fn quorum_reached_with_stake_weight() {
    let v1 = Validator {
        owner: [1u8; 32],
        id: Uuid::new_v4(),
        pubkey: vec![1],
        stake: 10,
        status: ValidatorStatus::Active,
        commission_rate: 0,
    };
    let v2 = Validator {
        owner: [2u8; 32],
        id: Uuid::new_v4(),
        pubkey: vec![2],
        stake: 10,
        status: ValidatorStatus::Active,
        commission_rate: 0,
    };
    let engine = HotStuffEngine::new(vec![v1.clone(), v2.clone()]);
    let block_id = [0u8; 32];

    engine.vote(block_id, 0, &v1).await.unwrap();
    assert!(engine.pop_commit().is_none());

    engine.vote(block_id, 0, &v2).await.unwrap();
    assert!(engine.pop_commit().is_some());
}

