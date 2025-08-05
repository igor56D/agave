mod snapshot_parser;

use {
    anyhow::{Context, Result},
    clap::{Parser, Subcommand},
    csv::Writer,
    dashmap::DashMap,
    log::*,
    rayon::prelude::*,
    reqwest::Client,
    serde::{Deserialize, Serialize},
    serde_json::Value,
    snapshot_parser::SnapshotParser,
    solana_clock::Slot,
    solana_pubkey::Pubkey,
    std::{
        collections::HashSet,
        fs::File,
        io::{BufReader, BufWriter},
        path::{Path, PathBuf}, time::Duration,
    },
};

#[cfg(not(any(target_env = "msvc", target_os = "freebsd")))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

// Solana mainnet constants
const SOLANA_SLOTS_PER_SECOND: f64 = 2.5;
const SECONDS_PER_MONTH: f64 = 30.0 * 24.0 * 60.0 * 60.0;

// Performance optimizations:
// - Uses DashMap for thread-safe concurrent HashMap operations without full locking
// - Uses rayon for parallel processing of snapshot accounts and size calculations
// - Account map building and total size calculation run in parallel
// - Index processing remains sequential to maintain time boundary order

#[derive(Parser)]
#[command(name = "agave-snapshot-analyzer")]
#[command(about = "Analyze Solana snapshot account staleness and block data usage")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze account staleness using pre-built access index
    Staleness {
        /// Path to the snapshot archive file
        #[arg(long, short = 's')]
        snapshot: PathBuf,
        
        /// Path to the bincode account access index file
        #[arg(long, short = 'i')]
        index: PathBuf,
        
        /// Current slot number (auto-detected from filename if not provided)
        #[arg(long)]
        current_slot: Option<Slot>,
        
        /// Number of threads for parallel processing
        #[arg(long, short = 'j')]
        threads: Option<usize>,
    },
    /// Analyze bytes loaded per block in a slot range
    BlockUsage {
        /// Path to the snapshot archive file
        #[arg(long, short = 's')]
        snapshot: PathBuf,
        
        /// Starting slot number
        #[arg(long)]
        start_slot: Slot,
        
        /// Ending slot number
        #[arg(long)]
        end_slot: Slot,
        
        /// RPC endpoint URL
        #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
        
        /// Output CSV file path
        #[arg(long, short = 'o', default_value = "block-usage.csv")]
        output: PathBuf,
        
        /// Number of threads for parallel processing
        #[arg(long, short = 'j')]
        threads: Option<usize>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSlotEntry {
    pub account: Pubkey,
    pub highest_slot: u64,
}

#[derive(Debug)]
struct StalenessCheckpoint {
    months: u32,
    boundary_slot: u64,
    total_size_bytes: u64,
    total_account_count: u64,
}

impl StalenessCheckpoint {
    fn print(&self) {
        println!("{} months ago (slot {}): {} accounts, {:.2} GB",
                 self.months,
                 self.boundary_slot,
                 self.total_account_count,
                 self.total_size_bytes as f64 / 1_000_000_000.0);
    }
}

fn calculate_slot_boundaries(current_slot: Slot) -> [Slot; 4] {
    let slots_per_month = (SECONDS_PER_MONTH * SOLANA_SLOTS_PER_SECOND) as u64;
    [
        current_slot.saturating_sub(slots_per_month),     // 1 month
        current_slot.saturating_sub(2 * slots_per_month), // 2 months
        current_slot.saturating_sub(4 * slots_per_month), // 4 months
        current_slot.saturating_sub(8 * slots_per_month), // 8 months
    ]
}

fn load_account_access_index(index_path: &Path) -> Vec<AccountSlotEntry> {
    info!("Loading account access index from: {}", index_path.display());
    
    let file = match File::open(index_path) {
        Ok(file) => file,
        Err(e) => {
            warn!("Failed to open index file: {}. Using empty index.", e);
            return Vec::new();
        }
    };
    
    let reader = BufReader::new(file);
    let entries: Vec<AccountSlotEntry> = match bincode::deserialize_from(reader) {
        Ok(entries) => entries,
        Err(e) => {
            warn!("Failed to deserialize account access index: {}. Using empty index.", e);
            return Vec::new();
        }
    };
    
    info!("Loaded {} account access entries", entries.len());
    
    // Verify the entries are sorted from high to low slot
    if let Some(windows) = entries.windows(2).find(|w| w[0].highest_slot < w[1].highest_slot) {
        warn!("Index may not be properly sorted: slot {} follows slot {}", 
              windows[0].highest_slot, windows[1].highest_slot);
    }

    entries
}

fn analyze_account_staleness_with_accounts(
    snapshot_accounts: Vec<snapshot_parser::AccountInfo>,
    index_path: &Path,
    current_slot: Slot,
) -> Result<Vec<StalenessCheckpoint>> {
    info!("Starting account staleness analysis");
    info!("Index: {}", index_path.display());
    info!("Current slot: {}", current_slot);

    // Step 1: Build account map from pre-loaded snapshot accounts
    info!("Building account map from {} accounts using parallel processing...", snapshot_accounts.len());
    let start = std::time::Instant::now();
    let account_map: DashMap<Pubkey, u64> = DashMap::new();
    
    // Process accounts in parallel using DashMap for thread-safe concurrent insertions
    snapshot_accounts.par_iter().for_each(|account| {
        account_map.insert(account.pubkey, account.data_len);
    });

    info!("Built account map with {} entries in {:.2}s", 
          account_map.len(), start.elapsed().as_secs_f64());

    // Step 2: Load account access index
    let access_entries = load_account_access_index(index_path);

    // Step 3: Calculate time boundaries
    let slot_boundaries = calculate_slot_boundaries(current_slot);
    info!("Time boundaries (slots): {:?}", slot_boundaries);
    
    let boundary_months = [1, 2, 4, 8];
    
    // Step 4: Process entries and track checkpoints
    let mut checkpoints = Vec::new();
    let mut total_size = 0u64;
    let mut total_account_count = 0u64;
    let mut boundary_index = 0;

    info!("Processing {} access entries...", access_entries.len());

    for (i, entry) in access_entries.iter().enumerate() {
        // Check if account exists in snapshot
        if let Some(account_size_ref) = account_map.get(&entry.account) {
            total_size += *account_size_ref;
            total_account_count += 1;
        }
        
        // Check if we've crossed any time boundaries
        while boundary_index < slot_boundaries.len() && 
              entry.highest_slot <= slot_boundaries[boundary_index] {
            
            let checkpoint = StalenessCheckpoint {
                months: boundary_months[boundary_index],
                boundary_slot: slot_boundaries[boundary_index],
                total_size_bytes: total_size,
                total_account_count,
            };
            
            info!("Checkpoint: {} months - {} GB at slot {} (entry {}/{})",
                  checkpoint.months,
                  checkpoint.total_size_bytes as f64 / 1_000_000_000.0,
                  checkpoint.boundary_slot,
                  i + 1,
                  access_entries.len());
            
            checkpoints.push(checkpoint);
            boundary_index += 1;
        }
        
        // Early exit if we've processed all boundaries
        if boundary_index >= slot_boundaries.len() {
            break;
        }
        
        // Progress reporting
        if (i + 1) % 100_000 == 0 {
            info!("Processed {}/{} entries, current total: {} accounts, {:.2} GB",
                  i + 1, 
                  access_entries.len(),
                  total_account_count,
                  total_size as f64 / 1_000_000_000.0);
        }
    }

    // Add final checkpoint if we haven't reached 8 months yet
    if boundary_index < slot_boundaries.len() {
        let checkpoint = StalenessCheckpoint {
            months: 8,
            boundary_slot: slot_boundaries[3],
            total_size_bytes: total_size,
            total_account_count,
        };
        checkpoints.push(checkpoint);
    }

    info!("Analysis complete. Final total: {} accounts, {:.2} GB", total_account_count, total_size as f64 / 1_000_000_000.0);

    Ok(checkpoints)
}

fn get_snapshot_slot_from_filename(snapshot_path: &Path) -> Option<Slot> {
    let filename = snapshot_path.file_name()?.to_str()?;
    
    // Parse slot from snapshot filename like "snapshot-123456-hash.tar.zst"
    if let Some(start) = filename.find("snapshot-") {
        let after_prefix = &filename[start + 9..]; // "snapshot-".len() = 9
        if let Some(dash_pos) = after_prefix.find('-') {
            let slot_str = &after_prefix[..dash_pos];
            return slot_str.parse::<Slot>().ok();
        }
    }
    
    None
}

fn configure_thread_pool(threads: Option<usize>) -> Result<()> {
    if let Some(num_threads) = threads {
        info!("Configuring rayon thread pool with {} threads", num_threads);
        if let Err(e) = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build_global() {
            warn!("Failed to initialize thread pool: {}. Using default.", e);
        }
    } else {
        let num_threads = rayon::current_num_threads();
        info!("Using default thread pool with {} threads", num_threads);
    }
    Ok(())
}

async fn run_staleness_analysis(
    snapshot: PathBuf,
    index: PathBuf,
    current_slot: Option<Slot>,
    threads: Option<usize>,
) -> Result<()> {
    configure_thread_pool(threads)?;

    if !snapshot.exists() {
        warn!("Snapshot file does not exist: {}. Will proceed with empty account data.", snapshot.display());
    }
    
    if !index.exists() {
        warn!("Index file does not exist: {}. Will proceed with empty index.", index.display());
    }

    // Get current slot from command line or try to parse from filename
    let current_slot = if let Some(slot) = current_slot {
        slot
    } else if let Some(slot) = get_snapshot_slot_from_filename(&snapshot) {
        info!("Using slot from filename: {}", slot);
        slot
    } else {
        warn!("Could not determine current slot. Using default slot 0. Consider providing --current-slot.");
        0
    };

    // Calculate total snapshot size and analyze staleness
    let mut parser = SnapshotParser::new(&snapshot);
    let snapshot_accounts = match parser.parse_accounts() {
        Ok(accounts) => accounts,
        Err(e) => {
            warn!("Failed to parse snapshot accounts: {}. Using empty account list.", e);
            Vec::new()
        }
    };
    
    info!("Calculating total snapshot size using parallel processing...");
    let start = std::time::Instant::now();
    let total_snapshot_size: u64 = snapshot_accounts.par_iter().map(|acc| acc.data_len).sum();
    let total_snapshot_accounts = snapshot_accounts.len() as u64;
    info!("Total snapshot size calculated in {:.2}s", start.elapsed().as_secs_f64());
    
    let checkpoints = match analyze_account_staleness_with_accounts(snapshot_accounts, &index, current_slot) {
        Ok(checkpoints) => checkpoints,
        Err(e) => {
            warn!("Failed to analyze account staleness: {}. Using empty results.", e);
            Vec::new()
        }
    };
    
    println!("\n=== Solana Account Staleness Analysis ===\n");
    println!("Total snapshot: {} accounts, {:.2} GB", total_snapshot_accounts, total_snapshot_size as f64 / 1_000_000_000.0);
    println!("Using pre-built account access index");
    println!("Analysis based on cumulative account sizes by access recency\n");
    
    if checkpoints.is_empty() {
        println!("⚠️  No checkpoints found - this may be due to:");
        println!("   - Missing or empty snapshot file");
        println!("   - Missing or empty index file");
        println!("   - Data processing errors (see warnings above)");
        println!("   - All accounts accessed more recently than 8 months");
    } else {
        println!("Checkpoints (accounts accessed within each time period):");
        println!("{}", "-".repeat(70));
        
        for checkpoint in &checkpoints {
            checkpoint.print();
        }
        
        if checkpoints.len() >= 2 {
            println!("\nGrowth between checkpoints:");
            println!("{}", "-".repeat(50));
            for i in 1..checkpoints.len() {
                let size_growth = checkpoints[i].total_size_bytes - checkpoints[i-1].total_size_bytes;
                let account_growth = checkpoints[i].total_account_count - checkpoints[i-1].total_account_count;
                println!("{} to {} months: +{} accounts, +{:.2} GB",
                         checkpoints[i-1].months,
                         checkpoints[i].months,
                         account_growth,
                         size_growth as f64 / 1_000_000_000.0);
            }
        }
    }

    Ok(())
}

async fn run_block_usage_analysis(
    snapshot: PathBuf,
    start_slot: Slot,
    end_slot: Slot,
    rpc_url: String,
    output: PathBuf,
    threads: Option<usize>,
) -> Result<()> {
    configure_thread_pool(threads)?;

    if start_slot > end_slot {
        anyhow::bail!("Start slot ({}) must be less than or equal to end slot ({})", start_slot, end_slot);
    }

    info!("Starting block usage analysis for slots {} to {}", start_slot, end_slot);
    info!("RPC URL: {}", rpc_url);
    info!("Output file: {}", output.display());

    // Load snapshot accounts
    info!("Loading snapshot data...");
    let mut parser = SnapshotParser::new(&snapshot);
    let snapshot_accounts = match parser.parse_accounts() {
        Ok(accounts) => accounts,
        Err(e) => {
            warn!("Failed to parse snapshot accounts: {}. Using empty account list.", e);
            Vec::new()
        }
    };

    // Build account map for quick lookups
    info!("Building account map from {} accounts using parallel processing...", snapshot_accounts.len());
    let start_time = std::time::Instant::now();
    let account_map: DashMap<Pubkey, u64> = DashMap::new();
    
    snapshot_accounts.par_iter().for_each(|account| {
        account_map.insert(account.pubkey, account.data_len);
    });

    info!("Built account map with {} entries in {:.2}s", 
          account_map.len(), start_time.elapsed().as_secs_f64());

    // Create HTTP client
    let client = Client::new();

    // Create CSV writer
    let output_file = File::create(&output)
        .with_context(|| format!("Failed to create output file: {}", output.display()))?;
    let mut csv_writer = Writer::from_writer(BufWriter::new(output_file));
    
    // Write CSV header
    csv_writer.write_record(&["slot", "bytes_loaded"])?;

    // Process each slot
    let total_slots = end_slot - start_slot + 1;
    let mut processed = 0;

    for slot in start_slot..=end_slot {
        match analyze_block_usage(&client, slot, &account_map, &rpc_url).await {
            Ok(bytes_loaded) => {
                csv_writer.write_record(&[slot.to_string(), bytes_loaded.to_string()])?;
            }
            Err(e) => {
                warn!("Failed to analyze slot {}: {}. Writing 0 bytes.", slot, e);
                csv_writer.write_record(&[slot.to_string(), "0".to_string()])?;
            }
        }

        // Sleep for 1 second
        tokio::time::sleep(Duration::from_secs(1)).await;

        processed += 1;
        if processed % 100 == 0 || processed == total_slots {
            info!("Processed {}/{} slots ({:.1}%)", processed, total_slots, 
                  (processed as f64 / total_slots as f64) * 100.0);
        }
    }

    csv_writer.flush()?;
    info!("Block usage analysis complete. Results written to: {}", output.display());

    Ok(())
}

async fn analyze_block_usage(
    client: &Client,
    slot: Slot,
    account_map: &DashMap<Pubkey, u64>,
    rpc_url: &str,
) -> Result<u64> {
    // Fetch block data from RPC
    let block_data = fetch_block_data(client, slot, rpc_url).await?;
    
    // Extract account references from the block
    let account_refs = extract_account_references(&block_data)?;
    
    // Calculate total bytes loaded
    let mut total_bytes = 0u64;
    for account_key in account_refs {
        if let Some(account_size_ref) = account_map.get(&account_key) {
            total_bytes += *account_size_ref;
        }
    }
    
    Ok(total_bytes)
}

async fn fetch_block_data(client: &Client, slot: Slot, rpc_url: &str) -> Result<Value> {
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBlock",
        "params": [
            slot,
            {
                "encoding": "json",
                "transactionDetails": "full",
                "rewards": false,
                "maxSupportedTransactionVersion": 0
            }
        ]
    });

    let response = client
        .post(rpc_url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .with_context(|| format!("Failed to send RPC request for slot {}", slot))?;

    if !response.status().is_success() {
        anyhow::bail!("RPC request failed with status: {}", response.status());
    }

    let response_json: Value = response.json().await
        .with_context(|| format!("Failed to parse RPC response for slot {}", slot))?;

    if let Some(error) = response_json.get("error") {
        anyhow::bail!("RPC error for slot {}: {}", slot, error);
    }

    response_json.get("result")
        .ok_or_else(|| anyhow::anyhow!("No result in RPC response for slot {}", slot))
        .map(|v| v.clone())
}

