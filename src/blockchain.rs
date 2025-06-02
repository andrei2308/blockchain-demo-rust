use crate::block::Block;
use crate::transaction::{Transaction, TransactionType};
use crate::account::WorldState;
use crate::evm::{RevmExecutor, ContractExecutionResult, ContractUtils};
use ethereum_types::{H256, Address, U256};

#[derive(Debug, Clone)]
pub struct Blockchain {
    pub blocks: Vec<Block>,
    pub state: WorldState,
    pub chain_id: u64,
}

impl Blockchain {
    pub fn new() -> Self {
        let genesis = Block::genesis();
        println!("Creating blockchain with genesis: {:?}", genesis.hash);

        Blockchain {
            blocks: vec![genesis],
            state: WorldState::new(),
            chain_id: 1337, // Custom chain ID
        }
    }

    pub fn new_with_chain_id(chain_id: u64) -> Self {
        let mut blockchain = Self::new();
        blockchain.chain_id = chain_id;
        blockchain
    }

    pub fn get_latest_block(&self) -> &Block {
        self.blocks.last().unwrap()
    }

    pub fn get_block_count(&self) -> usize {
        self.blocks.len()
    }

    pub fn get_block_by_number(&self, number: u64) -> Option<&Block> {
        self.blocks.get(number as usize)
    }

    pub fn get_block_by_hash(&self, hash: H256) -> Option<&Block> {
        self.blocks.iter().find(|block| block.hash == Some(hash))
    }

    pub fn add_block(&mut self, mut block: Block) -> Result<(), String> {
        let expected_number = self.get_latest_block().number + 1;
        if block.number != expected_number {
            return Err(format!("Invalid block number. Expected {}, got {}", expected_number, block.number));
        }

        let latest_hash = self.get_latest_block().hash.unwrap();
        if block.parent_hash != latest_hash {
            return Err("Invalid parent hash".to_string());
        }

        if block.hash.is_some() && !block.is_valid_proof(1) {
            return Err("Invalid proof of work".to_string());
        }

        let mut total_gas_used = 0u64;
        for tx in &block.transactions {
            if let Some(result) = self.execute_transaction(tx)? {
                total_gas_used += result.gas_used;
            } else {
                total_gas_used += 21000;
            }
        }

        block.gas_used = total_gas_used;
        if block.hash.is_none() {
            block.set_hash();
        }

        println!("⛓Added block {} with hash {:?}", block.number, block.hash);
        self.blocks.push(block);

        Ok(())
    }

    fn execute_transaction(&mut self, tx: &Transaction) -> Result<Option<ContractExecutionResult>, String> {
        if tx.from == Address::zero() {
            if let Some(to) = tx.to {
                let account = self.state.get_account_mut(&to);
                account.balance += tx.value;
                println!("💰 Minted {} wei for miner {}", tx.value, to);
                return Ok(None);
            }
        }

        tx.validate()?;

        if tx.is_contract_deployment() || tx.is_contract_call() {
            return self.execute_with_revm(tx);
        }

        let expected_nonce = self.state.get_nonce(&tx.from);
        if tx.nonce != expected_nonce {
            return Err(format!("Invalid nonce. Expected {}, got {}", expected_nonce, tx.nonce));
        }

        let total_cost = tx.value + tx.estimated_gas_cost();
        if self.state.get_balance(&tx.from) < total_cost {
            return Err("Insufficient balance for transaction and gas".to_string());
        }

        if let Some(to) = tx.to {
            self.state.transfer(&tx.from, &to, tx.value)?;

            let sender = self.state.get_account_mut(&tx.from);
            sender.nonce += 1;
            sender.balance -= tx.estimated_gas_cost(); // Deduct gas cost

            println!("💸 Transfer: {} -> {} ({} wei)", tx.from, to, tx.value);
        }

        Ok(None)
    }

    fn execute_with_revm(&mut self, tx: &Transaction) -> Result<Option<ContractExecutionResult>, String> {
        let latest_block = self.get_latest_block();
        let mut revm = RevmExecutor::new(
            latest_block.number + 1,
            latest_block.timestamp,
            Address::from([0u8; 20]), // Coinbase address
            50_000_000, // 50M gas limit per block
        );

        revm.load_state_from_world(&self.state)?;

        let result = revm.execute_transaction(
            tx.from,
            tx.to,
            tx.value,
            tx.data.clone(),
            tx.gas_limit,
            tx.gas_price,
            tx.nonce,
        )?;

        revm.save_state_to_world(&mut self.state)?;

        let sender_account = self.state.get_account_mut(&tx.from);
        sender_account.nonce += 1;

        if result.success {
            match tx.tx_type {
                TransactionType::ContractDeployment => {
                    if let Some(addr) = result.contract_address {
                        println!("Contract deployed at: {}", addr);

                        let contract_account = self.state.get_account_mut(&addr);
                        if !result.return_data.is_empty() {
                            contract_account.set_code(result.return_data.clone());
                        }
                    }
                }
                TransactionType::ContractCall => {
                    println!("Contract call executed successfully");
                    if !result.return_data.is_empty() {
                        println!("Return data: {} bytes", result.return_data.len());
                    }
                }
                _ => {}
            }
        } else {
            println!("REVM transaction failed: {}", result.reason);
            if let Some(error) = &result.error {
                println!("Error details: {}", error);
            }
        }

        Ok(Some(result))
    }

