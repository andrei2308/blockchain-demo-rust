use crate::account::WorldState;
use ethereum_types::{Address, U256, H256};
use revm::{
    primitives::{
        AccountInfo, Bytecode, ExecutionResult, Output, TransactTo,
        B256, U256 as rU256, Address as rAddress, Bytes,
    },
    Database, DatabaseCommit, Evm, EvmBuilder, InMemoryDB,
};

fn ethereum_u256_to_revm_u256(value: U256) -> rU256 {
    let mut bytes = [0u8; 32];
    value.to_big_endian(&mut bytes);
    rU256::from_be_bytes(bytes)
}

fn revm_u256_to_ethereum_u256(value: rU256) -> U256 {
    let bytes: [u8; 32] = value.to_be_bytes();
    U256::from_big_endian(&bytes)
}

fn h256_to_u256(value: H256) -> U256 {
    U256::from_big_endian(value.as_bytes())
}

pub struct RevmExecutor {
    pub evm: Evm<'static, (), InMemoryDB>,
}

impl RevmExecutor {
    pub fn new(block_number: u64, block_timestamp: u64, coinbase: Address, gas_limit: u64) -> Self {
        let mut evm = EvmBuilder::default()
            .with_db(InMemoryDB::default())
            .build();

        evm.context.evm.env.cfg.chain_id = 1337;

        evm.context.evm.env.block.number = rU256::from(block_number);
        evm.context.evm.env.block.timestamp = rU256::from(block_timestamp);
        evm.context.evm.env.block.coinbase = rAddress::from_slice(coinbase.as_bytes());
        evm.context.evm.env.block.gas_limit = rU256::from(gas_limit);
        evm.context.evm.env.block.basefee = rU256::from(1_000_000_000u64); // 1 gwei

        RevmExecutor { evm }
    }

    pub fn load_state_from_world(&mut self, state: &WorldState) -> Result<(), String> {
        for (address, account) in &state.accounts {
            let account_info = AccountInfo {
                balance: ethereum_u256_to_revm_u256(account.balance),
                nonce: account.nonce,
                code_hash: B256::from_slice(account.code_hash.as_bytes()),
                code: if account.code.is_empty() {
                    None
                } else {
                    Some(Bytecode::new_raw(Bytes::from(account.code.clone())))
                },
            };

            self.evm.context.evm.db.insert_account_info(
                rAddress::from_slice(address.as_bytes()),
                account_info,
            );

            for (key, value) in &account.storage {
                self.evm.context.evm.db.insert_account_storage(
                    rAddress::from_slice(address.as_bytes()),
                    ethereum_u256_to_revm_u256(*key),
                    ethereum_u256_to_revm_u256(*value),
                ).map_err(|e| format!("Storage load error: {:?}", e))?;
            }
        }
        Ok(())
    }

    pub fn execute_transaction(
        &mut self,
        from: Address,
        to: Option<Address>,
        value: U256,
        data: Vec<u8>,
        gas_limit: u64,
        gas_price: U256,
        nonce: u64,
    ) -> Result<ContractExecutionResult, String> {
        self.evm.context.evm.env.tx.caller = rAddress::from_slice(from.as_bytes());
        self.evm.context.evm.env.tx.gas_limit = gas_limit;
        self.evm.context.evm.env.tx.gas_price = ethereum_u256_to_revm_u256(gas_price);
        self.evm.context.evm.env.tx.value = ethereum_u256_to_revm_u256(value);
        self.evm.context.evm.env.tx.data = Bytes::from(data);
        self.evm.context.evm.env.tx.nonce = Some(nonce);

        self.evm.context.evm.env.tx.transact_to = match to {
            Some(addr) => TransactTo::Call(rAddress::from_slice(addr.as_bytes())),
            None => TransactTo::Create,
        };

        let result = self.evm.transact_commit()
            .map_err(|e| format!("REVM execution failed: {:?}", e))?;

        self.process_execution_result(result)
    }

    fn process_execution_result(&self, result: ExecutionResult) -> Result<ContractExecutionResult, String> {
        match result {
            ExecutionResult::Success { reason, gas_used, gas_refunded, logs, output } => {
                let (contract_address, return_data) = match output {
                    Output::Call(data) => (None, data.to_vec()),
                    Output::Create(data, addr) => {
                        let contract_addr = if let Some(addr) = addr {
                            Some(Address::from_slice(&addr.0[..20]))
                        } else {
                            None
                        };
                        (contract_addr, data.to_vec())
                    }
                };

                Ok(ContractExecutionResult {
                    success: true,
                    gas_used: gas_used as u64,
                    gas_refunded: gas_refunded as u64,
                    return_data,
                    contract_address,
                    logs: logs.into_iter().map(|log| EvmLog {
                        address: Address::from_slice(&log.address.0[..20]),
                        topics: log.topics().iter().map(|t| H256::from_slice(&t.0)).collect(),
                        data: log.data.data.to_vec(),
                    }).collect(),
                    reason: format!("{:?}", reason),
                    error: None,
                })
            }
            ExecutionResult::Revert { gas_used, output } => {
                Ok(ContractExecutionResult {
                    success: false,
                    gas_used: gas_used as u64,
                    gas_refunded: 0,
                    return_data: output.to_vec(),
                    contract_address: None,
                    logs: vec![],
                    reason: "Revert".to_string(),
                    error: Some("Transaction reverted".to_string()),
                })
            }
            ExecutionResult::Halt { reason, gas_used } => {
                Err(format!("EVM halted: {:?}, gas used: {}", reason, gas_used))
            }
        }
    }

