use ethereum_types::{Address, H256, U256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TransactionType {
    Transfer,
    ContractDeployment,
    ContractCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub from: Address,
    pub to: Option<Address>,
    pub value: U256,
    pub data: Vec<u8>,
    pub gas_limit: u64,
    pub gas_price: U256,
    pub nonce: u64,
    pub hash: Option<H256>,
    pub tx_type: TransactionType,
}