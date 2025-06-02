use ethereum_types::{Address, U256, H256};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

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

impl Transaction {
    pub fn new_transfer(from: Address, to: Address, value: U256, nonce: u64) -> Self {
        Transaction {
            from,
            to: Some(to),
            value,
            data: Vec::new(),
            gas_limit: 21000,
            gas_price: U256::from(20_000_000_000u64),
            nonce,
            hash: None,
            tx_type: TransactionType::Transfer,
        }
    }

    pub fn new_contract_deployment(from: Address, bytecode: Vec<u8>, value: U256, nonce: u64) -> Self {
        Transaction {
            from,
            to: None,
            value,
            data: bytecode,
            gas_limit: 2_000_000,
            gas_price: U256::from(20_000_000_000u64),
            nonce,
            hash: None,
            tx_type: TransactionType::ContractDeployment,
        }
    }

    pub fn new_contract_call(from: Address, to: Address, calldata: Vec<u8>, value: U256, nonce: u64) -> Self {
        Transaction {
            from,
            to: Some(to),
            value,
            data: calldata,
            gas_limit: 500_000,
            gas_price: U256::from(20_000_000_000u64),
            nonce,
            hash: None,
            tx_type: TransactionType::ContractCall,
        }
    }

    pub fn new_with_gas(
        from: Address,
        to: Option<Address>,
        value: U256,
        data: Vec<u8>,
        gas_limit: u64,
        gas_price: U256,
        nonce: u64,
        tx_type: TransactionType,
    ) -> Self {
        Transaction {
            from,
            to,
            value,
            data,
            gas_limit,
            gas_price,
            nonce,
            hash: None,
            tx_type,
        }
    }

    pub fn calculate_hash(&self) -> H256 {
        let mut hasher = Keccak256::new();
        hasher.update(self.from.as_bytes());

        if let Some(to) = self.to {
            hasher.update(to.as_bytes());
        } else {
            hasher.update(&[0u8; 20]);
        }

        let mut value_bytes = [0u8; 32];
        self.value.to_big_endian(&mut value_bytes);
        hasher.update(&value_bytes);

        hasher.update(&self.data);
        hasher.update(&self.gas_limit.to_be_bytes());

        let mut gas_price_bytes = [0u8; 32];
        self.gas_price.to_big_endian(&mut gas_price_bytes);
        hasher.update(&gas_price_bytes);

        hasher.update(&self.nonce.to_be_bytes());

        hasher.update(&[match self.tx_type {
            TransactionType::Transfer => 0,
            TransactionType::ContractDeployment => 1,
            TransactionType::ContractCall => 2,
        }]);

        H256::from_slice(&hasher.finalize())
    }

    pub fn set_hash(&mut self) {
        self.hash = Some(self.calculate_hash());
    }

    pub fn is_contract_deployment(&self) -> bool {
        matches!(self.tx_type, TransactionType::ContractDeployment)
    }

    pub fn is_contract_call(&self) -> bool {
        matches!(self.tx_type, TransactionType::ContractCall)
    }

    pub fn is_transfer(&self) -> bool {
        matches!(self.tx_type, TransactionType::Transfer)
    }

    pub fn estimated_gas_cost(&self) -> U256 {
        self.gas_price * U256::from(self.gas_limit)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.gas_limit == 0 {
            return Err("Gas limit cannot be zero".to_string());
        }

        if self.gas_price == U256::zero() {
            return Err("Gas price cannot be zero".to_string());
        }

        match self.tx_type {
            TransactionType::Transfer => {
                if self.to.is_none() {
                    return Err("Transfer must have a recipient".to_string());
                }
                if !self.data.is_empty() {
                    return Err("Transfer should not have data".to_string());
                }
            }
            TransactionType::ContractDeployment => {
                if self.to.is_some() {
                    return Err("Contract deployment should not have a recipient".to_string());
                }
                if self.data.is_empty() {
                    return Err("Contract deployment must have bytecode".to_string());
                }
            }
            TransactionType::ContractCall => {
                if self.to.is_none() {
                    return Err("Contract call must have a recipient".to_string());
                }
            }
        }

        Ok(())
    }

