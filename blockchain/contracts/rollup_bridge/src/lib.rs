use runtime::Tx;

pub fn deposit(_tx: &Tx) -> anyhow::Result<()> {
    Ok(())
}

pub fn withdraw(_tx: &Tx) -> anyhow::Result<()> {
    Ok(())
}

pub fn handle(tx: &Tx) -> anyhow::Result<()> {
    match &tx.payload {
        runtime::TxPayload::RollupBridgeDeposit { .. } => deposit(tx),
        runtime::TxPayload::RollupBridgeWithdraw { .. } => withdraw(tx),
        _ => Ok(()),
    }
}

