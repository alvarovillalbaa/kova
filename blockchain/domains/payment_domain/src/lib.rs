use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentDomainConfig {
    pub chain_id: String,
    pub settlement_interval_blocks: u64,
    pub da_mode: String,
}

pub fn channel_limits() -> (u64, u64) {
    (1, 10_000)
}

