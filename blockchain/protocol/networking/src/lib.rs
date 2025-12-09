use anyhow::Context;
use consensus::{SignedProposal, SignedVote};
use futures::StreamExt;
use libp2p::{
    gossipsub,
    gossipsub::{IdentTopic, MessageAuthenticity},
    identity, multiaddr::Protocol, Multiaddr, PeerId, SwarmBuilder, SwarmEvent,
};
use runtime::{Block, Tx};
use serde::{Deserialize, Serialize};
use state::Validator;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkEnvelope {
    Consensus(ConsensusMessage),
    Tx(Tx),
}

pub trait ConsensusNetwork: Send + Sync {
    fn broadcast(&self, msg: ConsensusMessage);
    fn broadcast_tx(&self, tx: &Tx);
}

#[derive(Default)]
pub struct NoopConsensusNetwork;

impl ConsensusNetwork for NoopConsensusNetwork {
    fn broadcast(&self, _msg: ConsensusMessage) {
        // no-op for single-node devnet or tests
    }

    fn broadcast_tx(&self, _tx: &Tx) {
        // no-op
    }
}

const CONSENSUS_TOPIC: &str = "kova/consensus/1.0";

#[derive(Clone)]
pub struct Libp2pConsensusNetwork {
    tx: mpsc::Sender<NetworkEnvelope>,
}

impl ConsensusNetwork for Libp2pConsensusNetwork {
    fn broadcast(&self, msg: ConsensusMessage) {
        let _ = self.tx.try_send(NetworkEnvelope::Consensus(msg));
    }

    fn broadcast_tx(&self, tx: &Tx) {
        let _ = self.tx.try_send(NetworkEnvelope::Tx(tx.clone()));
    }
}

pub async fn start_libp2p_consensus(
    keypair: identity::Keypair,
    listen_addr: Multiaddr,
    bootstrap: Vec<Multiaddr>,
) -> anyhow::Result<(
    Arc<Libp2pConsensusNetwork>,
    mpsc::Receiver<ConsensusMessage>,
    mpsc::Receiver<Tx>,
)> {
    let peer_id = PeerId::from(keypair.public());
    info!("libp2p peer id {}", peer_id);

    let transport = libp2p::quic::tokio::Transport::new(libp2p::quic::Config::new(&keypair));
    let mut gossipsub = gossipsub::Behaviour::new(
        MessageAuthenticity::Signed(keypair.clone()),
        gossipsub::ConfigBuilder::default()
            .validation_mode(gossipsub::ValidationMode::Strict)
            .mesh_n_low(4)
            .build()
            .context("building gossipsub config")?,
    )?;
    let topic = IdentTopic::new(CONSENSUS_TOPIC);
    gossipsub.subscribe(&topic)?;

    let mut swarm = SwarmBuilder::with_tokio_executor(transport, gossipsub, peer_id).build();
    swarm.listen_on(listen_addr)?;
    for addr in bootstrap {
        if swarm.dial(addr.clone()).is_ok() {
            info!("dialing bootstrap peer {}", addr);
        }
    }

    let (publish_tx, mut publish_rx) = mpsc::channel::<NetworkEnvelope>(256);
    let (consensus_tx, consensus_rx) = mpsc::channel::<ConsensusMessage>(256);
    let (tx_tx, tx_rx) = mpsc::channel::<Tx>(256);
    let network = Arc::new(Libp2pConsensusNetwork { tx: publish_tx.clone() });
    let topic_clone = topic.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                maybe_msg = publish_rx.recv() => {
                    if let Some(msg) = maybe_msg {
                        match serde_json::to_vec(&msg) {
                            Ok(bytes) => {
                                if let Err(err) = swarm.behaviour_mut().publish(topic_clone.clone(), bytes) {
                                    warn!("failed to publish consensus msg: {err}");
                                }
                            }
                            Err(err) => warn!("serialize consensus msg failed: {err}"),
                        }
                    } else {
                        break;
                    }
                }
                event = swarm.select_next_some() => {
                    match event {
                        SwarmEvent::Behaviour(gossipsub::Event::Message { message, .. }) => {
                            match serde_json::from_slice::<NetworkEnvelope>(&message.data) {
                                Ok(NetworkEnvelope::Consensus(msg)) => {
                                    if consensus_tx.send(msg).await.is_err() {
                                        warn!("inbound consensus channel closed");
                                    }
                                }
                                Ok(NetworkEnvelope::Tx(tx)) => {
                                    if tx_tx.send(tx).await.is_err() {
                                        warn!("inbound tx channel closed");
                                    }
                                }
                                Err(err) => warn!("failed to decode gossipsub msg: {err}"),
                            }
                        }
                        SwarmEvent::NewListenAddr { address, .. } => {
                            info!("listening on {address}");
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                            warn!("dial error {:?}: {error}", peer_id);
                        }
                        SwarmEvent::Dialing(peer_id) => {
                            debug!("dialing {:?}", peer_id);
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    Ok((network, consensus_rx, tx_rx))
}

pub fn parse_multiaddr_list(addrs: &str) -> Vec<Multiaddr> {
    addrs
        .split(',')
        .filter_map(|s| s.trim().parse::<Multiaddr>().ok())
        .map(|mut addr| {
            if !addr.iter().any(|p| matches!(p, Protocol::QuicV1)) {
                addr.push(Protocol::QuicV1);
            }
            addr
        })
        .collect()
}

