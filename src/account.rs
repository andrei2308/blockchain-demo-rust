use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use ethereum_types::{Address, U256, H256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub balance: U256,
    pub nonce: u64,
    pub code: Vec<u8>,
    pub code_hash: H256,
    pub storage: HashMap<U256, U256>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldState {
    pub accounts: HashMap<Address, Account>,
    pub state_root: H256,
}