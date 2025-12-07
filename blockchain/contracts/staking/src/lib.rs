use runtime::Tx;
use uuid::Uuid;

pub fn stake(_tx: &Tx) -> anyhow::Result<Uuid> {
    Ok(Uuid::nil())
}

pub fn unstake(_tx: &Tx) -> anyhow::Result<()> {
    Ok(())
}

