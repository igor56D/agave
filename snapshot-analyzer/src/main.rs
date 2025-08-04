mod snapshot_parser;

use {
    anyhow::{Context, Result},
    clap::{App, Arg},
    log::*,
    serde::{Deserialize, Serialize},
    snapshot_parser::SnapshotParser,
    solana_clock::Slot,
    solana_pubkey::Pubkey,
    std::{
        collections::HashMap,
        fs::File,
        io::BufReader,
        path::Path,
    },
};

#[cfg(not(any(target_env = "msvc", target_os = "freebsd")))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

// Solana mainnet constants
const SOLANA_SLOTS_PER_SECOND: f64 = 2.5;
const SECONDS_PER_MONTH: f64 = 30.0 * 24.0 * 60.0 * 60.0;

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
}

impl StalenessCheckpoint {
    fn print(&self) {
        println!("{} months ago (slot {}): {:.2} GB",
                 self.months,
                 self.boundary_slot,
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

fn load_account_access_index(index_path: &Path) -> Result<Vec<AccountSlotEntry>> {
    info!("Loading account access index from: {}", index_path.display());
    
    let file = File::open(index_path)
        .with_context(|| format!("Failed to open index file: {}", index_path.display()))?;
    
    let reader = BufReader::new(file);
    let entries: Vec<AccountSlotEntry> = bincode::deserialize_from(reader)
        .context("Failed to deserialize account access index")?;
    
    info!("Loaded {} account access entries", entries.len());
    
    // Verify the entries are sorted from high to low slot
    if let Some(windows) = entries.windows(2).find(|w| w[0].highest_slot < w[1].highest_slot) {
        warn!("Index may not be properly sorted: slot {} follows slot {}", 
              windows[0].highest_slot, windows[1].highest_slot);
    }

    Ok(entries)
}

fn analyze_account_staleness(
    snapshot_path: &Path,
    index_path: &Path,
    current_slot: Slot,
) -> Result<Vec<StalenessCheckpoint>> {
    info!("Starting account staleness analysis");
    info!("Snapshot: {}", snapshot_path.display());
    info!("Index: {}", index_path.display());
    info!("Current slot: {}", current_slot);

    // Step 1: Load snapshot and build account map
    info!("Loading snapshot data...");
    let mut parser = SnapshotParser::new(snapshot_path);
    let snapshot_accounts = parser.parse_accounts()
        .map_err(|e| anyhow::anyhow!("Failed to parse snapshot accounts: {}", e))?;
    
        let mut account_map: HashMap<Pubkey, u64> = HashMap::new();
    let mut total_snapshot_size = 0u64;
    for account in snapshot_accounts {
        account_map.insert(account.pubkey, account.data_len);
        total_snapshot_size += account.data_len;
    }
    
    info!("Loaded {} accounts from snapshot with total size: {:.2} GB", 
          account_map.len(), 
          total_snapshot_size as f64 / 1_000_000_000.0);

    // Step 2: Load account access index
    let access_entries = load_account_access_index(index_path)?;

    // Step 3: Calculate time boundaries
    let slot_boundaries = calculate_slot_boundaries(current_slot);
    info!("Time boundaries (slots): {:?}", slot_boundaries);
    
    let boundary_months = [1, 2, 4, 8];
    
    // Step 4: Process entries and track checkpoints
    let mut checkpoints = Vec::new();
    let mut total_size = 0u64;
    let mut boundary_index = 0;

    info!("Processing {} access entries...", access_entries.len());

    for (i, entry) in access_entries.iter().enumerate() {
        // Check if account exists in snapshot
        if let Some(&account_size) = account_map.get(&entry.account) {
            total_size += account_size;
        }
        
        // Check if we've crossed any time boundaries
        while boundary_index < slot_boundaries.len() && 
              entry.highest_slot <= slot_boundaries[boundary_index] {
            
            let checkpoint = StalenessCheckpoint {
                months: boundary_months[boundary_index],
                boundary_slot: slot_boundaries[boundary_index],
                total_size_bytes: total_size,
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
            info!("Processed {}/{} entries, current total: {:.2} GB",
                  i + 1, 
                  access_entries.len(),
                  total_size as f64 / 1_000_000_000.0);
        }
    }

    // Add final checkpoint if we haven't reached 8 months yet
    if boundary_index < slot_boundaries.len() {
        let checkpoint = StalenessCheckpoint {
            months: 8,
            boundary_slot: slot_boundaries[3],
            total_size_bytes: total_size,
        };
        checkpoints.push(checkpoint);
    }

    info!("Analysis complete. Final total: {:.2} GB", total_size as f64 / 1_000_000_000.0);

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

fn main() -> Result<()> {
    solana_logger::setup();

    let app = App::new("agave-snapshot-analyzer")
        .about("Analyze Solana snapshot account staleness using pre-built access index")
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            Arg::with_name("snapshot")
                .long("snapshot")
                .value_name("PATH")
                .help("Path to the snapshot archive file")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("index")
                .long("index")
                .value_name("PATH")
                .help("Path to the bincode account access index file")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("current-slot")
                .long("current-slot")
                .value_name("SLOT")
                .help("Current slot number (for time boundary calculation)")
                .takes_value(true),
        );

    let matches = app.get_matches();

    let snapshot_path = Path::new(matches.value_of("snapshot").unwrap());
    let index_path = Path::new(matches.value_of("index").unwrap());
    
    if !snapshot_path.exists() {
        anyhow::bail!("Snapshot file does not exist: {}", snapshot_path.display());
    }
    
    if !index_path.exists() {
        anyhow::bail!("Index file does not exist: {}", index_path.display());
    }

    // Get current slot from command line or try to parse from filename
    let current_slot = if let Some(slot_str) = matches.value_of("current-slot") {
        slot_str.parse::<Slot>()
            .context("Invalid slot number")?
    } else if let Some(slot) = get_snapshot_slot_from_filename(snapshot_path) {
        info!("Using slot from filename: {}", slot);
        slot
    } else {
        anyhow::bail!("Could not determine current slot. Please provide --current-slot");
    };

    // Calculate total snapshot size
    let mut parser = SnapshotParser::new(snapshot_path);
    let snapshot_accounts = parser.parse_accounts()
        .map_err(|e| anyhow::anyhow!("Failed to parse snapshot accounts for total size: {}", e))?;
    let total_snapshot_size: u64 = snapshot_accounts.iter().map(|acc| acc.data_len).sum();
    
    let checkpoints = analyze_account_staleness(snapshot_path, index_path, current_slot)?;
    
    println!("\n=== Solana Account Staleness Analysis ===\n");
    println!("Total snapshot size: {:.2} GB", total_snapshot_size as f64 / 1_000_000_000.0);
    println!("Using pre-built account access index");
    println!("Analysis based on cumulative account sizes by access recency\n");
    
    println!("Checkpoints (accounts accessed within each time period):");
    println!("{}", "-".repeat(60));
    
    for checkpoint in &checkpoints {
        checkpoint.print();
    }
    
    if checkpoints.len() >= 2 {
        println!("\nGrowth between checkpoints:");
        println!("{}", "-".repeat(40));
        for i in 1..checkpoints.len() {
            let growth = checkpoints[i].total_size_bytes - checkpoints[i-1].total_size_bytes;
            println!("{} to {} months: +{:.2} GB",
                     checkpoints[i-1].months,
                     checkpoints[i].months,
                     growth as f64 / 1_000_000_000.0);
        }
    }

    Ok(())
} 