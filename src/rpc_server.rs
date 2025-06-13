use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use serde_json::{json, Value};
use warp::{Filter, Reply};
use ethereum_types::{Address, U256, H256};
use crate::blockchain::Blockchain;
use crate::miner::Miner;
use crate::transaction::{Transaction, TransactionType};

pub struct RpcServer {
    blockchain: Arc<Mutex<Blockchain>>,
    miner: Arc<Miner>,
    pending_transactions: Arc<Mutex<Vec<Transaction>>>,
    auto_mining: Arc<Mutex<bool>>,
}

impl RpcServer {
    pub fn new(blockchain: Blockchain, miner: Miner) -> Self {
        RpcServer {
            blockchain: Arc::new(Mutex::new(blockchain)),
            miner: Arc::new(miner),
            pending_transactions: Arc::new(Mutex::new(Vec::new())),
            auto_mining: Arc::new(Mutex::new(true)), // Auto-mine by default
        }
    }

    pub async fn start(self, port: u16) {
        let server = Arc::new(self);

        let rpc_route = warp::path("rpc")
            .and(warp::post())
            .and(warp::body::json())
            .and(with_server(server.clone()))
            .and_then(handle_rpc_request);

        let cors = warp::cors()
            .allow_any_origin()
            .allow_headers(vec!["content-type"])
            .allow_methods(vec!["POST", "OPTIONS"]);

        let routes = rpc_route.with(cors);

        println!("RPC Server starting on http://localhost:{}", port);
        println!("You can now connect MetaMask or use web3 tools!");
        println!();
        println!("Connection details:");
        println!("   • RPC URL: http://localhost:{}", port);
        println!("   • Chain ID: 1337");
        println!("   • Currency: ETH");
        println!();

        warp::serve(routes)
            .run(([127, 0, 0, 1], port))
            .await;
    }
}

fn with_server(server: Arc<RpcServer>) -> impl Filter<Extract = (Arc<RpcServer>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || server.clone())
}

async fn handle_rpc_request(
    request: Value,
    server: Arc<RpcServer>
) -> Result<impl Reply, warp::Rejection> {
    let response = process_rpc_request(&request, &server).await;
    Ok(warp::reply::json(&response))
}

async fn process_rpc_request(request: &Value, server: &Arc<RpcServer>) -> Value {
    let method = request["method"].as_str().unwrap_or("");
    let params = &request["params"];
    let id = &request["id"];

    println!("RPC Request: {} {:?}", method, params);

    let result = match method {
        "eth_chainId" => json!("0x539"), // 1337 in hex
        "net_version" => json!("1337"),
        "eth_blockNumber" => handle_block_number(server),
        "eth_getBalance" => handle_get_balance(params, server),
        "eth_getTransactionCount" => handle_get_transaction_count(params, server),
        "eth_sendTransaction" => handle_send_transaction(params, server).await,
        "eth_sendRawTransaction" => handle_send_raw_transaction(params, server).await,
        "eth_call" => handle_eth_call(params, server).await,
        "eth_getCode" => handle_get_code(params, server),
        "eth_getBlockByNumber" => handle_get_block_by_number(params, server),
        "eth_getTransactionReceipt" => handle_get_transaction_receipt(params, server),
        "eth_gasPrice" => json!("0x4a817c800"), // 20 gwei
        "eth_estimateGas" => json!("0x5208"), // 21000 gas
        "web3_clientVersion" => json!("RustBlockchain/1.0.0"),
        "eth_accounts" => handle_eth_accounts(),
        _ => {
            println!("Unknown method: {}", method);
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Method {} not found", method)
                }
            });
        }
    };

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn handle_block_number(server: &Arc<RpcServer>) -> Value {
    let blockchain = server.blockchain.lock().unwrap();
    let block_number = blockchain.get_latest_block().number;
    json!(format!("0x{:x}", block_number))
}

fn handle_get_balance(params: &Value, server: &Arc<RpcServer>) -> Value {
    let address_str = params[0].as_str().unwrap_or("");
    let address = parse_address(address_str);

    let blockchain = server.blockchain.lock().unwrap();
    let balance = blockchain.state.get_balance(&address);

    json!(format!("0x{:x}", balance))
}

fn handle_get_transaction_count(params: &Value, server: &Arc<RpcServer>) -> Value {
    let address_str = params[0].as_str().unwrap_or("");
    let address = parse_address(address_str);

    let blockchain = server.blockchain.lock().unwrap();
    let nonce = blockchain.state.get_nonce(&address);

    json!(format!("0x{:x}", nonce))
}

async fn handle_send_transaction(params: &Value, server: &Arc<RpcServer>) -> Value {
    let tx_params = &params[0];

    let from = parse_address(tx_params["from"].as_str().unwrap_or(""));
    let to = tx_params["to"].as_str().map(parse_address);
    let value = parse_u256(tx_params["value"].as_str().unwrap_or("0x0"));
    let data = parse_hex_data(tx_params["data"].as_str().unwrap_or("0x"));
    let gas_limit = parse_u64(tx_params["gas"].as_str().unwrap_or("0x5208"));
    let gas_price = parse_u256(tx_params["gasPrice"].as_str().unwrap_or("0x4a817c800"));

    let nonce = {
        let blockchain = server.blockchain.lock().unwrap();
        blockchain.state.get_nonce(&from)
    };

    let tx_type = if to.is_none() {
        TransactionType::ContractDeployment
    } else if !data.is_empty() {
        TransactionType::ContractCall
    } else {
        TransactionType::Transfer
    };

    let mut tx = Transaction::new_with_gas(
        from, to, value, data, gas_limit, gas_price, nonce, tx_type
    );
    tx.set_hash();

    let tx_hash = tx.hash.unwrap();

    {
        let mut pending = server.pending_transactions.lock().unwrap();
        pending.push(tx);
    }

    if *server.auto_mining.lock().unwrap() {
        mine_pending_transactions(server).await;
    }

    json!(format!("0x{:x}", tx_hash))
}

