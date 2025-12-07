use runtime::Tx;
use uuid::Uuid;

pub fn register_domain(_tx: &Tx) -> anyhow::Result<Uuid> {
    Ok(Uuid::nil())
}

pub fn update_domain(_tx: &Tx) -> anyhow::Result<()> {
    Ok(())
}