    pub fn summary(&self) -> String {
        match self.tx_type {
            TransactionType::Transfer => {
                format!("Transfer {} wei from {} to {}",
                        self.value,
                        self.from,
                        self.to.map_or("None".to_string(), |a| format!("{}", a))
                )
            }
            TransactionType::ContractDeployment => {
                format!("Deploy contract from {} with {} bytes of bytecode",
                        self.from,
                        self.data.len()
                )
            }
            TransactionType::ContractCall => {
                format!("Call contract {} from {} with {} bytes of data",
                        self.to.map_or("None".to_string(), |a| format!("{}", a)),
                        self.from,
                        self.data.len()
                )
            }
        }
    }

    pub fn size(&self) -> usize {
        20 + // from
            20 + // to (even if None, we count it)
            32 + // value
            self.data.len() + // data
            8 + // gas_limit
            32 + // gas_price
            8 + // nonce
            32 + // hash
            1 // tx_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_transaction() {
        let from = Address::from([1u8; 20]);
        let to = Address::from([2u8; 20]);
        let mut tx = Transaction::new_transfer(from, to, U256::from(1000), 0);

        assert!(tx.is_transfer());
        assert!(!tx.is_contract_deployment());
        assert!(!tx.is_contract_call());
        assert_eq!(tx.value, U256::from(1000));
        assert_eq!(tx.nonce, 0);
        assert!(tx.validate().is_ok());

        tx.set_hash();
        assert!(tx.hash.is_some());
    }

    #[test]
    fn test_contract_deployment() {
        let from = Address::from([1u8; 20]);
        let bytecode = vec![0x60, 0x80, 0x60, 0x40];
        let mut tx = Transaction::new_contract_deployment(from, bytecode.clone(), U256::zero(), 0);

        assert!(tx.is_contract_deployment());
        assert!(!tx.is_transfer());
        assert!(!tx.is_contract_call());
        assert_eq!(tx.data, bytecode);
        assert!(tx.to.is_none());
        assert!(tx.validate().is_ok());

        tx.set_hash();
        assert!(tx.hash.is_some());
    }

    #[test]
    fn test_contract_call() {
        let from = Address::from([1u8; 20]);
        let to = Address::from([2u8; 20]);
        let calldata = vec![0xa9, 0x05, 0x9c, 0xbb]; // transfer function selector
        let mut tx = Transaction::new_contract_call(from, to, calldata.clone(), U256::zero(), 0);

        assert!(tx.is_contract_call());
        assert!(!tx.is_transfer());
        assert!(!tx.is_contract_deployment());
        assert_eq!(tx.data, calldata);
        assert_eq!(tx.to, Some(to));
        assert!(tx.validate().is_ok());

        tx.set_hash();
        assert!(tx.hash.is_some());
    }

    #[test]
    fn test_hash_consistency() {
        let from = Address::from([1u8; 20]);
        let to = Address::from([2u8; 20]);
        let tx1 = Transaction::new_transfer(from, to, U256::from(1000), 0);
        let tx2 = Transaction::new_transfer(from, to, U256::from(1000), 0);

        assert_eq!(tx1.calculate_hash(), tx2.calculate_hash());
    }

    #[test]
    fn test_validation_errors() {
        let from = Address::from([1u8; 20]);

        let invalid_transfer = Transaction {
            from,
            to: None,
            value: U256::from(1000),
            data: Vec::new(),
            gas_limit: 21000,
            gas_price: U256::from(20_000_000_000u64),
            nonce: 0,
            hash: None,
            tx_type: TransactionType::Transfer,
        };
        assert!(invalid_transfer.validate().is_err());

        let invalid_deployment = Transaction {
            from,
            to: None,
            value: U256::zero(),
            data: Vec::new(),
            gas_limit: 2_000_000,
            gas_price: U256::from(20_000_000_000u64),
            nonce: 0,
            hash: None,
            tx_type: TransactionType::ContractDeployment,
        };
        assert!(invalid_deployment.validate().is_err());
    }

    #[test]
    fn test_gas_cost_calculation() {
        let from = Address::from([1u8; 20]);
        let to = Address::from([2u8; 20]);
        let tx = Transaction::new_transfer(from, to, U256::from(1000), 0);

        let expected_cost = U256::from(20_000_000_000u64) * U256::from(21000);
        assert_eq!(tx.estimated_gas_cost(), expected_cost);
    }
}