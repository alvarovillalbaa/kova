use anyhow::Context;
use revm::primitives::{keccak256, B160};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{DomainCall, DomainExecutionReceipt, DomainState, DomainVm, DomainVmCtx};
use state::DomainType;

#[derive(Clone)]
pub struct EvmAdapter {
    domain_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvmCall {
    from: Option<String>,
    to: Option<String>,
    input: Option<String>,
    value: Option<String>,
}

impl EvmAdapter {
    pub fn new(domain_id: Uuid) -> Self {
        Self { domain_id }
    }

    fn parse_addr(s: &str) -> anyhow::Result<B160> {
        let clean = s.trim_start_matches("0x");
        let bytes = hex::decode(clean)?;
        let mut arr = [0u8; 20];
        for (i, b) in bytes.iter().take(20).enumerate() {
            arr[i] = *b;
        }
        Ok(B160::from(arr))
    }
}

#[async_trait::async_trait]
impl DomainVm for EvmAdapter {
    fn kind(&self) -> DomainType {
        DomainType::EvmSharedSecurity
    }

    async fn execute(
        &self,
        call: &DomainCall,
        ctx: DomainVmCtx<'_>,
    ) -> anyhow::Result<DomainExecutionReceipt> {
        let parsed: EvmCall =
            serde_json::from_value(call.payload.clone()).context("invalid evm call payload")?;
        let from = parsed
            .from
            .as_deref()
            .and_then(|s| Self::parse_addr(s).ok())
            .unwrap_or_else(|| B160::from_low_u64_be(0));
        let to = parsed
            .to
            .as_deref()
            .and_then(|s| Self::parse_addr(s).ok());
        let input_bytes = parsed
            .input
            .as_deref()
            .map(|s| hex::decode(s.trim_start_matches("0x")))
            .transpose()?
            .unwrap_or_default();
        let value = parsed
            .value
            .as_deref()
            .unwrap_or("0")
            .parse::<u128>()
            .unwrap_or(0);
        let gas_used = call.max_gas.unwrap_or(5_000_000);

        let mut state = ctx.state.clone();
        let mut trace = serde_json::json!({
            "from": format!("{from:?}"),
            "to": to.map(|addr| format!("{addr:?}")),
            "value": value,
            "input_len": input_bytes.len(),
            "block_height": ctx.block_height,
        });
        let mut seed = Vec::new();
        seed.extend(input_bytes);
        seed.extend_from_slice(&value.to_le_bytes());
        seed.extend_from_slice(&ctx.block_height.to_le_bytes());
        let root = keccak256(seed);
        state
            .kv
            .insert("evm:last_root".into(), root.0.to_vec());
        state
            .kv
            .insert("evm:last_from".into(), from.as_bytes().to_vec());
        if let Some(to_addr) = to {
            state
                .kv
                .insert("evm:last_to".into(), to_addr.as_bytes().to_vec());
        }
        if let Some(obj) = trace.as_object_mut() {
            obj.insert("domain_id".into(), serde_json::json!(self.domain_id));
        }

        Ok(DomainExecutionReceipt {
            domain_id: self.domain_id,
            state_root: [0u8; 32],
            gas_used,
            events: vec!["evm_call".into()],
            proof: None,
            trace,
            state,
        })
    }
}
