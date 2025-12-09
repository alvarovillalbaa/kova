use runtime::Tx;
use uuid::Uuid;

pub fn submit_proposal(_tx: &Tx) -> anyhow::Result<Uuid> {
    Ok(Uuid::nil())
}

pub fn vote(_tx: &Tx) -> anyhow::Result<()> {
    Ok(())
}

pub fn handle(tx: &Tx) -> anyhow::Result<()> {
    match tx.payload {
        runtime::TxPayload::GovernanceProposal { .. } => {
            submit_proposal(tx).map(|_| ())
        }
        runtime::TxPayload::GovernanceVote { .. }
        | runtime::TxPayload::GovernanceBridgeApprove { .. }
        | runtime::TxPayload::GovernanceExecute { .. } => vote(tx),
        _ => Ok(()),
    }
}

