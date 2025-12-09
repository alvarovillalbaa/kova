use anyhow::Context;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use wasmtime::{Config, Engine as WasmEngine, Module, Store};

use super::{DomainCall, DomainExecutionReceipt, DomainState, DomainVm, DomainVmCtx};
use state::DomainType;

#[derive(Clone)]
pub struct WasmAdapter {
    domain_id: Uuid,
    engine: WasmEngine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum WasmAction {
    Deploy { module_id: String, code_b64: String },
    Invoke { module_id: String, entry: Option<String> },
}

impl WasmAdapter {
    pub fn new(domain_id: Uuid) -> Self {
        let mut cfg = Config::new();
        cfg.consume_fuel(true);
        Self {
            domain_id,
            engine: WasmEngine::new(&cfg).unwrap_or_else(|_| WasmEngine::default()),
        }
    }
}

#[async_trait::async_trait]
impl DomainVm for WasmAdapter {
    fn kind(&self) -> DomainType {
        DomainType::Wasm
    }

    async fn execute(
        &self,
        call: &DomainCall,
        ctx: DomainVmCtx<'_>,
    ) -> anyhow::Result<DomainExecutionReceipt> {
        let action: WasmAction =
            serde_json::from_value(call.payload.clone()).context("invalid wasm call payload")?;
        let mut state = ctx.state.clone();
        let mut events = vec![];
        let mut gas_used = call.max_gas.unwrap_or(3_000_000);

        match action {
            WasmAction::Deploy { module_id, code_b64 } => {
                let bytes = BASE64
                    .decode(code_b64.as_bytes())
                    .context("invalid base64 wasm module")?;
                // Ensure module is valid.
                let _ = Module::new(&self.engine, &bytes)
                    .context("failed to compile wasm module for domain")?;
                state.kv.insert(format!("wasm:{module_id}"), bytes);
                events.push(format!("wasm_deploy:{module_id}"));
            }
            WasmAction::Invoke { module_id, entry } => {
                if let Some(code) = state.kv.get(&format!("wasm:{module_id}")) {
                    let module =
                        Module::new(&self.engine, code).context("wasm module failed to load")?;
                    let mut store = Store::new(&self.engine, ());
                    let fuel = call.max_gas.unwrap_or(3_000_000) as u64;
                    let _ = store.add_fuel(fuel);
                    let instance =
                        wasmtime::Instance::new(&mut store, &module, &[]).context("instantiation failed")?;
                    if let Some(func_name) = entry {
                        if let Some(func) = instance.get_typed_func::<(), ()>(&mut store, &func_name).ok() {
                            let _ = func.call(&mut store, ());
                        }
                    }
                    let consumed = store.fuel_consumed().unwrap_or(fuel);
                    gas_used = consumed as u64;
                    state.kv.insert(
                        format!("wasm:consumed:{module_id}"),
                        consumed.to_le_bytes().to_vec(),
                    );
                    events.push(format!("wasm_invoke:{module_id}"));
                } else {
                    anyhow::bail!("missing wasm module {module_id}");
                }
            }
        }

        Ok(DomainExecutionReceipt {
            domain_id: self.domain_id,
            state_root: [0u8; 32],
            gas_used,
            events,
            proof: None,
            trace: serde_json::json!({ "domain_id": self.domain_id, "block_height": ctx.block_height }),
            state,
        })
    }
}
