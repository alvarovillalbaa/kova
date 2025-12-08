use async_trait::async_trait;
use blake3;
use runtime::{
    address_from_pubkey, hash_block, sign_bytes, verify_signature_bytes, Block, BlockHeader, Hash,
    Tx,
};
use serde::{Deserialize, Serialize};
use state::Validator;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::time::{self, Duration};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedProposal {
    pub block: Block,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedVote {
    pub block_id: Hash,
    pub view: u64,
    pub voter: Validator,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashEvidence {
    pub validator_id: Uuid,
    pub reason: String,
    pub height: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ConsensusMetrics {
    pub current_view: u64,
    pub locked_qc: bool,
    pub pending_qc: bool,
    pub commit_queue_depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumCertificate {
    pub block_id: Hash,
    pub view: u64,
    pub signatures: Vec<Vec<u8>>,
    pub voters: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusState {
    pub view: u64,
    pub height: u64,
    pub locked_qc: Option<QuorumCertificate>,
    pub pending_qc: Option<QuorumCertificate>,
}

#[async_trait]
pub trait ConsensusEngine: Send + Sync {
    async fn propose(&self, proposal: SignedProposal) -> anyhow::Result<()>;
    async fn vote(&self, vote: SignedVote) -> anyhow::Result<()>;
    async fn on_qc(&self, qc: QuorumCertificate) -> anyhow::Result<()>;
    async fn on_timeout(&self, view: u64) -> anyhow::Result<()>;
    async fn validator_set(&self) -> anyhow::Result<Vec<Validator>>;
    async fn record_slash(&self, evidence: SlashEvidence) -> anyhow::Result<()>;
    fn metrics(&self) -> ConsensusMetrics;
    fn pop_commit(&self) -> Option<Hash>;
    fn leader_for_view(&self, view: u64) -> Option<Validator>;
    fn current_view(&self) -> u64;
}

pub fn build_block(header: BlockHeader, txs: Vec<Tx>, da_blobs: Vec<String>) -> Block {
    Block {
        header,
        transactions: txs,
        da_blobs,
    }
}

#[derive(Clone)]
pub struct HotStuffEngine {
    inner: Arc<Mutex<HotStuffInner>>,
    timeout: Duration,
}

#[derive(Debug)]
struct HotStuffInner {
    state: ConsensusState,
    pending_blocks: HashMap<Hash, Block>,
    block_tree: HashMap<Hash, Block>,
    votes: HashMap<(Hash, u64), VoteTally>,
    validators: Vec<Validator>,
    commit_queue: VecDeque<Hash>,
    total_stake: u128,
}

#[derive(Debug, Default, Clone)]
struct VoteTally {
    stake: u128,
    voters: Vec<Uuid>,
    signatures: Vec<Vec<u8>>,
}

impl HotStuffEngine {
    pub fn new(validators: Vec<Validator>) -> Self {
        let inner = HotStuffInner {
            state: ConsensusState {
                view: 0,
                height: 0,
                locked_qc: None,
                pending_qc: None,
            },
            pending_blocks: HashMap::new(),
            block_tree: HashMap::new(),
            votes: HashMap::new(),
            total_stake: validators.iter().map(|v| v.stake).sum(),
            validators,
            commit_queue: VecDeque::new(),
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
            timeout: Duration::from_millis(1_500),
        }
    }

    pub async fn run_timeouts(self) {
        let mut interval = time::interval(self.timeout);
        loop {
            interval.tick().await;
            let view = { self.inner.lock().unwrap().state.view };
            let _ = self.on_timeout(view).await;
        }
    }
}

impl HotStuffInner {
    fn quorum_threshold(&self) -> u128 {
        (self.total_stake * 2) / 3 + 1
    }

    fn leader_for_view(&self, view: u64) -> Option<Validator> {
        if self.validators.is_empty() {
            return None;
        }
        let mut slot = (view as u128) % self.total_stake.max(1);
        for v in &self.validators {
            if slot < v.stake {
                return Some(v.clone());
            }
            slot = slot.saturating_sub(v.stake);
        }
        self.validators.first().cloned()
    }
}

#[async_trait]
impl ConsensusEngine for HotStuffEngine {
    async fn propose(&self, proposal: SignedProposal) -> anyhow::Result<()> {
        let block = proposal.block.clone();
        let block_id = hash_block(&block);
        verify_proposal(&proposal, block_id)?;

        let mut guard = self.inner.lock().unwrap();
        if let Some(leader) = guard.leader_for_view(guard.state.view) {
            if leader.owner != block.header.proposer_id {
                tracing::warn!("proposal from non-leader for view {}", guard.state.view);
            }
        }
        let block_clone = proposal.block.clone();
        guard.pending_blocks.insert(block_id, block_clone.clone());
        guard.block_tree.insert(block_id, block_clone);
        guard.state.view += 1;
        Ok(())
    }

    async fn vote(&self, vote: SignedVote) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        verify_vote(&vote, &guard.validators)?;
        let block_id = vote.block_id;
        let view = vote.view;

        let threshold = guard.quorum_threshold();
        let (enough, signatures, voters) = {
            let tally = guard
                .votes
                .entry((block_id, view))
                .or_insert_with(VoteTally::default);

            if tally.voters.contains(&vote.voter.id) {
                return Ok(()); // ignore duplicate vote
            }

            tally.voters.push(vote.voter.id);
            tally.stake = tally.stake.saturating_add(vote.voter.stake);
            tally.signatures.push(vote.signature.clone());

            if tally.stake >= threshold {
                (true, tally.signatures.clone(), tally.voters.clone())
            } else {
                (false, Vec::new(), Vec::new())
            }
        };

        if enough {
            let qc = QuorumCertificate {
                block_id,
                view,
                signatures,
                voters,
            };
            guard.state.pending_qc = Some(qc.clone());
            guard.state.locked_qc = Some(qc.clone());
            guard.commit_queue.push_back(block_id);
        }
        Ok(())
    }

    async fn on_qc(&self, qc: QuorumCertificate) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        guard.state.locked_qc = Some(qc.clone());
        guard.state.height += 1;

        // 3-chain commit simulation: commit parent of qc.block_id if exists.
        if let Some(current) = guard.block_tree.get(&qc.block_id) {
            let parent_hash = current.header.parent_hash;
            if parent_hash != [0u8; 32] {
                guard.commit_queue.push_back(parent_hash);
            }
        }

        Ok(())
    }

    async fn on_timeout(&self, view: u64) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        if view == guard.state.view {
            guard.state.view += 1;
        }
        Ok(())
    }

    async fn validator_set(&self) -> anyhow::Result<Vec<Validator>> {
        let guard = self.inner.lock().unwrap();
        Ok(guard.validators.clone())
    }

    async fn record_slash(&self, evidence: SlashEvidence) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        // Placeholder: slashing is enforced by runtime; consensus records evidence for observability.
        let digest = blake3::hash(evidence.validator_id.as_bytes());
        guard.commit_queue.push_back(*digest.as_bytes());
        Ok(())
    }

    fn metrics(&self) -> ConsensusMetrics {
        let guard = self.inner.lock().unwrap();
        ConsensusMetrics {
            current_view: guard.state.view,
            locked_qc: guard.state.locked_qc.is_some(),
            pending_qc: guard.state.pending_qc.is_some(),
            commit_queue_depth: guard.commit_queue.len(),
        }
    }

    fn pop_commit(&self) -> Option<Hash> {
        let mut guard = self.inner.lock().unwrap();
        guard.commit_queue.pop_front()
    }

    fn leader_for_view(&self, view: u64) -> Option<Validator> {
        let guard = self.inner.lock().unwrap();
        guard.leader_for_view(view)
    }

    fn current_view(&self) -> u64 {
        let guard = self.inner.lock().unwrap();
        guard.state.view
    }
}

fn verify_proposal(proposal: &SignedProposal, block_id: Hash) -> anyhow::Result<()> {
    let proposer_addr = address_from_pubkey(&proposal.public_key);
    if proposer_addr != proposal.block.header.proposer_id {
        anyhow::bail!("proposal proposer_id does not match pubkey");
    }
    verify_signature_bytes(&proposal.public_key, &proposal.signature, &block_id)?;
    Ok(())
}

fn vote_signing_bytes(block_id: &Hash, view: u64) -> anyhow::Result<Vec<u8>> {
    Ok(bincode::serialize(&(block_id, view))?)
}

fn verify_vote(vote: &SignedVote, validators: &[Validator]) -> anyhow::Result<()> {
    let expected = validators
        .iter()
        .find(|v| v.id == vote.voter.id)
        .ok_or_else(|| anyhow::anyhow!("voter not in validator set"))?;
    if expected.pubkey != vote.voter.pubkey {
        anyhow::bail!("voter pubkey mismatch");
    }
    let msg = vote_signing_bytes(&vote.block_id, vote.view)?;
    verify_signature_bytes(&vote.voter.pubkey, &vote.signature, &msg)?;
    Ok(())
}

pub fn sign_vote(block_id: &Hash, view: u64, signing_key: &ed25519_dalek::SigningKey) -> Vec<u8> {
    let bytes = bincode::serialize(&(block_id, view)).unwrap_or_default();
    sign_bytes(signing_key, &bytes)
}

pub fn sign_proposal(block: &Block, signing_key: &ed25519_dalek::SigningKey) -> Vec<u8> {
    let block_id = hash_block(block);
    sign_bytes(signing_key, block_id.as_slice())
}

