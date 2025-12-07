use runtime::Tx;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmDomainConfig {
    pub chain_id: String,
    pub security_model: String,
    pub da_mode: String,
    pub sequencer_binding: Option<String>,
    pub token_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossDomainPacket {
    pub src_domain: String,
    pub dst_domain: String,
    pub sequence: u64,
    pub payload: serde_json::Value,
    pub timeout_height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightClientHeader {
    pub state_root: [u8; 32],
    pub validator_set_hash: [u8; 32],
    pub height: u64,
}

pub fn validate_batch(txs: &[Tx]) -> bool {
    // Placeholder: enforce EVM-specific rules
    !txs.is_empty()
}

pub fn verify_packet(packet: &CrossDomainPacket, header: &LightClientHeader) -> bool {
    // Simplified: ensure packet hasn't timed out and header height progresses.
    header.height <= packet.timeout_height && packet.sequence > 0
}

#[macro_export]
macro_rules! define_domain {
    (
        chain_id: $chain_id:expr,
        security_model: $security:expr,
        da_mode: $da:expr,
        sequencer_binding: $seq:expr,
        token_model: $token:expr
    ) => {
        $crate::EvmDomainConfig {
            chain_id: $chain_id.to_string(),
            security_model: $security.to_string(),
            da_mode: $da.to_string(),
            sequencer_binding: $seq.map(|s: &str| s.to_string()),
            token_model: $token.to_string(),
        }
    };
}