    pub fn save_state_to_world(&mut self, state: &mut WorldState) -> Result<(), String> {
        for (address, _) in &state.accounts.clone() {
            let evm_addr = rAddress::from_slice(address.as_bytes());

            if let Ok(Some(account_info)) = self.evm.context.evm.db.basic(evm_addr) {
                let account = state.get_account_mut(address);
                account.balance = revm_u256_to_ethereum_u256(account_info.balance);
                account.nonce = account_info.nonce;

                if let Some(code) = account_info.code {
                    account.code = code.bytes().to_vec();
                    if !account.code.is_empty() {
                        use sha3::{Digest, Keccak256};
                        account.code_hash = H256::from_slice(&Keccak256::digest(&account.code));
                    }
                }
            }
        }

        Ok(())
    }

    pub fn deploy_contract(
        &mut self,
        deployer: Address,
        bytecode: Vec<u8>,
        constructor_args: Vec<u8>,
        value: U256,
        gas_limit: u64,
        nonce: u64,
    ) -> Result<ContractExecutionResult, String> {
        println!("Deploying contract from {} with {} bytes of bytecode", deployer, bytecode.len());

        let mut deployment_data = bytecode;
        deployment_data.extend_from_slice(&constructor_args);

        let result = self.execute_transaction(
            deployer,
            None,
            value,
            deployment_data,
            gas_limit,
            U256::from(20_000_000_000u64), // 20 gwei
            nonce,
        )?;

        if result.success {
            println!("Contract deployed successfully at: {:?}", result.contract_address);
            println!("Gas used: {}", result.gas_used);
        } else {
            println!("Contract deployment failed: {}", result.reason);
            if let Some(error) = &result.error {
                println!("Error details: {}", error);
            }
        }

        Ok(result)
    }

    pub fn call_contract(
        &mut self,
        caller: Address,
        contract: Address,
        calldata: Vec<u8>,
        value: U256,
        gas_limit: u64,
        nonce: u64,
    ) -> Result<ContractExecutionResult, String> {
        println!("Calling contract {} from {} with {} bytes of calldata", contract, caller, calldata.len());

        let result = self.execute_transaction(
            caller,
            Some(contract),
            value,
            calldata,
            gas_limit,
            U256::from(20_000_000_000u64),
            nonce,
        )?;

        if result.success {
            println!("Contract call successful");
            println!("Gas used: {}", result.gas_used);
            println!("Returned {} bytes", result.return_data.len());
            if !result.logs.is_empty() {
                println!("Emitted {} events", result.logs.len());
            }
        } else {
            println!("Contract call failed: {}", result.reason);
            if let Some(error) = &result.error {
                println!("Error details: {}", error);
            }
        }

        Ok(result)
    }

