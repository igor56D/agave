use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use solana_clock::Slot;
use solana_commitment_config::CommitmentConfig;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcBlockConfig;
use solana_transaction_status_client_types::{
    TransactionDetails, UiConfirmedBlock, UiTransactionEncoding, EncodedTransaction, 
    UiMessage, option_serializer::OptionSerializer,
    EncodedTransactionWithStatusMeta,
};
use std::{
    cmp::max, collections::HashMap
};
use tokio;

#[derive(Parser)]
#[command(name = "solana-block-dag-analyzer")]
#[command(about = "Analyzes Solana blocks and builds DAG from transaction account access patterns")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch a specific block and build DAG
    Block {
        /// Block slot number
        #[arg(short, long)]
        slot: Slot,
        /// RPC endpoint URL
        #[arg(short, long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
        /// Number of chains for parallel execution
        #[arg(short, long, default_value_t = 4)]
        chains: usize,
    },
    /// Fetch a range of blocks and build DAG
    Range {
        /// Starting slot number
        #[arg(short, long)]
        start_slot: Slot,
        /// Ending slot number
        #[arg(short, long)]
        end_slot: Slot,
        /// RPC endpoint URL  
        #[arg(short, long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
        /// Number of chains for parallel execution
        #[arg(short, long, default_value_t = 4)]
        chains: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransactionNode {
    index: usize,
    accounts_read: Vec<String>,
    accounts_written: Vec<String>,
    all_accounts: Vec<String>,
    program_id: Option<String>,
    compute_units: Option<u64>,
}

struct BlockDAG {
    dist_tx: Vec<u64>,
    dist_tx_min: Vec<u64>,
    dist_chain: Vec<u64>,
    write_acc_tx_map: HashMap<String, usize>,
    read_acc_tx_map: HashMap<String, usize>,
    empty_cus: u64,
}

impl BlockDAG {
    fn new(num_txs: usize, num_chains: usize) -> Self {
        Self {
            dist_tx: vec![0; num_txs],
            dist_tx_min: vec![0; num_txs],
            dist_chain: vec![0; num_chains],
            write_acc_tx_map: HashMap::new(),
            read_acc_tx_map: HashMap::new(),
            empty_cus: 0,
        }
    }

    fn get_parents(&mut self, tx: &TransactionNode) -> Vec<usize> {
        let write_parents: Vec<usize> = tx.accounts_read
            .iter()
            .chain(&tx.accounts_written)
            .filter_map(|acc| self.write_acc_tx_map.get(acc).copied())
            .collect();

        // write accounts additionally depend on previous reads
        let read_parents: Vec<usize> = tx.accounts_written
            .iter()
            .filter_map(|acc| self.read_acc_tx_map.get(acc).copied())
            .collect();

        // update acc_tx_maps
        tx.accounts_read.iter().for_each(|acc| {
            self.read_acc_tx_map.insert(acc.clone(), tx.index);
        });
        tx.accounts_written.iter().for_each(|acc| {
            self.write_acc_tx_map.insert(acc.clone(), tx.index);
        });

        write_parents.into_iter().chain(read_parents).collect()
    }

    fn add_tx(&mut self, tx: &TransactionNode) {
        let parents = self.get_parents(tx);

        // find parent with max distance
        let max_dist = parents.iter().map(|p| self.dist_tx[*p]).max().unwrap_or(0);
        let max_dist_min = parents.iter().map(|p| self.dist_tx_min[*p]).max().unwrap_or(0);

        // update dist_tx_min
        self.dist_tx_min[tx.index] = max_dist_min + tx.compute_units.unwrap_or(0);
        
        // if chains exists that are shorter than max_dist, pick the largest one. if no such chain exists, pick the shortest chain
        let chain_idx = self.dist_chain.iter()
            .enumerate()
            .filter(|(_, &d)| d < max_dist)
            .max_by_key(|(_, &d)| d)
            .map(|(idx, _)| idx)
            .unwrap_or_else(|| {
                self.dist_chain.iter()
                    .enumerate()
                    .min_by_key(|(_, &d)| d)
                    .map(|(idx, _)| idx)
                    .unwrap()
            });

        self.empty_cus += max_dist.saturating_sub(self.dist_chain[chain_idx]);
        self.dist_tx[tx.index] = max(max_dist, self.dist_chain[chain_idx]) + tx.compute_units.unwrap_or(0);
        self.dist_chain[chain_idx] = self.dist_tx[tx.index];
    }

    fn get_max_dist(&self) -> u64 {
        *self.dist_tx.iter().max().unwrap_or(&0)
    }
}

impl TransactionNode {
    fn new(index: usize) -> Self {
        Self {
            index,
            accounts_read: Vec::new(),
            accounts_written: Vec::new(),
            all_accounts: Vec::new(),
            program_id: None,
            compute_units: None,
        }
    }
}

async fn fetch_block(rpc_client: &RpcClient, slot: Slot) -> Result<UiConfirmedBlock> {
    let config = RpcBlockConfig {
        encoding: Some(UiTransactionEncoding::JsonParsed),
        transaction_details: Some(TransactionDetails::Full),
        rewards: Some(false),
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(0),
    };

    rpc_client
        .get_block_with_config(slot, config)
        .await
        .with_context(|| format!("Failed to fetch block at slot {}", slot))
}

fn extract_accounts_from_message(message: &UiMessage) -> (Vec<String>, Vec<String>) {
    match message {
        UiMessage::Parsed(parsed_message) => {
            let mut written = Vec::new();
            let mut read = Vec::new();
            
            for account in &parsed_message.account_keys {
                if account.writable {
                    written.push(account.pubkey.clone());
                } else {
                    read.push(account.pubkey.clone());
                }
            }

            (written, read)
        }
        _ => (Vec::new(), Vec::new()),
    }
}

fn extract_message_from_transaction(transaction: &EncodedTransaction) -> Option<&UiMessage> {
    match transaction {
        EncodedTransaction::Json(ui_tx) => Some(&ui_tx.message),
        _ => None,
    }
}

fn process_transaction(idx: usize, transaction: &EncodedTransactionWithStatusMeta) -> Option<TransactionNode> {
    let tx_meta = transaction.meta.as_ref()?;
    let mut tx_node = TransactionNode::new(idx);
    
    // Extract accounts
    if let Some(message) = extract_message_from_transaction(&transaction.transaction) {
        let (written, read) = extract_accounts_from_message(message);
        tx_node.accounts_written = written;
        tx_node.accounts_read = read;
    }
    
    // Extract compute units
    if let OptionSerializer::Some(compute_units) = &tx_meta.compute_units_consumed {
        tx_node.compute_units = Some(*compute_units);
    }
    
    Some(tx_node)
}

fn analyze_block(block: &UiConfirmedBlock, chains: usize) -> Result<(u64, u64, u64, Vec<u64>, u64)> {
    let total_cus = block.transactions.as_ref().unwrap().iter()
        .flat_map(|t| t.meta.as_ref())
        .filter_map(|meta| match &meta.compute_units_consumed {
            OptionSerializer::Some(cu) => Some(*cu),
            _ => None,
        })
        .sum::<u64>();

    let mut block_dag = BlockDAG::new(block.transactions.as_ref().unwrap().len(), chains);

    // Process all transactions functionally
    block.transactions
        .as_ref()
        .unwrap()
        .iter()
        .enumerate()
        .filter_map(|(idx, tx)| process_transaction(idx, tx))
        .for_each(|tx_node| block_dag.add_tx(&tx_node));

    let longest_path = *block_dag.dist_tx_min.iter().max().unwrap();
    let chain_lengths = block_dag.dist_chain.clone();
    let empty_cus = block_dag.empty_cus;
    
    Ok((total_cus, block_dag.get_max_dist(), longest_path, chain_lengths, empty_cus))
}

async fn analyze_single_block(slot: Slot, rpc_url: &str, chains: usize) -> Result<()> {
    let rpc_client = RpcClient::new(rpc_url.to_string());
    let block = fetch_block(&rpc_client, slot).await?;
    let (total_cus, max_dist, longest_path, chain_lengths, empty_cus) = analyze_block(&block, chains)?;
    
    // Format output with separate columns for each chain
    let mut output = format!("{},{},{},{}", slot, total_cus, max_dist, longest_path);
    for chain_length in &chain_lengths {
        output.push_str(&format!(",{}", chain_length));
    }
    output.push_str(&format!(",{}", empty_cus));
    
    println!("{}", output);
    Ok(())
}

fn print_csv_header(chains: usize) {
    let mut header = "slot,total_cus,max_dist_cus,longest_path_cus".to_string();
    for i in 0..chains {
        header.push_str(&format!(",chain{}_cus", i));
    }
    header.push_str(",empty_cus");
    println!("{}", header);
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Block {
            slot,
            rpc_url,
            chains,
        } => {
            print_csv_header(chains);
            analyze_single_block(slot, &rpc_url, chains).await?;
        }
        Commands::Range {
            start_slot,
            end_slot,
            rpc_url,
            chains,
        } => {
            print_csv_header(chains);
            for slot in start_slot..=end_slot {
                analyze_single_block(slot, &rpc_url, chains).await?;
            }
        }
    }

    Ok(())
} 