use ethereum_types::{Address, U256, H256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use sha3::{Digest, Keccak256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub balance: U256,
    pub nonce: u64,
    pub code: Vec<u8>,
    pub code_hash: H256,
    pub storage: HashMap<U256, U256>,
}

impl Account {
    pub fn new() -> Self {
        Account {
            balance: U256::zero(),
            nonce: 0,
            code: Vec::new(),
            code_hash: H256::zero(),
            storage: HashMap::new(),
        }
    }

    pub fn new_with_balance(balance: U256) -> Self {
        Account {
            balance,
            nonce: 0,
            code: Vec::new(),
            code_hash: H256::zero(),
            storage: HashMap::new(),
        }
    }

    pub fn new_contract(balance: U256, code: Vec<u8>) -> Self {
        let code_hash = if code.is_empty() {
            H256::zero()
        } else {
            H256::from_slice(&Keccak256::digest(&code))
        };

        Account {
            balance,
            nonce: 0,
            code,
            code_hash,
            storage: HashMap::new(),
        }
    }

    pub fn is_contract(&self) -> bool {
        !self.code.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.balance == U256::zero()
            && self.nonce == 0
            && self.code.is_empty()
            && self.storage.is_empty()
    }

    pub fn set_code(&mut self, code: Vec<u8>) {
        self.code = code;
        if self.code.is_empty() {
            self.code_hash = H256::zero();
        } else {
            self.code_hash = H256::from_slice(&Keccak256::digest(&self.code));
        }
    }

    pub fn get_storage(&self, key: &U256) -> U256 {
        self.storage.get(key).copied().unwrap_or(U256::zero())
    }

    pub fn set_storage(&mut self, key: U256, value: U256) {
        if value == U256::zero() {
            // Remove zero values to save space
            self.storage.remove(&key);
        } else {
            self.storage.insert(key, value);
        }
    }

    pub fn get_all_storage(&self) -> &HashMap<U256, U256> {
        &self.storage
    }

    pub fn clear_storage(&mut self) {
        self.storage.clear();
    }

    pub fn add_balance(&mut self, amount: U256) {
        self.balance += amount;
    }

    pub fn sub_balance(&mut self, amount: U256) -> Result<(), String> {
        if self.balance < amount {
            return Err("Insufficient balance".to_string());
        }
        self.balance -= amount;
        Ok(())
    }

    pub fn increment_nonce(&mut self) {
        self.nonce += 1;
    }

    pub fn size(&self) -> usize {
        32 + // balance
            8 + // nonce
            self.code.len() + // code
            32 + // code_hash
            (self.storage.len() * (32 + 32)) // storage (key + value pairs)
    }

    pub fn storage_root(&self) -> H256 {
        if self.storage.is_empty() {
            return H256::zero();
        }

        let mut hasher = Keccak256::new();
        let mut sorted_storage: Vec<_> = self.storage.iter().collect();
        sorted_storage.sort_by_key(|&(k, _)| k);

        for (key, value) in sorted_storage {
            let mut key_bytes = [0u8; 32];
            let mut value_bytes = [0u8; 32];
            key.to_big_endian(&mut key_bytes);
            value.to_big_endian(&mut value_bytes);
            hasher.update(&key_bytes);
            hasher.update(&value_bytes);
        }

        H256::from_slice(&hasher.finalize())
    }
}

impl Default for Account {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldState {
    pub accounts: HashMap<Address, Account>,
    pub state_root: H256,
}

impl WorldState {
    pub fn new() -> Self {
        WorldState {
            accounts: HashMap::new(),
            state_root: H256::zero(),
        }
    }

    pub fn get_account(&self, address: &Address) -> Option<&Account> {
        self.accounts.get(address)
    }

    pub fn get_account_mut(&mut self, address: &Address) -> &mut Account {
        self.accounts.entry(*address).or_insert_with(Account::new)
    }

    pub fn account_exists(&self, address: &Address) -> bool {
        self.accounts.contains_key(address)
    }

    pub fn create_account(&mut self, address: Address, account: Account) {
        self.accounts.insert(address, account);
        self.update_state_root();
    }

    pub fn delete_account(&mut self, address: &Address) {
        self.accounts.remove(address);
        self.update_state_root();
    }

    pub fn get_balance(&self, address: &Address) -> U256 {
        self.accounts.get(address)
            .map(|acc| acc.balance)
            .unwrap_or(U256::zero())
    }

    pub fn set_balance(&mut self, address: &Address, balance: U256) {
        let account = self.get_account_mut(address);
        account.balance = balance;
        self.update_state_root();
    }

    pub fn get_nonce(&self, address: &Address) -> u64 {
        self.accounts.get(address)
            .map(|acc| acc.nonce)
            .unwrap_or(0)
    }

    pub fn set_nonce(&mut self, address: &Address, nonce: u64) {
        let account = self.get_account_mut(address);
        account.nonce = nonce;
        self.update_state_root();
    }

    pub fn increment_nonce(&mut self, address: &Address) {
        let account = self.get_account_mut(address);
        account.increment_nonce();
        self.update_state_root();
    }

    pub fn transfer(&mut self, from: &Address, to: &Address, amount: U256) -> Result<(), String> {
        if self.get_balance(from) < amount {
            return Err("Insufficient balance".to_string());
        }

        {
            let sender = self.get_account_mut(from);
            sender.sub_balance(amount)?;
            sender.increment_nonce();
        }

        {
            let receiver = self.get_account_mut(to);
            receiver.add_balance(amount);
        }

        self.update_state_root();
        Ok(())
    }

    pub fn deploy_contract(&mut self, deployer: &Address, contract_address: &Address, code: Vec<u8>) -> Result<(), String> {
        if self.accounts.contains_key(contract_address) {
            return Err("Contract address already exists".to_string());
        }

        self.increment_nonce(deployer);

        let contract_account = Account::new_contract(U256::zero(), code);
        self.accounts.insert(*contract_address, contract_account);

        self.update_state_root();
        println!("Contract deployed at {} with {} bytes of code",
                 contract_address,
                 self.accounts[contract_address].code.len()
        );
        Ok(())
    }

    pub fn get_contract_code(&self, address: &Address) -> Vec<u8> {
        self.accounts.get(address)
            .map(|acc| acc.code.clone())
            .unwrap_or_default()
    }

    pub fn set_contract_code(&mut self, address: &Address, code: Vec<u8>) {
        let account = self.get_account_mut(address);
        account.set_code(code);
        self.update_state_root();
    }

    pub fn is_contract(&self, address: &Address) -> bool {
        self.accounts.get(address)
            .map(|acc| acc.is_contract())
            .unwrap_or(false)
    }

    pub fn get_storage(&self, address: &Address, key: &U256) -> U256 {
        self.accounts.get(address)
            .map(|acc| acc.get_storage(key))
            .unwrap_or(U256::zero())
    }

    pub fn set_storage(&mut self, address: &Address, key: U256, value: U256) {
        let account = self.get_account_mut(address);
        account.set_storage(key, value);
        self.update_state_root();
    }

    pub fn get_all_storage(&self, address: &Address) -> HashMap<U256, U256> {
        self.accounts.get(address)
            .map(|acc| acc.storage.clone())
            .unwrap_or_default()
    }

    pub fn clear_storage(&mut self, address: &Address) {
        if let Some(account) = self.accounts.get_mut(address) {
            account.clear_storage();
            self.update_state_root();
        }
    }

    pub fn get_all_contracts(&self) -> Vec<Address> {
        self.accounts.iter()
            .filter(|(_, account)| account.is_contract())
            .map(|(address, _)| *address)
            .collect()
    }

    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    pub fn contract_count(&self) -> usize {
        self.accounts.values()
            .filter(|account| account.is_contract())
            .count()
    }

    pub fn total_balance(&self) -> U256 {
        self.accounts.values()
            .fold(U256::zero(), |acc, account| acc + account.balance)
    }

    pub fn remove_empty_accounts(&mut self) {
        self.accounts.retain(|_, account| !account.is_empty());
        self.update_state_root();
    }

    pub fn update_state_root(&mut self) {
        if self.accounts.is_empty() {
            self.state_root = H256::zero();
            return;
        }

        let mut hasher = Keccak256::new();
        let mut sorted_accounts: Vec<_> = self.accounts.iter().collect();
        sorted_accounts.sort_by_key(|&(addr, _)| addr);

        for (address, account) in sorted_accounts {
            hasher.update(address.as_bytes());

            let mut account_hasher = Keccak256::new();
            let mut balance_bytes = [0u8; 32];
            account.balance.to_big_endian(&mut balance_bytes);
            account_hasher.update(&balance_bytes);
            account_hasher.update(&account.nonce.to_be_bytes());
            account_hasher.update(account.code_hash.as_bytes());
            account_hasher.update(account.storage_root().as_bytes());

            hasher.update(&account_hasher.finalize());
        }

        self.state_root = H256::from_slice(&hasher.finalize());
    }

    pub fn get_state_root(&self) -> H256 {
        self.state_root
    }

    pub fn print_contracts(&self) {
        let contracts: Vec<_> = self.accounts.iter()
            .filter(|(_, account)| account.is_contract())
            .collect();

        if contracts.is_empty() {
            println!("No contracts deployed");
            return;
        }

        println!("=== DEPLOYED CONTRACTS ===");
        for (address, account) in contracts {
            println!("Contract {}: {} bytes, {} storage entries, balance: {} wei",
                     address,
                     account.code.len(),
                     account.storage.len(),
                     account.balance
            );

            if !account.storage.is_empty() {
                println!("  Storage entries:");
                for (i, (key, value)) in account.storage.iter().enumerate() {
                    if i >= 5 { // Limit output
                        println!("  ... and {} more", account.storage.len() - 5);
                        break;
                    }
                    println!("    Slot {}: {}", key, value);
                }
            }
        }
    }

    pub fn print_accounts(&self) {
        println!("\n=== ACCOUNT INFORMATION ===");
        println!("Total accounts: {}", self.account_count());
        println!("Total contracts: {}", self.contract_count());
        println!("Total balance: {} wei", self.total_balance());
        println!("State root: {:?}", self.state_root);

        let mut sorted_accounts: Vec<_> = self.accounts.iter().collect();
        sorted_accounts.sort_by_key(|&(addr, _)| addr);

        for (address, account) in sorted_accounts {
            let account_type = if account.is_contract() { "Contract" } else { "EOA" };
            println!("{} {}: {} wei, nonce: {}",
                     account_type,
                     address,
                     account.balance,
                     account.nonce
            );
        }
    }

    pub fn snapshot(&self) -> WorldStateSnapshot {
        WorldStateSnapshot {
            accounts: self.accounts.clone(),
            state_root: self.state_root,
        }
    }

    pub fn restore_snapshot(&mut self, snapshot: WorldStateSnapshot) {
        self.accounts = snapshot.accounts;
        self.state_root = snapshot.state_root;
    }

    pub fn apply_changes(&mut self, other: &WorldState) {
        for (address, account) in &other.accounts {
            self.accounts.insert(*address, account.clone());
        }
        self.update_state_root();
    }
}

impl Default for WorldState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct WorldStateSnapshot {
    pub accounts: HashMap<Address, Account>,
    pub state_root: H256,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_creation() {
        let account = Account::new();
        assert_eq!(account.balance, U256::zero());
        assert_eq!(account.nonce, 0);
        assert!(!account.is_contract());
        assert!(account.is_empty());
    }

    #[test]
    fn test_account_with_balance() {
        let account = Account::new_with_balance(U256::from(1000));
        assert_eq!(account.balance, U256::from(1000));
        assert!(!account.is_empty());
    }

    #[test]
    fn test_contract_account() {
        let code = vec![0x60, 0x80, 0x60, 0x40];
        let account = Account::new_contract(U256::from(500), code.clone());
        assert_eq!(account.balance, U256::from(500));
        assert_eq!(account.code, code);
        assert!(account.is_contract());
        assert!(!account.is_empty());
        assert_ne!(account.code_hash, H256::zero());
    }

    #[test]
    fn test_storage_operations() {
        let mut account = Account::new();

        account.set_storage(U256::from(1), U256::from(42));
        assert_eq!(account.get_storage(&U256::from(1)), U256::from(42));

        assert_eq!(account.get_storage(&U256::from(2)), U256::zero());

        account.set_storage(U256::from(1), U256::zero());
        assert_eq!(account.get_storage(&U256::from(1)), U256::zero());
        assert!(account.storage.is_empty());
    }

    #[test]
    fn test_world_state_transfer() {
        let mut state = WorldState::new();
        let alice = Address::from([1u8; 20]);
        let bob = Address::from([2u8; 20]);

        state.set_balance(&alice, U256::from(1000));
        state.set_balance(&bob, U256::from(500));

        let result = state.transfer(&alice, &bob, U256::from(300));
        assert!(result.is_ok());

        assert_eq!(state.get_balance(&alice), U256::from(700));
        assert_eq!(state.get_balance(&bob), U256::from(800));
        assert_eq!(state.get_nonce(&alice), 1); // Nonce incremented
    }

    #[test]
    fn test_insufficient_balance() {
        let mut state = WorldState::new();
        let alice = Address::from([1u8; 20]);
        let bob = Address::from([2u8; 20]);

        state.set_balance(&alice, U256::from(100));

        let result = state.transfer(&alice, &bob, U256::from(200));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Insufficient balance");
    }

    #[test]
    fn test_contract_deployment() {
        let mut state = WorldState::new();
        let deployer = Address::from([1u8; 20]);
        let contract_addr = Address::from([2u8; 20]);

        let bytecode = vec![0x60, 0x80, 0x60, 0x40]; // Sample bytecode

        let result = state.deploy_contract(&deployer, &contract_addr, bytecode.clone());
        assert!(result.is_ok());

        assert!(state.is_contract(&contract_addr));
        assert_eq!(state.get_contract_code(&contract_addr), bytecode);
        assert_eq!(state.get_nonce(&deployer), 1);
    }

    #[test]
    fn test_storage_operations_in_state() {
        let mut state = WorldState::new();
        let contract = Address::from([1u8; 20]);

        state.deploy_contract(&Address::from([2u8; 20]), &contract, vec![0x60, 0x80]).unwrap();

        state.set_storage(&contract, U256::from(1), U256::from(42));
        assert_eq!(state.get_storage(&contract, &U256::from(1)), U256::from(42));

        let all_storage = state.get_all_storage(&contract);
        assert_eq!(all_storage.len(), 1);
        assert_eq!(all_storage[&U256::from(1)], U256::from(42));
    }

    #[test]
    fn test_state_root_calculation() {
        let mut state = WorldState::new();
        let initial_root = state.get_state_root();

        let alice = Address::from([1u8; 20]);
        state.set_balance(&alice, U256::from(1000));

        let new_root = state.get_state_root();
        assert_ne!(initial_root, new_root);

        let mut state2 = WorldState::new();
        state2.set_balance(&alice, U256::from(1000));
        assert_eq!(state2.get_state_root(), new_root);
    }

    #[test]
    fn test_snapshot_and_restore() {
        let mut state = WorldState::new();
        let alice = Address::from([1u8; 20]);

        state.set_balance(&alice, U256::from(1000));
        let snapshot = state.snapshot();

        state.set_balance(&alice, U256::from(2000));
        assert_eq!(state.get_balance(&alice), U256::from(2000));

        state.restore_snapshot(snapshot);
        assert_eq!(state.get_balance(&alice), U256::from(1000));
    }

    #[test]
    fn test_empty_account_removal() {
        let mut state = WorldState::new();
        let alice = Address::from([1u8; 20]);

        state.set_balance(&alice, U256::from(1000));
        assert_eq!(state.account_count(), 1);

        state.set_balance(&alice, U256::zero());

        state.remove_empty_accounts();
        assert_eq!(state.account_count(), 0);
    }

    #[test]
    fn test_contract_statistics() {
        let mut state = WorldState::new();

        let contract1 = Address::from([1u8; 20]);
        let contract2 = Address::from([2u8; 20]);
        let deployer = Address::from([3u8; 20]);

        state.deploy_contract(&deployer, &contract1, vec![0x60, 0x80]).unwrap();
        state.deploy_contract(&deployer, &contract2, vec![0x60, 0x40]).unwrap();

        let alice = Address::from([4u8; 20]);
        state.set_balance(&alice, U256::from(1000));

        assert_eq!(state.account_count(), 4);
        assert_eq!(state.contract_count(), 2);

        let contracts = state.get_all_contracts();
        assert_eq!(contracts.len(), 2);
        assert!(contracts.contains(&contract1));
        assert!(contracts.contains(&contract2));
    }
}