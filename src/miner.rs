use crate::blockchain::Blockchain;
use crate::block::Block;
use crate::transaction::{Transaction, TransactionType};
use ethereum_types::Address;

pub struct Miner {
    pub miner_address: Address,
    pub block_reward: u64,
}

impl Miner {
    pub fn new(miner_address: Address) -> Self {
        Miner {
            miner_address,
            block_reward: 5000,
        }
    }

    pub fn mine_block(
        &self,
        blockchain: &mut Blockchain,
        transactions: Vec<Transaction>,
        difficulty: usize
    ) -> Result<Block, String> {
        println!("\nMiner {} starting to mine block...", self.miner_address);

        let mut all_transactions = vec![self.create_coinbase_transaction(blockchain)];

        all_transactions.extend(transactions);

        let latest = blockchain.get_latest_block();
        let mut block = Block::new(
            latest.number + 1,
            latest.hash.unwrap(),
            all_transactions,
        );

        let attempts = block.mine(difficulty);

        blockchain.add_block(block.clone())?;

        println!("Block reward: {} wei paid to {}", self.block_reward, self.miner_address);
        println!("Mining stats: {} attempts for difficulty {}", attempts, difficulty);

        Ok(block)
    }

    fn create_coinbase_transaction(&self, blockchain: &Blockchain) -> Transaction {
        use crate::transaction::Transaction;
        use ethereum_types::U256;

        let mut coinbase = Transaction {
            from: Address::zero(),
            to: Some(self.miner_address),
            value: U256::from(self.block_reward),
            data: b"Block reward".to_vec(),
            gas_limit: 0,
            gas_price: U256::zero(),
            nonce: 0,
            hash: None,
            tx_type: TransactionType::Transfer
        };

        coinbase.set_hash();
        coinbase
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::Blockchain;
    use ethereum_types::U256;

    #[test]
    fn test_mining() {
        let mut blockchain = Blockchain::new();

        let miner_address = Address::from([99u8; 20]);
        let miner = Miner::new(miner_address);

        let result = miner.mine_block(&mut blockchain, vec![], 1);
        assert!(result.is_ok());

        let balance = blockchain.state.get_balance(&miner_address);
        assert_eq!(balance, U256::from(5000));
    }
}