    pub fn deploy_contract_with_revm(
        &mut self,
        deployer: Address,
        bytecode: Vec<u8>,
        constructor_args: Vec<u8>,
        value: U256,
        gas_limit: u64,
    ) -> Result<(Address, ContractExecutionResult), String> {
        let nonce = self.state.get_nonce(&deployer);

        let contract_address = ContractUtils::calculate_create_address(&deployer, nonce);

        let mut deployment_data = bytecode;
        deployment_data.extend_from_slice(&constructor_args);

        let mut tx = Transaction::new_contract_deployment(deployer, deployment_data, value, nonce);
        tx.set_hash();

        if let Some(result) = self.execute_transaction(&tx)? {
            if result.success {
                return Ok((contract_address, result));
            } else {
                return Err(format!("Contract deployment failed: {}", result.reason));
            }
        }

        Err("Failed to execute deployment transaction".to_string())
    }

    pub fn call_contract_with_revm(
        &mut self,
        caller: Address,
        contract: Address,
        calldata: Vec<u8>,
        value: U256,
        gas_limit: u64,
    ) -> Result<ContractExecutionResult, String> {
        let nonce = self.state.get_nonce(&caller);

        let mut tx = Transaction::new_contract_call(caller, contract, calldata, value, nonce);
        tx.set_hash();

        if let Some(result) = self.execute_transaction(&tx)? {
            return Ok(result);
        }

        Err("Failed to execute contract call".to_string())
    }

    pub fn view_contract_call(
        &self,
        caller: Address,
        contract: Address,
        calldata: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let latest_block = self.get_latest_block();
        let mut revm = RevmExecutor::new(
            latest_block.number + 1,
            latest_block.timestamp,
            Address::from([0u8; 20]),
            50_000_000,
        );

        revm.load_state_from_world(&self.state)?;

        let return_data = revm.view_call(caller, contract, calldata)?;
        Ok(return_data)
    }

    pub fn validate_chain(&self) -> Result<(), String> {
        if self.blocks.is_empty() {
            return Err("Empty blockchain".to_string());
        }

        let genesis = &self.blocks[0];
        if genesis.number != 0 || genesis.parent_hash != H256::zero() {
            return Err("Invalid genesis block".to_string());
        }

        for i in 1..self.blocks.len() {
            let current = &self.blocks[i];
            let previous = &self.blocks[i - 1];

            if current.number != previous.number + 1 {
                return Err(format!("Invalid block number at position {}", i));
            }

            if current.parent_hash != previous.hash.unwrap() {
                return Err(format!("Invalid parent hash at block {}", current.number));
            }

            let difficulty = if current.number <= 2 { 2 } else { 3 };
            if !current.is_valid_proof(difficulty) {
                return Err(format!("Invalid proof of work at block {}", current.number));
            }
        }

        println!("Blockchain validation successful! {} blocks validated.", self.blocks.len());
        Ok(())
    }

    pub fn get_total_supply(&self) -> u64 {
        let mut total = 0;
        for block in &self.blocks {
            if block.number > 0 && !block.transactions.is_empty() {
                let coinbase = &block.transactions[0];
                if coinbase.from == Address::zero() {
                    total += coinbase.value.as_u64();
                }
            }
        }
        total
    }

    pub fn get_transaction_count(&self, address: &Address) -> u64 {
        self.state.get_nonce(address)
    }

    pub fn get_transactions_for_address(&self, address: &Address) -> Vec<&Transaction> {
        let mut transactions = Vec::new();
        for block in &self.blocks {
            for tx in &block.transactions {
                if tx.from == *address || tx.to == Some(*address) {
                    transactions.push(tx);
                }
            }
        }
        transactions
    }

    pub fn print_chain_info(&self) {
        println!("\n=== BLOCKCHAIN INFO ===");
        println!("Chain ID: {}", self.chain_id);
        println!("Total blocks: {}", self.blocks.len());
        println!("Latest block: {}", self.get_latest_block().number);
        println!("Latest hash: {:?}", self.get_latest_block().hash);
        println!("Total supply: {} wei", self.get_total_supply());

        println!("\n=== BLOCKS ===");
        for block in &self.blocks {
            println!("Block {}: {:?} ({} txs, {} gas used)",
                     block.number,
                     block.hash,
                     block.transactions.len(),
                     block.gas_used
            );
        }

        println!("\n=== CONTRACTS ===");
        self.state.print_contracts();
    }

