use runtime::{Tx, TxPayload};

pub fn apply_l1_tx(tx: &Tx) -> anyhow::Result<()> {
    match &tx.payload {
        TxPayload::Stake { .. } => staking::stake(tx).map(|_| ()),
        TxPayload::Unstake { .. } => staking::unstake(tx),
        TxPayload::Delegate { .. } | TxPayload::Undelegate { .. } => Ok(()), // TODO: delegation
        TxPayload::DomainCreate { .. } => {
            domains_registry::register_domain(tx).map(|_| ())
        }
        TxPayload::DomainConfigUpdate { .. } => domains_registry::update_domain(tx),
        TxPayload::RollupBridgeDeposit { .. } | TxPayload::RollupBridgeWithdraw { .. } => {
            rollup_bridge::handle(tx)
        }
        TxPayload::GovernanceProposal { .. }
        | TxPayload::GovernanceVote { .. }
        | TxPayload::GovernanceBridgeApprove { .. }
        | TxPayload::GovernanceExecute { .. } => governance::handle(tx),
        TxPayload::PrivacyDeposit { .. } => privacy_pools::deposit(tx),
        TxPayload::PrivacyWithdraw { .. } => privacy_pools::withdraw(tx),
        _ => Ok(()),
    }
}

