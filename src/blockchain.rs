use crate::block::Block;
use crate::account::WorldState;

#[derive(Debug, Clone)]
pub struct Blockchain {
    pub blocks: Vec<Block>,
    pub state: WorldState,
    pub chain_id: u64,
}