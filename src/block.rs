use crate::transaction::Transaction;
use ethereum_types::{H256,U256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Block {
    pub number: u64,
    pub hash: Option<H256>,
    pub parent_hash: H256,
    pub transactions: Vec<Transaction>,
    pub timestamp: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub nonce: u64
}