use runtime::Block;
use tracing::info;

#[derive(Default)]
pub struct InMemorySink {
    pub blocks: Vec<Block>,
}

impl InMemorySink {
    pub fn ingest_block(&mut self, block: Block) {
        info!("ingesting block {}", block.header.height);
        self.blocks.push(block);
    }
}

