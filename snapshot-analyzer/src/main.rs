mod snapshot_parser;

use {
    anyhow::{Context, Result},
    clap::{Parser, Subcommand},
    csv::Writer,
    dashmap::DashMap,
    log::*,
    rayon::prelude::*,
    serde::{Deserialize, Serialize},
    snapshot_parser::SnapshotParser,
    solana_clock::Slot,
    solana_pubkey::Pubkey,
    std::{
        collections::HashMap,
        fs::File,
        io::{BufReader, BufWriter},
        path::{Path, PathBuf},
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
    /// Analyze account staleness using pre-built activity index
    Staleness {
        /// Path to the snapshot archive file
        #[arg(long, short = 's')]
        snapshot: PathBuf,
        
        /// Path to the bincode account activity index file
        #[arg(long, short = 'i')]
        index: PathBuf,
        
        /// Current slot number (auto-detected from filename if not provided, converted to epoch)
        #[arg(long)]
        current_slot: Option<Slot>,
        
        /// Number of threads for parallel processing
        #[arg(long, short = 'j')]
        threads: Option<usize>,
    },
    /// Analyze bytes loaded per block using provided slot-account mapping
    BlockUsage {
        /// Path to the snapshot archive file
        #[arg(long, short = 's')]
        snapshot: PathBuf,
        
        /// Path to bincode file containing HashMap<u64, Vec<String>> mapping slots to account lists
        #[arg(long, short = 'f')]
        slot_accounts_file: PathBuf,
        
        /// Output CSV file path
        #[arg(long, short = 'o', default_value = "block-usage.csv")]
        output: PathBuf,
        
        /// Number of threads for parallel processing
        #[arg(long, short = 'j')]
        threads: Option<usize>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountActivityEntry {
    pub account: Pubkey,
    pub activity: AccountActivity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountActivity {
    /// Top 10 highest epochs where this account was read from (sorted descending)
    pub top_read_epochs: Vec<u16>,
    /// Top 10 highest epochs where this account was written to (sorted descending) 
    pub top_write_epochs: Vec<u16>,
    /// Total number of read operations
    pub read_count: u32,
    /// Total number of write operations
    pub write_count: u32,
}

#[derive(Debug)]
struct StalenessCheckpoint {
    months: u32,
    boundary_epoch: u16,
    total_size_bytes: u64,
    total_account_count: u64,
}

impl StalenessCheckpoint {
    fn print(&self) {
        println!("{} months ago (epoch {}): {} accounts, {:.2} GB",
                 self.months,
                 self.boundary_epoch,
                 self.total_account_count,
                 self.total_size_bytes as f64 / 1_000_000_000.0);
    }
}

// Solana epoch constants
const SOLANA_SLOTS_PER_EPOCH: u64 = 432_000; // Approximate slots per epoch on mainnet

fn slot_to_epoch(slot: Slot) -> u16 {
    (slot / SOLANA_SLOTS_PER_EPOCH) as u16
}

fn calculate_epoch_boundaries(current_epoch: u16) -> [u16; 4] {
    let epochs_per_month = (SECONDS_PER_MONTH * SOLANA_SLOTS_PER_SECOND / SOLANA_SLOTS_PER_EPOCH as f64) as u16;
    
    [
        current_epoch.saturating_sub(epochs_per_month),     // 1 month
        current_epoch.saturating_sub(2 * epochs_per_month), // 2 months
        current_epoch.saturating_sub(4 * epochs_per_month), // 4 months
        current_epoch.saturating_sub(8 * epochs_per_month), // 8 months
    ]
}

fn load_account_activity_index(index_path: &Path) -> Vec<AccountActivityEntry> {
    info!("Loading account activity index from: {}", index_path.display());
    
    // Use the same approach as producer: read entire file into memory first
    let file_data = match std::fs::read(index_path) {
        Ok(data) => {
            info!("Successfully read {} bytes from file", data.len());
            data
        },
        Err(e) => {
            warn!("Failed to read index file: {}. Using empty index.", e);
            return Vec::new();
        }
    };
    
    // Show first 16 bytes for debugging
    if file_data.len() >= 16 {
        info!("First 16 bytes (hex): {:02x?}", &file_data[0..16]);
        
        // Interpret first 8 bytes as length prefix
        let length_bytes: [u8; 8] = file_data[0..8].try_into().unwrap();
        let expected_length = u64::from_le_bytes(length_bytes);
        info!("Bincode length prefix: {} entries expected", expected_length);
        
        // Sanity check
        let max_reasonable_entries = 50_000_000u64;
        if expected_length > max_reasonable_entries {
            error!("Expected length {} is unreasonably large (> {})", expected_length, max_reasonable_entries);
            error!("This suggests the file format is incompatible or corrupted");
            return Vec::new();
        }
    } else {
        warn!("File is too small ({} bytes) to contain valid data", file_data.len());
        return Vec::new();
    }
    
    // Now deserialize using the same approach as producer
    let entries: Vec<AccountActivityEntry> = match bincode::deserialize::<Vec<AccountActivityEntry>>(&file_data) {
        Ok(entries) => {
            info!("Successfully deserialized {} entries", entries.len());
            entries
        },
        Err(e) => {
            error!("Failed to deserialize account activity index: {}", e);
            return Vec::new();
        }
    };
    
    info!("Loaded {} account activity entries", entries.len());
    
    // Verify the entries are sorted by most recent activity (highest epoch in either read or write)
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            let prev_entry = &entries[i - 1];
            let prev_max_epoch = prev_entry.activity.top_read_epochs.first()
                .copied()
                .unwrap_or(0)
                .max(prev_entry.activity.top_write_epochs.first().copied().unwrap_or(0));
            let curr_max_epoch = entry.activity.top_read_epochs.first()
                .copied()
                .unwrap_or(0)
                .max(entry.activity.top_write_epochs.first().copied().unwrap_or(0));
            
            if curr_max_epoch > prev_max_epoch {
                warn!("Index may not be properly sorted: epoch {} follows epoch {} at index {}", 
                      curr_max_epoch, prev_max_epoch, i);
                break;
            }
        }
    }

    entries
}

fn load_slot_accounts_mapping(file_path: &Path) -> Result<HashMap<u64, Vec<String>>> {
    info!("Loading slot-accounts mapping from: {}", file_path.display());
    
    let file = File::open(file_path)
        .with_context(|| format!("Failed to open slot-accounts file: {}", file_path.display()))?;

    
    
    let reader = BufReader::new(file);
    let mapping: HashMap<u64, Vec<String>> = bincode::deserialize_from(reader)
        .with_context(|| format!("Failed to deserialize slot-accounts mapping from: {}", file_path.display()))?;
    
    info!("Loaded mapping for {} slots", mapping.len());

    // print out number of accounts per slot
    for (slot, accounts) in &mapping {
        println!("Slot {}: {} accounts", slot, accounts.len());
    }
    
    Ok(mapping)
}

fn analyze_account_staleness_with_accounts(
    snapshot_accounts: Vec<snapshot_parser::AccountInfo>,
    access_entries: Vec<AccountActivityEntry>,
    current_slot: Slot,
) -> Result<Vec<StalenessCheckpoint>> {
    info!("Starting account staleness analysis");
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

    // Step 2: Calculate time boundaries (using pre-loaded access entries)
    let current_epoch = slot_to_epoch(current_slot);
    let epoch_boundaries = calculate_epoch_boundaries(current_epoch);
    info!("Current epoch: {}, Time boundaries (epochs): {:?}", current_epoch, epoch_boundaries);
    
    let boundary_months = [1, 2, 4, 8];
    
    // Step 3: Process entries and track checkpoints
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
        
        // Get the most recent activity epoch for this account
        let most_recent_epoch = entry.activity.top_read_epochs.first()
            .copied()
            .unwrap_or(0)
            .max(entry.activity.top_write_epochs.first().copied().unwrap_or(0));
        
        // Check if we've crossed any time boundaries
        while boundary_index < epoch_boundaries.len() && 
              most_recent_epoch <= epoch_boundaries[boundary_index] {
            
            let checkpoint = StalenessCheckpoint {
                months: boundary_months[boundary_index],
                boundary_epoch: epoch_boundaries[boundary_index],
                total_size_bytes: total_size,
                total_account_count,
            };
            
            info!("Checkpoint: {} months - {} GB at epoch {} (entry {}/{})",
                  checkpoint.months,
                  checkpoint.total_size_bytes as f64 / 1_000_000_000.0,
                  checkpoint.boundary_epoch,
                  i + 1,
                  access_entries.len());
            
            checkpoints.push(checkpoint);
            boundary_index += 1;
        }
        
        // Early exit if we've processed all boundaries
        if boundary_index >= epoch_boundaries.len() {
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
    if boundary_index < epoch_boundaries.len() {
        let checkpoint = StalenessCheckpoint {
            months: 8,
            boundary_epoch: epoch_boundaries[3],
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
    
    let current_epoch = slot_to_epoch(current_slot);
    info!("Current slot: {}, Current epoch: {}", current_slot, current_epoch);

    // Load account activity index first for debugging
    info!("Loading account activity index for debugging...");
    let access_entries = load_account_activity_index(&index);
    println!("📊 Loaded {} account activity entries from index", access_entries.len());
    
    // Show some statistics about the loaded index
    if !access_entries.is_empty() {
        let total_reads: u64 = access_entries.iter().map(|e| e.activity.read_count as u64).sum();
        let total_writes: u64 = access_entries.iter().map(|e| e.activity.write_count as u64).sum();
        let accounts_with_reads = access_entries.iter().filter(|e| e.activity.read_count > 0).count();
        let accounts_with_writes = access_entries.iter().filter(|e| e.activity.write_count > 0).count();
        
        println!("📈 Index statistics:");
        println!("   - Accounts with read activity: {}", accounts_with_reads);
        println!("   - Accounts with write activity: {}", accounts_with_writes);
        println!("   - Total read operations: {}", total_reads);
        println!("   - Total write operations: {}", total_writes);
        
        // Show epoch range
        let mut all_epochs = Vec::new();
        for entry in &access_entries {
            all_epochs.extend_from_slice(&entry.activity.top_read_epochs);
            all_epochs.extend_from_slice(&entry.activity.top_write_epochs);
        }
        if !all_epochs.is_empty() {
            all_epochs.sort_unstable();
            println!("   - Epoch range: {} to {}", all_epochs.first().unwrap_or(&0), all_epochs.last().unwrap_or(&0));
        }
    }

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
    
    let checkpoints = match analyze_account_staleness_with_accounts(snapshot_accounts, access_entries, current_slot) {
        Ok(checkpoints) => checkpoints,
        Err(e) => {
            warn!("Failed to analyze account staleness: {}. Using empty results.", e);
            Vec::new()
        }
    };
    
    println!("\n=== Solana Account Staleness Analysis ===\n");
    println!("Total snapshot: {} accounts, {:.2} GB", total_snapshot_accounts, total_snapshot_size as f64 / 1_000_000_000.0);
    println!("Using pre-built account activity index");
    println!("Analysis based on cumulative account sizes by epoch-based activity recency\n");
    
    if checkpoints.is_empty() {
        println!("⚠️  No checkpoints found - this may be due to:");
        println!("   - Missing or empty snapshot file");
        println!("   - Missing or empty index file");
        println!("   - Data processing errors (see warnings above)");
        println!("   - All accounts have activity more recent than 8 months");
    } else {
        println!("Checkpoints (accounts with activity within each time period):");
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

fn run_block_usage_analysis(
    snapshot: PathBuf,
    slot_accounts_file: PathBuf,
    output: PathBuf,
    threads: Option<usize>,
) -> Result<()> {
    configure_thread_pool(threads)?;

    info!("Starting block usage analysis using slot-accounts mapping");
    info!("Slot-accounts file: {}", slot_accounts_file.display());
    info!("Output file: {}", output.display());

    // Load slot-accounts mapping
    let slot_accounts_mapping = load_slot_accounts_mapping(&slot_accounts_file)?;
    let total_slots = slot_accounts_mapping.len();
    info!("Loaded mapping for {} slots", total_slots);

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

    // Create CSV writer
    let output_file = File::create(&output)
        .with_context(|| format!("Failed to create output file: {}", output.display()))?;
    let mut csv_writer = Writer::from_writer(BufWriter::new(output_file));
    
    // Write CSV header
    csv_writer.write_record(&["slot", "bytes_loaded"])?;

    // Process each slot from the mapping
    let mut processed = 0;
    let mut sorted_slots: Vec<_> = slot_accounts_mapping.keys().collect();
    sorted_slots.sort();

    for &slot in &sorted_slots {
        if let Some(account_strings) = slot_accounts_mapping.get(&slot) {
            let bytes_loaded = analyze_slot_usage(&account_map, account_strings);
            csv_writer.write_record(&[slot.to_string(), bytes_loaded.to_string()])?;
        } else {
            warn!("No account data found for slot {}", slot);
            csv_writer.write_record(&[slot.to_string(), "0".to_string()])?;
        }

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

fn analyze_slot_usage(account_map: &DashMap<Pubkey, u64>, account_strings: &[String]) -> u64 {
    let mut total_bytes = 0u64;
    
    for account_string in account_strings {
        if let Ok(pubkey) = account_string.parse::<Pubkey>() {
            if let Some(account_size_ref) = account_map.get(&pubkey) {
                total_bytes += *account_size_ref;
            }
        } else {
            warn!("Failed to parse account string as Pubkey: {}", account_string);
        }
    }
    
    total_bytes
}



#[tokio::main]
async fn main() -> Result<()> {
    solana_logger::setup();

    let cli = Cli::parse();

    match cli.command {
        Commands::Staleness { snapshot, index, current_slot, threads } => {
            run_staleness_analysis(snapshot, index, current_slot, threads).await
        },
        Commands::BlockUsage { snapshot, slot_accounts_file, output, threads } => {
            run_block_usage_analysis(snapshot, slot_accounts_file, output, threads)
        },
    }
}