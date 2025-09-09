mod snapshot_parser;

use {
    anyhow::Result,
    clap::{Parser, Subcommand},
    log::*,
    rusqlite::Connection,
    serde::{Deserialize, Serialize},
    snapshot_parser::SnapshotParser,
    solana_pubkey::Pubkey,
    std::{collections::HashMap, path::{Path, PathBuf}},
};

#[cfg(not(any(target_env = "msvc", target_os = "freebsd")))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;


#[derive(Parser)]
#[command(
    name = "agave-snapshot-analyzer",
    about = "Analyze Solana snapshot data using embedded database"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create database from snapshot and activity index
    CreateDb {
        /// Path to the snapshot archive file
        #[arg(long, short = 's')]
        snapshot: PathBuf,
        
        /// Path to the bincode account activity index file
        #[arg(long, short = 'i')]
        index: PathBuf,
        
        /// Output database file path
        #[arg(long, short = 'o')]
        output: PathBuf,
    },
    
    /// Run SQL query on database
    Query {
        /// Path to the database file
        #[arg(long, short = 'd')]
        database: PathBuf,
        
        /// SQL query to execute
        #[arg(long, short = 'q')]
        query: String,
    },
    
    /// Analyze account staleness (runs predefined query)
    Staleness {
        /// Path to the database file
        #[arg(long, short = 'd')]
        database: PathBuf,
        
        /// Current epoch for staleness calculation
        #[arg(long)]
        current_epoch: Option<u16>,
    },
    
    /// Analyze block usage (requires additional slot-account mapping)
    BlockUsage {
        /// Path to the database file
        #[arg(long, short = 'd')]
        database: PathBuf,
        
        /// Path to bincode file containing HashMap<u64, Vec<String>> mapping slots to account lists
        #[arg(long, short = 'f')]
        slot_accounts_file: PathBuf,
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


async fn create_database(snapshot: PathBuf, index: PathBuf, output: PathBuf) -> Result<()> {
    info!("Creating database at: {}", output.display());
    
    // Remove existing database
    if output.exists() {
        std::fs::remove_file(&output)?;
    }
    
    // Create SQLite connection
    let mut conn = Connection::open(&output)?;
    
    // Optimize SQLite for bulk inserts
    conn.execute_batch(r#"
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA cache_size = 100000;
        PRAGMA temp_store = MEMORY;
    "#)?;
    
    // Create the main accounts table
    conn.execute_batch(r#"
        CREATE TABLE accounts (
            account TEXT PRIMARY KEY,
            account_size INTEGER,
            top_read_epochs TEXT,
            top_write_epochs TEXT,
            read_count INTEGER,
            write_count INTEGER,
            max_read_epoch INTEGER,
            max_write_epoch INTEGER,
            total_activity_count INTEGER
        );
    "#)?;
    
    // Load and insert data
    info!("Loading account activity index...");
    let activity_map = load_account_activity_index(&index);
    if activity_map.is_empty() {
        return Err(anyhow::anyhow!("No activity entries loaded"));
    }
    
    info!("Loading snapshot...");
    let mut parser = SnapshotParser::new(&snapshot);
    let account_sizes = parser.parse_accounts(&activity_map).await
        .map_err(|e| anyhow::anyhow!("Failed to parse snapshot: {}", e))?;
    
    info!("Inserting data into database...");
    
    // Use a single large transaction for maximum speed
    let tx = conn.transaction()?;
    
    // Prepare statement once
    let mut stmt = tx.prepare(r#"
        INSERT INTO accounts VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#)?;
    
    let mut inserted = 0;
    
    for (pubkey, activity) in &activity_map {
        if let Some(&account_size) = account_sizes.get(pubkey) {
            let max_read = activity.top_read_epochs.iter().max().copied().unwrap_or(0) as i32;
            let max_write = activity.top_write_epochs.iter().max().copied().unwrap_or(0) as i32;
            let total_activity = activity.read_count + activity.write_count;
            
            // Convert arrays to JSON strings
            let read_epochs_str = format!("[{}]", activity.top_read_epochs.iter()
                .map(|x| x.to_string()).collect::<Vec<_>>().join(","));
            let write_epochs_str = format!("[{}]", activity.top_write_epochs.iter()
                .map(|x| x.to_string()).collect::<Vec<_>>().join(","));
            
            stmt.execute((
                &pubkey.to_string(),
                account_size as i64,
                &read_epochs_str,
                &write_epochs_str,
                activity.read_count as i32,
                activity.write_count as i32,
                max_read,
                max_write,
                total_activity as i32,
            ))?;
            
            inserted += 1;
            
            // Progress reporting every 50k accounts
            if inserted % 50000 == 0 {
                info!("Inserted {} accounts...", inserted);
            }
        }
    }
    
    // Commit the entire transaction
    drop(stmt);
    tx.commit()?;
    
    info!("Database created successfully with {} tracked accounts", inserted);
    Ok(())
}

fn load_account_activity_index(index_path: &Path) -> HashMap<Pubkey, AccountActivity> {
    info!("Loading account activity index from: {}", index_path.display());
    
    let file_data = match std::fs::read(index_path) {
        Ok(data) => data,
        Err(e) => {
        warn!("Failed to read index file: {}. Using empty index.", e);
        return HashMap::new();
        }
    };
    
    match bincode::deserialize::<Vec<AccountActivityEntry>>(&file_data) {
        Ok(entries) => {
            info!("Loaded {} account activity entries", entries.len());
            entries
                .into_iter()
                .map(|entry| (entry.account, entry.activity))
                .collect()
        },
        Err(e) => {
            error!("Failed to deserialize account activity index: {}", e);
            HashMap::new()
        }
    }
}

fn run_query(database: PathBuf, query: String) -> Result<()> {
    let conn = Connection::open(&database)?;
    
    let mut stmt = conn.prepare(&query)?;
    let column_count = stmt.column_count();
    
    let rows = stmt.query_map([], |row| {
        let mut values = Vec::new();
        for i in 0..column_count {
            let value: String = match row.get::<_, Option<String>>(i) {
                Ok(Some(s)) => s,
                Ok(None) => "NULL".to_string(),
                Err(_) => {
                    // Try as integer
                    match row.get::<_, Option<i64>>(i) {
                        Ok(Some(n)) => n.to_string(),
                        Ok(None) => "NULL".to_string(),
                        Err(_) => "NULL".to_string(),
                    }
                }
            };
            values.push(value);
        }
        Ok(values)
    })?;
    
    for row in rows {
        let row = row?;
        println!("{}", row.join("\t"));
    }

    Ok(())
}

fn run_staleness_query(database: PathBuf, current_epoch: Option<u16>) -> Result<()> {
    let conn = Connection::open(&database)?;
    
    let current_epoch = match current_epoch {
        Some(epoch) => epoch,
        None => {
            // Get the highest epoch from the database
            let mut stmt = conn.prepare("SELECT MAX(MAX(max_read_epoch, max_write_epoch)) FROM accounts")?;
            let max_epoch: Option<i32> = stmt.query_row([], |row| row.get(0))?;
            match max_epoch {
                Some(epoch) => {
                    info!("Using highest epoch from database: {}", epoch);
                    epoch as u16
                },
                None => {
                    warn!("No epochs found in database, using default: 600");
                    600
                }
            }
        }
    };
    
    let query = format!(r#"
        WITH staleness_buckets AS (
            SELECT 
                account,
                account_size,
                MAX(max_read_epoch, max_write_epoch) as latest_epoch,
                CASE 
                    WHEN MAX(max_read_epoch, max_write_epoch) >= {} - 20 THEN '1_month'
                    WHEN MAX(max_read_epoch, max_write_epoch) >= {} - 40 THEN '2_months'
                    WHEN MAX(max_read_epoch, max_write_epoch) >= {} - 80 THEN '4_months'
                    WHEN MAX(max_read_epoch, max_write_epoch) >= {} - 160 THEN '8_months'
                    ELSE 'older'
                END as staleness_category
            FROM accounts
        )
        SELECT 
            staleness_category,
            COUNT(*) as account_count,
            SUM(account_size) as total_bytes,
            ROUND(CAST(SUM(account_size) AS REAL) / 1000000000.0, 2) as total_gb
        FROM staleness_buckets 
        GROUP BY staleness_category 
        ORDER BY 
            CASE staleness_category 
                WHEN '1_month' THEN 1
                WHEN '2_months' THEN 2  
                WHEN '4_months' THEN 3
                WHEN '8_months' THEN 4
                ELSE 5
            END;
    "#, current_epoch, current_epoch, current_epoch, current_epoch);
    
    println!("Account Staleness Analysis (Current Epoch: {})", current_epoch);
    println!("Category\tAccounts\tTotal Bytes\tTotal GB");
    
    // Run the query using the existing connection
    let mut stmt = conn.prepare(&query)?;
    let column_count = stmt.column_count();
    
    let rows = stmt.query_map([], |row| {
        let mut values = Vec::new();
        for i in 0..column_count {
            let value: String = match row.get::<_, Option<String>>(i) {
                Ok(Some(s)) => s,
                Ok(None) => "NULL".to_string(),
                Err(_) => {
                    // Try as integer
                    match row.get::<_, Option<i64>>(i) {
                        Ok(Some(n)) => n.to_string(),
                        Ok(None) => "NULL".to_string(),
                        Err(_) => "NULL".to_string(),
                    }
                }
            };
            values.push(value);
        }
        Ok(values)
    })?;
    
    for row in rows {
        let row = row?;
        println!("{}", row.join("\t"));
    }

    Ok(())
}

fn run_block_usage_query(database: PathBuf, _slot_accounts_file: PathBuf) -> Result<()> {
    // For now, just show the most active accounts since block usage needs the slot mapping
    let query = r#"
        SELECT 
            account,
            account_size,
            total_activity_count,
            max_read_epoch,
            max_write_epoch
        FROM accounts 
        ORDER BY total_activity_count DESC 
        LIMIT 20;
    "#;
    
    println!("Most Active Accounts:");
    println!("Account\tSize\tActivity\tMax Read\tMax Write");
    run_query(database, query.to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    solana_logger::setup();

    let cli = Cli::parse();

    match cli.command {
        Commands::CreateDb { snapshot, index, output } => {
            create_database(snapshot, index, output).await
        },
        Commands::Query { database, query } => {
            run_query(database, query)
        },
        Commands::Staleness { database, current_epoch } => {
            run_staleness_query(database, current_epoch)
        },
        Commands::BlockUsage { database, slot_accounts_file } => {
            run_block_usage_query(database, slot_accounts_file)
        },
    }
}