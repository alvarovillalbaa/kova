use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use zk_core::{
    blake3_commit, stub_proof, Commitments, ProgramDescriptor, ProgramId, ProgramRegistry, ProofArtifact,
    ProofRequest, ZkBackend, ZkError, ZkResult,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sp1Program {
    pub id: ProgramId,
    pub elf: Vec<u8>,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sp1Config {
    pub programs: Vec<Sp1Program>,
    pub verify_only: bool,
}

impl Default for Sp1Config {
    fn default() -> Self {
        Self {
            programs: vec![],
            verify_only: false,
        }
    }
}

pub struct Sp1Backend {
    programs: HashMap<ProgramId, Sp1Program>,
    registry: ProgramRegistry,
    verify_only: bool,
}

impl Sp1Backend {
    pub fn new(cfg: Sp1Config) -> Self {
        let mut registry = ProgramRegistry::new();
        let mut programs = HashMap::new();
        for p in &cfg.programs {
            registry.register(ProgramDescriptor {
                id: p.id.clone(),
                name: p.name.clone(),
                description: "SP1 program".into(),
                version: p.version.clone(),
            });
            programs.insert(p.id.clone(), p.clone());
        }
        Self {
            programs,
            registry,
            verify_only: cfg.verify_only,
        }
    }

    fn ensure_program(&self, id: &ProgramId) -> ZkResult<&Sp1Program> {
        self.programs
            .get(id)
            .ok_or_else(|| ZkError::UnknownProgram(id.clone()))
    }
}

#[async_trait]
impl ZkBackend for Sp1Backend {
    fn backend_id(&self) -> &'static str {
        "sp1"
    }

    fn registry(&self) -> &ProgramRegistry {
        &self.registry
    }

    async fn prove(&self, request: ProofRequest) -> ZkResult<ProofArtifact> {
        let program = self.ensure_program(&request.program_id)?;
        if self.verify_only {
            return Err(ZkError::BackendUnavailable(
                "backend configured as verify-only".into(),
            ));
        }
        if program.elf.is_empty() {
            return Err(ZkError::BackendUnavailable(
                "SP1 ELF not provided; set program elf bytes".into(),
            ));
        }

        #[cfg(feature = "sp1")]
        {
            use sp1_sdk::{ProverClient, SP1ProofWithPublicValues, SP1Stdin};

            let mut stdin = SP1Stdin::new();
            stdin.write_slice(&request.witness);

            let client = ProverClient::new();
            let (pk, vk) = client
                .setup(program.elf.clone())
                .map_err(|e| ZkError::Other(format!("setup failed: {e}")))?;
            let SP1ProofWithPublicValues { proof, public_values } = client
                .prove(&pk, stdin)
                .map_err(|e| ZkError::Other(format!("prove failed: {e}")))?;

            let public_outputs = public_values.as_bytes().to_vec();
            Ok(ProofArtifact {
                backend: self.backend_id().into(),
                program_id: request.program_id,
                proof: bincode::serialize(&proof)
                    .map_err(|e| ZkError::Other(format!("serialize proof: {e}")))?,
                public_outputs,
                commitments: request.commitments,
                verification_key: Some(vk.bytes().to_vec()),
            })
        }
        #[cfg(not(feature = "sp1"))]
        {
            Ok(stub_proof(
                request.program_id,
                request.witness,
                request.commitments,
            ))
        }
    }

    async fn verify(&self, artifact: &ProofArtifact) -> ZkResult<()> {
        #[cfg(feature = "sp1")]
        {
            use sp1_sdk::{SP1Proof, SP1PublicValues, SP1VerifyingKey};

            let program = self.ensure_program(&artifact.program_id)?;
            let vk_bytes = artifact
                .verification_key
                .clone()
                .ok_or_else(|| ZkError::ProofRejected("missing verification key".into()))?;
            let vk = SP1VerifyingKey::from_bytes(&vk_bytes)
                .map_err(|e| ZkError::ProofRejected(format!("vk parse error: {e}")))?;
            let proof: SP1Proof = bincode::deserialize(&artifact.proof)
                .map_err(|e| ZkError::ProofRejected(format!("proof decode error: {e}")))?;
            let public = SP1PublicValues::from_bytes(&artifact.public_outputs)
                .map_err(|e| ZkError::ProofRejected(format!("public decode error: {e}")))?;
            sp1_sdk::verify(&proof, &vk, &public, program.elf.clone())
                .map_err(|e| ZkError::ProofRejected(format!("verify failed: {e}")))
        }
        #[cfg(not(feature = "sp1"))]
        {
            let expected = blake3_commit(&artifact.public_outputs);
            if artifact.proof != expected {
                return Err(ZkError::ProofRejected(
                    "stub hash mismatch for proof".into(),
                ));
            }
            Ok(())
        }
    }
}
