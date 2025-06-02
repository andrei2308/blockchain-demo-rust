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

impl Block {
    pub fn new(
        number: u64,
        parent_hash: H256,
        transactions: Vec<Transaction>,
    ) -> Self {
        Block {
            number,
            hash: None,
            parent_hash,
            transactions,
            timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            gas_limit: 30_000_000,
            gas_used: 0,
            nonce: 0
        }
    }

    pub fn calculate_hash(&self) -> H256 {
        use sha3::{Digest, Keccak256};

        let mut hasher = Keccak256::new();
        hasher.update(&self.number.to_be_bytes());
        hasher.update(self.parent_hash.as_bytes());
        hasher.update(&self.timestamp.to_be_bytes());
        hasher.update(&self.nonce.to_be_bytes());
        hasher.update(&self.gas_limit.to_be_bytes());
        hasher.update(&self.gas_used.to_be_bytes());

        for tx in &self.transactions {
            if let Some(tx_hash) = tx.hash {
                hasher.update(tx_hash.as_bytes());
            }
        }

        H256::from_slice(&hasher.finalize())
    }

    pub fn set_hash(&mut self) {
        self.hash = Some(self.calculate_hash());
    }

    // mining logic

    pub fn mine(&mut self, difficulty: usize) -> u64 {
        let target = "0".repeat(difficulty);
        let mut attempts = 0;

        println!("Mining block {} with difficulty {}...", self.number, difficulty);
        let start_time = std::time::Instant::now();

        loop {
            let hash = self.calculate_hash();
            let hash_str = format!("{:x}", hash);

            attempts += 1;

            if attempts % 100_000 == 0 {
                println!("Attempt {}: {}", attempts, hash_str);
            }

            if hash_str.starts_with(&target) {
                self.hash = Some(hash);
                let duration = start_time.elapsed();
                println!("Block mined! Nonce: {}, Hash: {}, Time: {:?}, Attempts: {}",
                         self.nonce, hash_str, duration, attempts);
                return attempts;
            }

            self.nonce += 1;

            if attempts > 10_000_000 {
                panic!("Mining took too long! Try lower difficulty.");
            }
        }
    }

    pub fn is_valid_proof(&self, difficulty: usize) -> bool {
        if let Some(hash) = self.hash {
            let hash_str = format!("{:x}", hash);
            let target = "0".repeat(difficulty);
            hash_str.starts_with(&target)
        } else {
            false
        }
    }

    pub fn genesis() -> Self {
        let mut genesis = Block::new(
            0,
            H256::zero(),
            Vec::new(),
        );

        println!("Mining genesis block...");
        genesis.mine(2);
        genesis
    }

    pub fn validate_gas_usage(&self) -> Result<(), String> {
        if self.gas_used > self.gas_limit {
            return Err(format!(
                "Block gas used ({}) exceeds limit ({})",
                self.gas_used,
                self.gas_limit
            ));
        }
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::Transaction;
    use ethereum_types::Address;

    #[test]
    fn test_genesis_block() {
        let genesis = Block::genesis();

        assert_eq!(genesis.number, 0);
        assert_eq!(genesis.parent_hash, H256::zero());
        assert!(genesis.transactions.is_empty());
        assert!(genesis.hash.is_some());

        println!("Genesis block: {:?}", genesis);
    }

    #[test]
    fn test_block_with_transactions() {
        let from = Address::from([1u8; 20]);
        let to = Address::from([2u8; 20]);
        let genesis = Block::genesis();

        let mut tx = Transaction::new_transfer(from, to, U256::from(1000), 1);
        tx.set_hash();

        let mut block = Block::new(
            1,
            genesis.hash.unwrap(),
            vec![tx],
        );

        block.set_hash();

        assert_eq!(block.number, 1);
        assert_eq!(block.transactions.len(), 1);
        assert!(block.hash.is_some());

        println!("Block with transaction: {:?}", block);
    }
}