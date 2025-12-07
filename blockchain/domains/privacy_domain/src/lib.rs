use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyDomainConfig {
    pub chain_id: String,
    pub circuits: Vec<String>,
    pub privacy_level: String,
}

pub fn allowed_operation(op: &str) -> bool {
    matches!(op, "deposit" | "withdraw")
}