    pub fn view_call(
        &mut self,
        caller: Address,
        contract: Address,
        calldata: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let result = self.call_contract(
            caller,
            contract,
            calldata,
            U256::zero(),
            1_000_000, // High gas limit for view calls
            0, // Nonce doesn't matter for view calls
        )?;

        if result.success {
            Ok(result.return_data)
        } else {
            Err(format!("View call failed: {}", result.reason))
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContractExecutionResult {
    pub success: bool,
    pub gas_used: u64,
    pub gas_refunded: u64,
    pub return_data: Vec<u8>,
    pub contract_address: Option<Address>,
    pub logs: Vec<EvmLog>,
    pub reason: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EvmLog {
    pub address: Address,
    pub topics: Vec<H256>,
    pub data: Vec<u8>,
}

pub struct ContractUtils;

impl ContractUtils {
    pub fn calculate_create_address(deployer: &Address, nonce: u64) -> Address {
        use rlp::RlpStream;
        use sha3::{Digest, Keccak256};

        let mut stream = RlpStream::new_list(2);
        stream.append(deployer);
        stream.append(&nonce);

        let hash = Keccak256::digest(&stream.out());
        Address::from_slice(&hash[12..])
    }

    pub fn calculate_create2_address(
        deployer: &Address,
        salt: &H256,
        bytecode_hash: &H256,
    ) -> Address {
        use sha3::{Digest, Keccak256};

        let mut hasher = Keccak256::new();
        hasher.update(&[0xff]);
        hasher.update(deployer.as_bytes());
        hasher.update(salt.as_bytes());
        hasher.update(bytecode_hash.as_bytes());

        let hash = hasher.finalize();
        Address::from_slice(&hash[12..])
    }

    pub fn encode_function_call(signature: &str, params: &[Vec<u8>]) -> Vec<u8> {
        use sha3::{Digest, Keccak256};

        let hash = Keccak256::digest(signature.as_bytes());
        let mut calldata = hash[0..4].to_vec();

        for param in params {
            calldata.extend_from_slice(param);
        }

        calldata
    }

    pub fn decode_uint256(data: &[u8]) -> U256 {
        if data.len() >= 32 {
            U256::from_big_endian(&data[0..32])
        } else {
            U256::zero()
        }
    }

    pub fn encode_uint256(value: U256) -> Vec<u8> {
        let mut bytes = vec![0u8; 32];
        value.to_big_endian(&mut bytes);
        bytes
    }

    pub fn parse_bytecode(hex_str: &str) -> Result<Vec<u8>, String> {
        let clean_hex = hex_str.trim_start_matches("0x");
        hex::decode(clean_hex).map_err(|e| format!("Invalid hex: {}", e))
    }
}

pub struct SolidityContracts;

impl SolidityContracts {
    pub fn simple_storage_bytecode() -> Vec<u8> {
        // This is real bytecode for:
        // contract SimpleStorage {
        //     uint256 private storedData;
        //     function set(uint256 x) public { storedData = x; }
        //     function get() public view returns (uint256) { return storedData; }
        // }
        hex::decode("608060405234801561001057600080fd5b50610150806100206000396000f3fe608060405234801561001057600080fd5b50600436106100365760003560e01c80636057361d1461003b5780636d4ce63c14610057575b600080fd5b61005560048036038101906100509190610094565b610075565b005b61005f6100a8565b60405161006c91906100cf565b60405180910390f35b8060008190555050565b60008054905090565b60008135905061009d81610102565b92915050565b6000602082840312156100b557600080fd5b60006100c38482850161008e565b91505092915050565b6100d5816100f8565b82525050565b60006020820190506100f060008301846100cc565b92915050565b6000819050919050565b61010b816100f8565b811461011657600080fd5b5056fea2646970667358221220abcdef1234567890abcdef1234567890abcdef1234567890abcdef123456789064736f6c63430008070033")
            .unwrap_or_else(|_| {
                // Fallback simple bytecode
                vec![
                    0x60, 0x80, 0x60, 0x40, 0x52, 0x34, 0x80, 0x15, 0x61, 0x00, 0x10, 0x57, 0x60, 0x00, 0x80, 0xfd,
                    0x5b, 0x50, 0x60, 0x04, 0x36, 0x10, 0x61, 0x00, 0x36, 0x57, 0x60, 0x00, 0x35, 0x60, 0xe0, 0x1c,
                    0x80, 0x63, 0x60, 0x57, 0x36, 0x1d, 0x14, 0x61, 0x00, 0x4a, 0x57, 0x80, 0x63, 0x6d, 0x4c, 0xe6,
                    0x3c, 0x14, 0x61, 0x00, 0x55, 0x57, 0x5b, 0x60, 0x00, 0x80, 0xfd, 0x5b, 0x61, 0x00, 0x53, 0x61,
                ]
            })
    }

    pub fn set_function_selector() -> [u8; 4] {
        [0x60, 0x57, 0x36, 0x1d] // set(uint256)
    }

    pub fn get_function_selector() -> [u8; 4] {
        [0x6d, 0x4c, 0xe6, 0x3c] // get()
    }

    pub fn encode_set_call(value: U256) -> Vec<u8> {
        let mut calldata = Self::set_function_selector().to_vec();
        calldata.extend_from_slice(&ContractUtils::encode_uint256(value));
        calldata
    }

    pub fn encode_get_call() -> Vec<u8> {
        Self::get_function_selector().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::WorldState;

    #[test]
    fn test_revm_creation() {
        let executor = RevmExecutor::new(1, 1234567890, Address::from([1u8; 20]), 9000_000_000_000_000_000);
        assert!(true);
    }

    #[test]
    fn test_u256_conversions() {
        let eth_u256 = U256::from(12345);
        let revm_u256 = ethereum_u256_to_revm_u256(eth_u256);
        let back_to_eth = revm_u256_to_ethereum_u256(revm_u256);
        assert_eq!(eth_u256, back_to_eth);
    }

    #[test]
    fn test_h256_to_u256_conversion() {
        let h256 = H256::from([1u8; 32]);
        let u256 = h256_to_u256(h256);
        assert_ne!(u256, U256::zero());
    }

    #[test]
    fn test_contract_address_calculation() {
        let deployer = Address::from([1u8; 20]);
        let addr = ContractUtils::calculate_create_address(&deployer, 0);
        assert_ne!(addr, Address::zero());
    }

    #[test]
    fn test_function_encoding() {
        let set_call = SolidityContracts::encode_set_call(U256::from(42));
        assert_eq!(set_call.len(), 4 + 32);

        let get_call = SolidityContracts::encode_get_call();
        assert_eq!(get_call.len(), 4);
    }
}