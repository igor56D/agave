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
    cmp::{max, Ordering, Reverse}, collections::{HashMap, BinaryHeap}
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
        /// Number of vote tracks for simple vote transactions
        #[arg(long, default_value_t = 2)]
        vote_tracks: usize,
        /// Batch size for transaction assignment (1 = immediate assignment)
        #[arg(short, long, default_value_t = 1)]
        batch_size: usize,
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
        /// Number of vote tracks for simple vote transactions
        #[arg(long, default_value_t = 2)]
        vote_tracks: usize,
        /// Batch size for transaction assignment (1 = immediate assignment)
        #[arg(short, long, default_value_t = 1)]
        batch_size: usize,
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
    is_simple_vote: bool,
}

#[derive(Debug, Clone)]
struct BatchedTransaction {
    tx: TransactionNode,
    min_start_time: u64,
    parents: Vec<usize>,
}

impl PartialEq for BatchedTransaction {
    fn eq(&self, other: &Self) -> bool {
        self.min_start_time == other.min_start_time
    }
}

impl Eq for BatchedTransaction {}

impl PartialOrd for BatchedTransaction {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BatchedTransaction {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap, so to get min-heap behavior (smallest start_time first),
        // we reverse the comparison: smaller start_time should be "greater" in priority
        other.min_start_time.cmp(&self.min_start_time)
    }
}

struct BlockDAG {
    len_tx_final: Vec<u64>, // CU path after assignment
    crit_len_tx: Vec<u64>, // CU path before assignment
    len_track: Vec<u64>, // tip CUs of each track
    len_vote_track: Vec<u64>, // tip CUs of each vote track
    write_acc_tx_map: HashMap<String, usize>, // map from account to transaction index
    read_acc_tx_map: HashMap<String, usize>, // map from account to transaction index
    empty_cus: u64, // empty/idle CUs in tracks
    tx_track_assignments: Vec<Option<(usize, u64, u64, bool)>>, // indexed by tx.index: (track_idx, start_cus, end_cus, is_simple_vote)
    edges: Vec<Vec<usize>>, // edges[i] = list of parent transaction indices for transaction i
    tx_compute_units: Vec<u64>, // original compute units for each transaction
    // Batching fields
    batch_size: usize,
    pending_batch: BinaryHeap<BatchedTransaction>,
}