async fn handle_send_raw_transaction(params: &Value, server: &Arc<RpcServer>) -> Value {
    let raw_tx = params[0].as_str().unwrap_or("");
    println!("📝 Raw transaction received: {}", raw_tx);
    json!("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
}

async fn handle_eth_call(params: &Value, server: &Arc<RpcServer>) -> Value {
    let call_params = &params[0];
    let to_str = call_params["to"].as_str().unwrap_or("");
    let data_str = call_params["data"].as_str().unwrap_or("0x");

    let to = parse_address(to_str);
    let data = parse_hex_data(data_str);

    println!("Contract call to: 0x{}, data: {}", hex::encode(to.as_bytes()), data_str);

    json!("0x")
}

fn handle_get_code(params: &Value, server: &Arc<RpcServer>) -> Value {
    let address_str = params[0].as_str().unwrap_or("");
    let address = parse_address(address_str);

    let blockchain = server.blockchain.lock().unwrap();
    let code = blockchain.state.get_contract_code(&address);

    if code.is_empty() {
        json!("0x")
    } else {
        json!(format!("0x{}", hex::encode(code)))
    }
}

fn handle_get_block_by_number(params: &Value, server: &Arc<RpcServer>) -> Value {
    let block_number_str = params[0].as_str().unwrap_or("latest");
    let include_txs = params[1].as_bool().unwrap_or(false);

    let blockchain = server.blockchain.lock().unwrap();
    let block = if block_number_str == "latest" {
        blockchain.get_latest_block().clone()
    } else {
        blockchain.get_latest_block().clone()
    };

    let transactions = if include_txs {
        block.transactions.iter().map(|tx| {
            json!({
                "hash": format!("0x{:x}", tx.hash.unwrap_or(H256::zero())),
                "from": format!("0x{}", hex::encode(tx.from.as_bytes())),
                "to": tx.to.map(|addr| format!("0x{}", hex::encode(addr.as_bytes()))),
                "value": format!("0x{:x}", tx.value),
                "gas": format!("0x{:x}", tx.gas_limit),
                "gasPrice": format!("0x{:x}", tx.gas_price),
                "nonce": format!("0x{:x}", tx.nonce),
                "input": format!("0x{}", hex::encode(&tx.data))
            })
        }).collect::<Vec<_>>()
    } else {
        block.transactions.iter().map(|tx|
            json!(format!("0x{:x}", tx.hash.unwrap_or(H256::zero())))
        ).collect::<Vec<_>>()
    };

    json!({
        "number": format!("0x{:x}", block.number),
        "hash": format!("0x{:x}", block.hash.unwrap_or(H256::zero())),
        "parentHash": format!("0x{:x}", block.parent_hash),
        "timestamp": format!("0x{:x}", block.timestamp),
        "gasLimit": format!("0x{:x}", block.gas_limit),
        "gasUsed": format!("0x{:x}", block.gas_used),
        "transactions": transactions,
        "nonce": format!("0x{:x}", block.nonce)
    })
}

fn handle_get_transaction_receipt(params: &Value, server: &Arc<RpcServer>) -> Value {
    json!(null)
}

fn handle_eth_accounts() -> Value {
    json!([
        "0x1111111111111111111111111111111111111111",
        "0x6464646464646464646464646464646464646464"
    ])
}

async fn mine_pending_transactions(server: &Arc<RpcServer>) {
    let transactions = {
        let mut pending = server.pending_transactions.lock().unwrap();
        let txs = pending.clone();
        pending.clear();
        txs
    };

    if !transactions.is_empty() {
        println!("Auto-mining {} pending transactions...", transactions.len());

        let result = {
            let mut blockchain = server.blockchain.lock().unwrap();
            server.miner.mine_block(&mut blockchain, transactions, 2)
        };

        match result {
            Ok(_) => println!("Block mined successfully!"),
            Err(e) => println!("Mining failed: {}", e),
        }
    }
}

// Helper functions
fn parse_address(addr_str: &str) -> Address {
    let addr_str = addr_str.trim_start_matches("0x");
    if addr_str.len() == 40 {
        Address::from_slice(&hex::decode(addr_str).unwrap_or_default())
    } else {
        Address::zero()
    }
}

fn parse_u256(value_str: &str) -> U256 {
    let value_str = value_str.trim_start_matches("0x");
    U256::from_str_radix(value_str, 16).unwrap_or(U256::zero())
}

fn parse_u64(value_str: &str) -> u64 {
    let value_str = value_str.trim_start_matches("0x");
    u64::from_str_radix(value_str, 16).unwrap_or(0)
}

fn parse_hex_data(data_str: &str) -> Vec<u8> {
    let data_str = data_str.trim_start_matches("0x");
    hex::decode(data_str).unwrap_or_default()
}