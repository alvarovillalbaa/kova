use consensus::{SignedProposal, SignedVote};
use runtime::Block;
use serde::{Deserialize, Serialize};
use state::Validator;

#[derive(Debug, Clone)]
pub struct GossipMessage {
    pub topic: String,
    pub payload: Vec<u8>,
}

pub trait Gossip: Send + Sync {
    fn publish(&self, msg: GossipMessage);
    fn subscribe(&self, topic: &str);
}

pub trait BlockPropagation: Send + Sync {
    fn broadcast_block(&self, block: &Block);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusMessage {
    Propose(SignedProposal),
    Vote(SignedVote),
    Timeout {
        view: u64,
        from: Validator,
    },
}

pub trait ConsensusNetwork: Send + Sync {
    fn broadcast(&self, msg: ConsensusMessage);
}

#[derive(Default)]
pub struct NoopConsensusNetwork;

impl ConsensusNetwork for NoopConsensusNetwork {
    fn broadcast(&self, _msg: ConsensusMessage) {
        // no-op for single-node devnet or tests
    }
}