impl BlockDAG {
    fn new(num_txs: usize, num_tracks: usize, num_vote_tracks: usize, batch_size: usize) -> Self {
        Self {
            len_tx_final: vec![0; num_txs],
            crit_len_tx: vec![0; num_txs],
            len_track: vec![0; num_tracks],
            len_vote_track: vec![0; num_vote_tracks],
            write_acc_tx_map: HashMap::new(),
            read_acc_tx_map: HashMap::new(),
            empty_cus: 0,
            tx_track_assignments: vec![None; num_txs],
            edges: vec![Vec::new(); num_txs],
            tx_compute_units: vec![0; num_txs],
            batch_size,
            pending_batch: BinaryHeap::new(),
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

    fn calculate_min_start_time(&self, parents: &[usize]) -> u64 {
        parents.iter().map(|p| self.crit_len_tx[*p]).max().unwrap_or(0)
    }

    fn assign_transaction_to_track(&mut self, batched_tx: BatchedTransaction) {
        let tx = &batched_tx.tx;
        // get max of parents in self.len_tx_final
        let min_start_time = batched_tx.parents.iter().map(|p| self.len_tx_final[*p]).max().unwrap_or(0);
        
        // if tracks exists that are shorter than min_start_time, pick the largest one. if no such track exists, pick the shortest track
        let len_tracks = if tx.is_simple_vote {
            &mut self.len_vote_track
        } else {
            &mut self.len_track
        };

        let track_idx = len_tracks.iter()
            .enumerate()
            .filter(|(_, &d)| d <= min_start_time)
            .max_by_key(|(_, &d)| d)
            .map(|(idx, _)| idx)
            .unwrap_or_else(|| {
                len_tracks.iter()
                    .enumerate()
                    .min_by_key(|(_, &d)| d)
                    .map(|(idx, _)| idx)
                    .unwrap()
            });

        let start_cus = max(min_start_time, len_tracks[track_idx]);
        let tx_cus = tx.compute_units.unwrap_or(0);
        let end_cus = start_cus + tx_cus;

        // Record transaction assignment using tx.index
        self.tx_track_assignments[tx.index] = Some((track_idx, start_cus, end_cus, tx.is_simple_vote));

        self.empty_cus += min_start_time.saturating_sub(len_tracks[track_idx]);
        self.len_tx_final[tx.index] = end_cus;
        len_tracks[track_idx] = end_cus;
    }

    fn process_pending_batch(&mut self) {
        while let Some(batched_tx) = self.pending_batch.pop() {
            self.assign_transaction_to_track(batched_tx);
        }
    }

    fn add_tx(&mut self, tx: &TransactionNode) {
        let parents = self.get_parents(tx);
        self.crit_len_tx[tx.index] = self.calculate_min_start_time(&parents) + tx.compute_units.unwrap_or(0);

        // Store edges and compute units for longest path computation
        self.edges[tx.index] = parents.clone();
        self.tx_compute_units[tx.index] = tx.compute_units.unwrap_or(0);

        let batched_tx = BatchedTransaction {
            tx: tx.clone(),
            min_start_time: self.crit_len_tx[tx.index],
            parents,
        };

        // Add to batch
        self.pending_batch.push(batched_tx);
        
        // Process batch when it reaches batch_size
        if self.pending_batch.len() >= self.batch_size {
            self.process_pending_batch();
        }
    }

    fn finalize(&mut self) {
        // Process any remaining transactions in the batch
        self.process_pending_batch();
    }

    fn get_max_dist(&self) -> u64 {
        *self.len_tx_final.iter().max().unwrap_or(&0)
    }

    fn find_longest_path(&self) -> Vec<bool> {
        let mut on_critical_path = vec![false; self.crit_len_tx.len()];
        
        if self.crit_len_tx.is_empty() {
            return on_critical_path;
        }
        
        // Find the transaction with maximum crit_len_tx (end of longest path)
        let crit_len = *self.crit_len_tx.iter().max().unwrap_or(&0);
        if crit_len == 0 {
            return on_critical_path;
        }
        
        // Choose the FIRST transaction that achieves the maximum distance
        let end_tx = self.crit_len_tx.iter()
            .position(|&dist| dist == crit_len)
            .unwrap_or(0);
        
        // Simple traversal: follow one path backwards
        let mut current_tx = end_tx;
        
        loop {
            on_critical_path[current_tx] = true;
            
            // Find the first parent that satisfies the longest path condition
            let current_dist = self.crit_len_tx[current_tx];
            let current_cu = self.tx_compute_units[current_tx];
            
            let mut found_parent = None;
            for &parent_idx in &self.edges[current_tx] {
                let parent_dist = self.crit_len_tx[parent_idx];
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
            if let Some((track_idx, start_cus, end_cus, is_simple_vote)) = assignment {
                let on_critical_path = longest_path.get(tx_index).unwrap_or(&false);
                csv.push_str(&format!("{},{},{},{},{}\n", track_idx, start_cus, end_cus, on_critical_path, is_simple_vote));
            }
        }
        csv
    }
}

impl TransactionNode {
    fn new(index: usize, is_simple_vote: bool) -> Self {
        Self {
            index,
            accounts_read: Vec::new(),
            accounts_written: Vec::new(),
            all_accounts: Vec::new(),
            program_id: None,
            compute_units: None,
            is_simple_vote,
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

fn is_simple_vote_transaction(transaction: &EncodedTransactionWithStatusMeta) -> bool {
    const VOTE_PROGRAM_ID: &str = "Vote111111111111111111111111111111111111111";
    
    let Some(message) = extract_message_from_transaction(&transaction.transaction) else { return false };
    let UiMessage::Parsed(parsed_message) = message else { return false };
    
    // Must be legacy message only (not versioned)
    // In JSON parsed format, versioned messages would be handled differently
    // This check ensures we're dealing with legacy transactions
    
    // Must have 1 or 2 signatures only - check from the transaction itself
    let signature_count = match &transaction.transaction {
        EncodedTransaction::Json(ui_tx) => ui_tx.signatures.len(),
        _ => return false, // Only handle JSON encoded transactions
    };
    if signature_count == 0 || signature_count > 2 {
        return false;
    }
    
    // Must have exactly 1 instruction
    if parsed_message.instructions.len() != 1 {
        return false;
    }
    
    // The single instruction must be a vote instruction
    let instruction = &parsed_message.instructions[0];
    match instruction {
        UiInstruction::Parsed(UiParsedInstruction::Parsed(parsed)) => 
            parsed.program_id == VOTE_PROGRAM_ID,
        UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(partially_decoded)) => 
            partially_decoded.program_id == VOTE_PROGRAM_ID,
        UiInstruction::Compiled(_) => false, // Could be enhanced to resolve program IDs
    }
}

fn process_transaction(idx: usize, transaction: &EncodedTransactionWithStatusMeta) -> Option<TransactionNode> {
    let tx_meta = transaction.meta.as_ref()?;
    let mut tx_node = TransactionNode::new(idx, is_simple_vote_transaction(transaction));
    
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

fn analyze_block(block: &UiConfirmedBlock, tracks: usize, vote_tracks: usize, batch_size: usize) -> Result<(u64, u64, BlockDAG)> {
    let total_cus = block.transactions.as_ref().unwrap().iter()
        .flat_map(|t| t.meta.as_ref())
        .filter_map(|meta| match &meta.compute_units_consumed {
            OptionSerializer::Some(cu) => Some(*cu),
            _ => None,
        })
        .sum::<u64>();

    let mut block_dag = BlockDAG::new(block.transactions.as_ref().unwrap().len(), tracks, vote_tracks, batch_size);

    block.transactions
        .as_ref()
        .unwrap()
        .iter()
        .enumerate()
        .filter_map(|(idx, tx)| process_transaction(idx, tx))
        .for_each(|tx_node| block_dag.add_tx(&tx_node));

    // Finalize to process any remaining batched transactions
    block_dag.finalize();

    let crit_path = *block_dag.crit_len_tx.iter().max().unwrap();
    Ok((total_cus, crit_path, block_dag))
}

async fn analyze_single_block(slot: Slot, rpc_url: &str, tracks: usize, vote_tracks: usize, batch_size: usize, dag_csv: bool) -> Result<()> {
    let rpc_client = RpcClient::new(rpc_url.to_string());
    let block = fetch_block(&rpc_client, slot).await?;
    let (total_cus, crit_path,block_dag) = analyze_block(&block, tracks, vote_tracks, batch_size)?;

    // skip if total CUs is less than 40M
    if total_cus < 40_000_000 {
        return Ok(());
    }
    
    if dag_csv {
        println!("{}", block_dag.get_dag_csv_rows());
    } else {
        // Format output with separate columns for each track
        let mut output = format!("{},{},{},{}", slot, total_cus, block_dag.get_max_dist(), crit_path);
        for track_length in &block_dag.len_track {
            output.push_str(&format!(",{}", track_length));
        }
        for track_length in &block_dag.len_vote_track {
            output.push_str(&format!(",{}", track_length));
        }
        output.push_str(&format!(",{}", block_dag.empty_cus));
        
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
            vote_tracks,
            batch_size,
            dag_csv,
        } => {
            if dag_csv {
                println!("track,start_cus,end_cus,on_critical_path,is_simple_vote");
            } else {
                print_csv_header(tracks);
            }
            analyze_single_block(slot, &rpc_url, tracks, vote_tracks, batch_size, dag_csv).await?;
        }
        Commands::Range {
            start_slot,
            end_slot,
            rpc_url,
            tracks,
            vote_tracks,
            batch_size,
            dag_csv,
        } => {
            if dag_csv {
                println!("track,start_cus,end_cus,on_critical_path,is_simple_vote");
                for slot in start_slot..=end_slot {
                    analyze_single_block(slot, &rpc_url, tracks, vote_tracks, batch_size, dag_csv).await?;
                }
            } else {
                print_csv_header(tracks);
                for slot in start_slot..=end_slot {
                    analyze_single_block(slot, &rpc_url, tracks, vote_tracks, batch_size, dag_csv).await?;
                }
            }
        }
    }

    Ok(())
} 