fn extract_account_references(block_data: &Value) -> Result<HashSet<Pubkey>> {
    let mut account_refs = HashSet::new();

    // Get transactions from the block
    let transactions = block_data
        .get("transactions")
        .and_then(|t| t.as_array())
        .ok_or_else(|| anyhow::anyhow!("No transactions found in block data"))?;

    for transaction in transactions {
        // Extract account keys from transaction message
        if let Some(message) = transaction.get("transaction").and_then(|t| t.get("message")) {
            if let Some(account_keys) = message.get("accountKeys").and_then(|ak| ak.as_array()) {
                for account_key in account_keys {
                    if let Some(key_str) = account_key.as_str() {
                        if let Ok(pubkey) = key_str.parse::<Pubkey>() {
                            account_refs.insert(pubkey);
                        }
                    }
                }
            }
        }
    }

    Ok(account_refs)
}

#[tokio::main]
async fn main() -> Result<()> {
    solana_logger::setup();

    let cli = Cli::parse();

    match cli.command {
        Commands::Staleness { snapshot, index, current_slot, threads } => {
            run_staleness_analysis(snapshot, index, current_slot, threads).await
        },
        Commands::BlockUsage { snapshot, start_slot, end_slot, rpc_url, output, threads } => {
            run_block_usage_analysis(snapshot, start_slot, end_slot, rpc_url, output, threads).await
        },
    }
} 