    pub fn get_stats(&self) -> BlockchainStats {
        let mut total_transactions = 0;
        let mut total_gas_used = 0;
        let mut contract_count = 0;

        for block in &self.blocks {
            total_transactions += block.transactions.len();
            total_gas_used += block.gas_used;
        }

        for (_, account) in &self.state.accounts {
            if account.is_contract() {
                contract_count += 1;
            }
        }

        BlockchainStats {
            block_count: self.blocks.len(),
            transaction_count: total_transactions,
            total_gas_used,
            total_supply: self.get_total_supply(),
            contract_count,
            chain_id: self.chain_id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BlockchainStats {
    pub block_count: usize,
    pub transaction_count: usize,
    pub total_gas_used: u64,
    pub total_supply: u64,
    pub contract_count: usize,
    pub chain_id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blockchain_creation() {
        let blockchain = Blockchain::new();
        assert_eq!(blockchain.get_block_count(), 1);
        assert_eq!(blockchain.get_latest_block().number, 0);
        assert_eq!(blockchain.chain_id, 1337);
    }

    #[test]
    fn test_add_valid_block() {
        let mut blockchain = Blockchain::new();

        let alice = Address::from([1u8; 20]);
        let bob = Address::from([2u8; 20]);

        let mut tx = Transaction::new_transfer(alice, bob, U256::from(100), 0);
        tx.set_hash();

        let block = Block::new(
            1,
            blockchain.get_latest_block().hash.unwrap(),
            vec![tx],
        );

        let result = blockchain.add_block(block);
        assert!(result.is_ok());
        assert_eq!(blockchain.get_block_count(), 2);
    }

    #[test]
    fn test_invalid_parent_hash() {
        let mut blockchain = Blockchain::new();

        let block = Block::new(
            1,
            H256::from([99u8; 32]), // Wrong parent hash
            vec![],
        );

        let result = blockchain.add_block(block);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid parent hash"));
    }

    #[test]
    fn test_chain_validation() {
        let mut blockchain = Blockchain::new();

        for i in 1..=3 {
            let alice = Address::from([1u8; 20]);
            let bob = Address::from([2u8; 20]);

            let mut tx = Transaction::new_transfer(alice, bob, U256::from(10), i - 1);
            tx.set_hash();

            let block = Block::new(
                i,
                blockchain.get_latest_block().hash.unwrap(),
                vec![tx],
            );

            blockchain.add_block(block).unwrap();
        }

        let validation = blockchain.validate_chain();
        assert!(validation.is_ok());
    }

    #[test]
    fn test_transaction_history() {
        let mut blockchain = Blockchain::new();

        let alice = Address::from([1u8; 20]);
        let bob = Address::from([2u8; 20]);

        for i in 0..3 {
            let mut tx = Transaction::new_transfer(alice, bob, U256::from(100), i);
            tx.set_hash();

            let block = Block::new(
                i + 1,
                blockchain.get_latest_block().hash.unwrap(),
                vec![tx],
            );

            blockchain.add_block(block).unwrap();
        }

        let alice_txs = blockchain.get_transactions_for_address(&alice);
        assert_eq!(alice_txs.len(), 3);

        let bob_txs = blockchain.get_transactions_for_address(&bob);
        assert_eq!(bob_txs.len(), 3);
    }

    #[test]
    fn test_balance_tracking() {
        let mut blockchain = Blockchain::new();

        let alice = Address::from([1u8; 20]);
        let bob = Address::from([2u8; 20]);

        let initial_alice_balance = blockchain.state.get_balance(&alice);
        let initial_bob_balance = blockchain.state.get_balance(&bob);

        let mut tx = Transaction::new_transfer(alice, bob, U256::from(1000), 0);
        tx.set_hash();

        let block = Block::new(
            1,
            blockchain.get_latest_block().hash.unwrap(),
            vec![tx],
        );

        blockchain.add_block(block).unwrap();

        let final_alice_balance = blockchain.state.get_balance(&alice);
        let final_bob_balance = blockchain.state.get_balance(&bob);

        assert!(final_alice_balance < initial_alice_balance);
        assert_eq!(final_bob_balance, initial_bob_balance + U256::from(1000));
    }

    #[test]
    fn test_contract_deployment_simulation() {
        let mut blockchain = Blockchain::new();

        let alice = Address::from([1u8; 20]);
        let bytecode = vec![0x60, 0x80, 0x60, 0x40, 0x52]; // Simple bytecode

        let mut tx = Transaction::new_contract_deployment(alice, bytecode.clone(), U256::zero(), 0);
        tx.set_hash();

        assert!(tx.is_contract_deployment());
        assert_eq!(tx.data, bytecode);
        assert!(tx.to.is_none());
    }

    #[test]
    fn test_blockchain_stats() {
        let mut blockchain = Blockchain::new();

        let alice = Address::from([1u8; 20]);
        let bob = Address::from([2u8; 20]);

        let mut tx = Transaction::new_transfer(alice, bob, U256::from(100), 0);
        tx.set_hash();

        let block = Block::new(
            1,
            blockchain.get_latest_block().hash.unwrap(),
            vec![tx],
        );

        blockchain.add_block(block).unwrap();

        let stats = blockchain.get_stats();
        assert_eq!(stats.block_count, 2);
        assert_eq!(stats.transaction_count, 1);
        assert_eq!(stats.chain_id, 1337);
    }
}