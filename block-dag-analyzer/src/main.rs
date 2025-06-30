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
    UiInstruction, UiParsedInstruction,
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
        /// Number of tracks for parallel execution
        #[arg(short, long, default_value_t = 4)]
        tracks: usize,
        /// Output DAG CSV data instead of block summary
        #[arg(long)]
        dag_csv: bool,
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
        /// Number of tracks for parallel execution
        #[arg(short, long, default_value_t = 4)]
        tracks: usize,
        /// Output DAG CSV data instead of block summary
        #[arg(long)]
        dag_csv: bool,
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
    dist_track: Vec<u64>,
    write_acc_tx_map: HashMap<String, usize>,
    read_acc_tx_map: HashMap<String, usize>,
    empty_cus: u64,
    tx_track_assignments: Vec<Option<(usize, u64, u64)>>, // indexed by tx.index: (track_idx, start_cus, end_cus)
    edges: Vec<Vec<usize>>, // edges[i] = list of parent transaction indices for transaction i
    tx_compute_units: Vec<u64>, // original compute units for each transaction
}

impl BlockDAG {
    fn new(num_txs: usize, num_tracks: usize) -> Self {
        Self {
            dist_tx: vec![0; num_txs],
            dist_tx_min: vec![0; num_txs],
            dist_track: vec![0; num_tracks],
            write_acc_tx_map: HashMap::new(),
            read_acc_tx_map: HashMap::new(),
            empty_cus: 0,
            tx_track_assignments: vec![None; num_txs],
            edges: vec![Vec::new(); num_txs],
            tx_compute_units: vec![0; num_txs],
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
        
        // Store edges and compute units for longest path computation
        self.edges[tx.index] = parents.clone();
        self.tx_compute_units[tx.index] = tx.compute_units.unwrap_or(0);

        // find parent with max distance
        let max_dist = parents.iter().map(|p| self.dist_tx[*p]).max().unwrap_or(0);
        let max_dist_min = parents.iter().map(|p| self.dist_tx_min[*p]).max().unwrap_or(0);

        // update dist_tx_min
        self.dist_tx_min[tx.index] = max_dist_min + tx.compute_units.unwrap_or(0);
        
        // if tracks exists that are shorter than max_dist, pick the largest one. if no such track exists, pick the shortest track
        let track_idx = self.dist_track.iter()
            .enumerate()
            .filter(|(_, &d)| d <= max_dist)
            .max_by_key(|(_, &d)| d)
            .map(|(idx, _)| idx)
            .unwrap_or_else(|| {
                self.dist_track.iter()
                    .enumerate()
                    .min_by_key(|(_, &d)| d)
                    .map(|(idx, _)| idx)
                    .unwrap()
            });

        let start_cus = max(max_dist, self.dist_track[track_idx]);
        let tx_cus = tx.compute_units.unwrap_or(0);
        let end_cus = start_cus + tx_cus;

        // Record transaction assignment using tx.index
        self.tx_track_assignments[tx.index] = Some((track_idx, start_cus, end_cus));

        self.empty_cus += max_dist.saturating_sub(self.dist_track[track_idx]);
        self.dist_tx[tx.index] = end_cus;
        self.dist_track[track_idx] = end_cus;
    }

    fn get_max_dist(&self) -> u64 {
        *self.dist_tx.iter().max().unwrap_or(&0)
    }

    fn find_longest_path(&self) -> Vec<bool> {
        let mut on_critical_path = vec![false; self.dist_tx_min.len()];
        
        if self.dist_tx_min.is_empty() {
            return on_critical_path;
        }
        
        // Find the transaction with maximum dist_tx_min (end of longest path)
        let max_dist_min = *self.dist_tx_min.iter().max().unwrap_or(&0);
        if max_dist_min == 0 {
            return on_critical_path;
        }
        
        // Choose the FIRST transaction that achieves the maximum distance
        let end_tx = self.dist_tx_min.iter()
            .position(|&dist| dist == max_dist_min)
            .unwrap_or(0);
        
        // Simple traversal: follow one path backwards
        let mut current_tx = end_tx;
        
        loop {
            //println!("current_tx: {}", current_tx);
            //println!("dist_tx_min: {:?}", self.dist_tx_min[current_tx]);
            on_critical_path[current_tx] = true;
            
            // Find the first parent that satisfies the longest path condition
            let current_dist = self.dist_tx_min[current_tx];
            let current_cu = self.tx_compute_units[current_tx];
            
            let mut found_parent = None;
            for &parent_idx in &self.edges[current_tx] {
                let parent_dist = self.dist_tx_min[parent_idx];
                if parent_dist + current_cu == current_dist {
                    found_parent = Some(parent_idx);
                    break; // Take the first valid parent
                }
            }
            
            match found_parent {
                Some(parent_idx) => current_tx = parent_idx,
                None => break, // No more parents on the longest path
            }
        }

        //println!("count: {}", on_critical_path.iter().filter(|&b| *b).count());
        
        on_critical_path
    }

    fn get_dag_csv_rows(&self) -> String {
        let longest_path = self.find_longest_path();
        let mut csv = String::new();
        for (tx_index, assignment) in self.tx_track_assignments.iter().enumerate() {
            if let Some((track_idx, start_cus, end_cus)) = assignment {
                let on_critical_path = longest_path.get(tx_index).unwrap_or(&false);
                csv.push_str(&format!("{},{},{},{}\n", track_idx, start_cus, end_cus, on_critical_path));
            }
        }
        csv
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

fn is_vote_transaction(transaction: &EncodedTransactionWithStatusMeta) -> bool {
    const VOTE_PROGRAM_ID: &str = "Vote111111111111111111111111111111111111111";
    
    let Some(message) = extract_message_from_transaction(&transaction.transaction) else { return false };
    let UiMessage::Parsed(parsed_message) = message else { return false };
    
    parsed_message.instructions.iter().any(|instruction| {
        match instruction {
            UiInstruction::Parsed(UiParsedInstruction::Parsed(parsed)) => 
                parsed.program_id == VOTE_PROGRAM_ID,
            UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(partially_decoded)) => 
                partially_decoded.program_id == VOTE_PROGRAM_ID,
            UiInstruction::Compiled(_) => false, // Could be enhanced to resolve program IDs
        }
    })
}

fn process_transaction(idx: usize, transaction: &EncodedTransactionWithStatusMeta) -> Option<TransactionNode> {
    // Filter out vote transactions
    if is_vote_transaction(transaction) {
        return None;
    }
    
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

fn analyze_block(block: &UiConfirmedBlock, tracks: usize) -> Result<(u64, u64, u64, Vec<u64>, u64, BlockDAG)> {
    let total_cus = block.transactions.as_ref().unwrap().iter()
        .flat_map(|t| t.meta.as_ref())
        .filter_map(|meta| match &meta.compute_units_consumed {
            OptionSerializer::Some(cu) => Some(*cu),
            _ => None,
        })
        .sum::<u64>();

    let mut block_dag = BlockDAG::new(block.transactions.as_ref().unwrap().len(), tracks);

    // Process all transactions functionally
    block.transactions
        .as_ref()
        .unwrap()
        .iter()
        .enumerate()
        .filter_map(|(idx, tx)| process_transaction(idx, tx))
        .for_each(|tx_node| block_dag.add_tx(&tx_node));

    let longest_path = *block_dag.dist_tx_min.iter().max().unwrap();
    let track_lengths = block_dag.dist_track.clone();
    let empty_cus = block_dag.empty_cus;
    let max_dist = block_dag.get_max_dist();
    
    Ok((total_cus, max_dist, longest_path, track_lengths, empty_cus, block_dag))
}

async fn analyze_single_block(slot: Slot, rpc_url: &str, tracks: usize, dag_csv: bool) -> Result<()> {
    let rpc_client = RpcClient::new(rpc_url.to_string());
    let block = fetch_block(&rpc_client, slot).await?;
    let (total_cus, max_dist, longest_path, track_lengths, empty_cus, block_dag) = analyze_block(&block, tracks)?;

    // skip if total CUs is less than 40M
    if total_cus < 40_000_000 {
        return Ok(());
    }
    
    if dag_csv {
        println!("{}", block_dag.get_dag_csv_rows());
    } else {
        // Format output with separate columns for each track
        let mut output = format!("{},{},{},{}", slot, total_cus, max_dist, longest_path);
        for track_length in &track_lengths {
            output.push_str(&format!(",{}", track_length));
        }
        output.push_str(&format!(",{}", empty_cus));
        
        println!("{}", output);
    }
    Ok(())
}

fn print_csv_header(tracks: usize) {
    let mut header = "slot,total_cus,max_dist_cus,longest_path_cus".to_string();
    for i in 0..tracks {
        header.push_str(&format!(",track{}_cus", i));
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
            tracks,
            dag_csv,
        } => {
            if dag_csv {
                println!("track,start_cus,end_cus,on_critical_path");
            } else {
                print_csv_header(tracks);
            }
            analyze_single_block(slot, &rpc_url, tracks, dag_csv).await?;
        }
        Commands::Range {
            start_slot,
            end_slot,
            rpc_url,
            tracks,
            dag_csv,
        } => {
            if dag_csv {
                println!("track,start_cus,end_cus,on_critical_path");
                for slot in start_slot..=end_slot {
                    analyze_single_block(slot, &rpc_url, tracks, dag_csv).await?;
                }
            } else {
                print_csv_header(tracks);
                for slot in start_slot..=end_slot {
                    analyze_single_block(slot, &rpc_url, tracks, dag_csv).await?;
                }
            }
        }
    }

    Ok(())
} 