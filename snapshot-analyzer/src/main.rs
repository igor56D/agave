mod snapshot_parser;

use {
    anyhow::Result,
    clap::{Parser, Subcommand},
    log::*,
    rayon::prelude::*,
    rusqlite::Connection,
    serde::{Deserialize, Serialize},
    snapshot_parser::{BankSysvarSnapshotValues, SnapshotParser},
    solana_bloom::bloom::Bloom,
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    std::{
        collections::HashMap,
        fs::File,
        io::{BufWriter, Write},
        path::{Path, PathBuf},
    },
    tabled::{builder::Builder, settings::Style},
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

        /// Create database of accounts in index but not in snapshot (index-only accounts)
        #[arg(long)]
        index_only: bool,

        /// Create database of snapshot accounts only (without activity index data)
        #[arg(long)]
        snapshot_only: bool,
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

    /// Run SQL template with parameters
    RunQuery {
        /// Path to the database file
        #[arg(long, short = 'd')]
        database: PathBuf,

        /// SQL template file name (from templates/ directory, without .sql extension)
        #[arg(long, short = 't')]
        template: String,

        /// Template parameters in format key=value (can be specified multiple times)
        #[arg(long, short = 'p')]
        params: Vec<String>,
    },

    /// Create bloom filter from all pubkeys in snapshot
    CreateBloom {
        /// Path to the snapshot archive file
        #[arg(long, short = 's')]
        snapshot: PathBuf,

        /// Output bloom filter file path
        #[arg(long, short = 'o')]
        output: PathBuf,

        /// False positive rate (default: 0.01 = 1%)
        #[arg(long, default_value = "0.01")]
        false_rate: f64,
    },

    /// Report accounts below the rent-exempt minimum
    RentPaying {
        /// Path to the snapshot archive file
        #[arg(long, short = 's')]
        snapshot: PathBuf,

        /// Output file to write rent-paying account addresses (one per line)
        #[arg(long, short = 'o')]
        output: PathBuf,
    },

    /// Check that snapshot-serialized Bank fields match their corresponding sysvar accounts
    CheckSysvars {
        /// Path to the snapshot archive file
        #[arg(long, short = 's')]
        snapshot: PathBuf,
    },

    /// Sort all accounts by size (largest first), compute prefix sums, write to file, and print power-of-2 summary
    AccountSizePrefixSums {
        /// Path to the snapshot archive file
        #[arg(long, short = 's')]
        snapshot: PathBuf,

        /// Output file for full prefix sums (one line per account: rank, size_bytes, prefix_sum_bytes)
        #[arg(long, short = 'o')]
        output: PathBuf,
    },

    /// Output a compact CSV mapping account (last 8 bytes of pubkey, hex) to account data size (u32)
    AccountSizesCsv {
        /// Path to the snapshot archive file
        #[arg(long, short = 's')]
        snapshot: PathBuf,

        /// Output CSV file path (columns: pubkey_suffix_hex, size)
        #[arg(long, short = 'o')]
        output: PathBuf,
    },

    /// Download the most recent full snapshot that can be discovered
    #[command(name = "download-latest", alias = "download-nearest")]
    DownloadNearest {
        /// RPC URL (used for getBlockTime). SOLANA_RPC_URL env overrides default.
        #[arg(long, default_value = DEFAULT_RPC_URL)]
        rpc_url: String,

        /// URL that returns JSON array: [{"slot", "hash", "block_time"?}]. SNAPSHOT_LIST_URL env overrides default.
        #[arg(long, default_value = DEFAULT_SNAPSHOT_LIST_URL)]
        snapshot_list_url: String,

        /// Directory to write the downloaded snapshot archive
        #[arg(long, default_value = ".")]
        output_dir: PathBuf,

        /// Base URL for downloading snapshot files (default: base of snapshot_list_url).
        /// Snapshot is fetched as {download_base_url}/snapshot-{slot}-{hash}.tar.zst (or .tar.lz4)
        #[arg(long)]
        download_base_url: Option<String>,
    },
}

/// Default RPC URL for mainnet-beta (used by download-nearest when not specified).
const DEFAULT_RPC_URL: &str = "https://api.mainnet-beta.solana.com";

/// Default snapshot list URL for mainnet-beta.
/// Must return a JSON array of {"slot": number, "hash": string, "block_time"?: number}.
/// Override with SNAPSHOT_LIST_URL env or --snapshot-list-url (many public RPCs don't host this).
const DEFAULT_SNAPSHOT_LIST_URL: &str = "https://api.mainnet-beta.solana.com/snapshot-list.json";

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

#[derive(Debug, Clone)]
pub struct AccountMetadata {
    pub lamports: u64,
    pub owner: Pubkey,
    pub executable: bool,
    pub data_size: u64,
}

fn create_database(
    snapshot: PathBuf,
    index: PathBuf,
    output: PathBuf,
    index_only: bool,
    snapshot_only: bool,
) -> Result<()> {
    info!("Creating database at: {}", output.display());

    // Remove existing database
    if output.exists() {
        std::fs::remove_file(&output)?;
    }

    // Create SQLite connection
    let conn = Connection::open(&output)?;

    // Optimize SQLite for bulk inserts (aggressive settings; machine has ample RAM)
    // These pragmas are applied before any tables are created
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = OFF;
        PRAGMA locking_mode = EXCLUSIVE;
        PRAGMA page_size = 32768;
        PRAGMA journal_mode = OFF;
        PRAGMA synchronous = OFF;
        PRAGMA temp_store = MEMORY;
        -- Negative cache_size is KiB; here ~1 GiB cache
        PRAGMA cache_size = -1048576;
        -- Enable mmap to reduce syscall overhead if supported
        PRAGMA mmap_size = 1073741824;
    "#,
    )?;

    if index_only {
        create_index_only_database(conn, snapshot, index)
    } else if snapshot_only {
        create_snapshot_only_database(conn, snapshot)
    } else {
        create_standard_database(conn, snapshot, index)
    }
}

fn create_index_only_database(
    mut conn: Connection,
    snapshot: PathBuf,
    index: PathBuf,
) -> Result<()> {
    info!("Creating index-only database (accounts in index but not in snapshot)");

    // Create the index-only accounts table (no snapshot metadata)
    conn.execute_batch(
        r#"
        CREATE TABLE index_only_accounts (
            account TEXT PRIMARY KEY,
            top_read_epochs TEXT,
            top_write_epochs TEXT,
            read_count INTEGER,
            write_count INTEGER,
            max_read_epoch INTEGER,
            max_write_epoch INTEGER,
            total_activity_count INTEGER
        );
    "#,
    )?;

    // Load account activity index
    info!("Loading account activity index...");
    let activity_map = load_account_activity_index(&index);
    if activity_map.is_empty() {
        return Err(anyhow::anyhow!("No activity entries loaded"));
    }

    // Load snapshot to identify which accounts ARE in it
    info!("Loading snapshot to identify accounts present in snapshot...");
    let mut parser = SnapshotParser::new(&snapshot);
    let accounts_in_snapshot = parser
        .parse_accounts(&activity_map)
        .map_err(|e| anyhow::anyhow!("Failed to parse snapshot: {}", e))?;

    info!("Identifying accounts in index but NOT in snapshot...");
    let thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(32)
        .build()
        .expect("Failed to create thread pool");

    // Build as Vec to keep parallel collection efficient; consume the map to free memory
    let mut index_only_accounts: Vec<(Pubkey, AccountActivity)> = thread_pool.install(|| {
        activity_map
            .into_par_iter()
            .filter(|(pubkey, _)| !accounts_in_snapshot.contains_key(pubkey))
            .collect()
    });

    info!(
        "Found {} accounts in index but not in snapshot",
        index_only_accounts.len()
    );
    info!("Inserting index-only accounts into database...");

    // Speed up inserts further by disabling autovacuum/analysis during load
    conn.execute_batch(
        r#"
        PRAGMA analysis_limit=0;
        PRAGMA optimize;
    "#,
    )?;

    let tx = conn.transaction()?;
    let mut stmt = tx.prepare(
        r#"
        INSERT INTO index_only_accounts VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    "#,
    )?;

    let mut inserted = 0;

    for (pubkey, activity) in index_only_accounts.drain(..) {
        let max_read = activity.top_read_epochs.iter().max().copied().unwrap_or(0) as i32;
        let max_write = activity.top_write_epochs.iter().max().copied().unwrap_or(0) as i32;
        let total_activity = activity.read_count + activity.write_count;

        // Convert arrays to JSON strings
        let read_epochs_str = format!(
            "[{}]",
            activity
                .top_read_epochs
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let write_epochs_str = format!(
            "[{}]",
            activity
                .top_write_epochs
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );

        stmt.execute((
            &pubkey.to_string(),
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
            info!("Inserted {} index-only accounts...", inserted);
        }
    }

    // Commit the transaction
    drop(stmt);
    tx.commit()?;

    info!(
        "Index-only database created successfully with {} accounts",
        inserted
    );
    println!("Index-only accounts added: {}", inserted);
    Ok(())
}

fn create_snapshot_only_database(mut conn: Connection, snapshot: PathBuf) -> Result<()> {
    info!("Creating snapshot-only database (accounts from snapshot without activity index)");

    // Create the snapshot-only accounts table
    conn.execute_batch(
        r#"
        CREATE TABLE accounts (
            account TEXT PRIMARY KEY,
            account_size INTEGER,
            lamports INTEGER,
            owner TEXT,
            executable INTEGER
        );
    "#,
    )?;

    info!("Table created, initializing parser...");
    let mut parser = SnapshotParser::new(&snapshot);
    info!("Parser initialized, starting single-pass account parsing...");

    // Use single-pass parsing to avoid unpacking snapshot twice
    let account_metadata = parser
        .parse_all_accounts()
        .map_err(|e| anyhow::anyhow!("Failed to parse snapshot: {}", e))?;

    info!(
        "Single-pass parsing completed, got metadata for {} accounts",
        account_metadata.len()
    );

    // Speed up inserts further by disabling analysis during load
    conn.execute_batch(
        r#"
        PRAGMA analysis_limit=0;
        PRAGMA optimize;
    "#,
    )?;

    info!("Starting database transaction...");
    // Use a single large transaction for maximum speed
    let tx = conn.transaction()?;

    // Prepare statement once
    let mut stmt = tx.prepare(
        r#"
        INSERT INTO accounts VALUES (?, ?, ?, ?, ?)
    "#,
    )?;

    info!(
        "Prepared insert statement, starting insertion of {} accounts...",
        account_metadata.len()
    );
    let mut inserted = 0;

    for (pubkey, metadata) in &account_metadata {
        stmt.execute((
            &pubkey.to_string(),
            metadata.data_size as i64,
            metadata.lamports as i64,
            &metadata.owner.to_string(),
            metadata.executable as i32,
        ))?;

        inserted += 1;

        // Progress reporting every 50k accounts
        if inserted % 50000 == 0 {
            info!("Inserted {} accounts...", inserted);
        }
    }

    info!(
        "All {} accounts inserted, committing transaction...",
        inserted
    );
    // Commit the entire transaction
    drop(stmt);
    tx.commit()?;

    info!(
        "Snapshot-only database created successfully with {} accounts",
        inserted
    );
    println!("Snapshot-only database created: {} accounts", inserted);
    Ok(())
}

fn create_standard_database(mut conn: Connection, snapshot: PathBuf, index: PathBuf) -> Result<()> {
    info!("Creating standard database (accounts in both index and snapshot)");

    // Create the main accounts table
    conn.execute_batch(
        r#"
        CREATE TABLE accounts (
            account TEXT PRIMARY KEY,
            account_size INTEGER,
            lamports INTEGER,
            owner TEXT,
            executable INTEGER,
            top_read_epochs TEXT,
            top_write_epochs TEXT,
            read_count INTEGER,
            write_count INTEGER,
            max_read_epoch INTEGER,
            max_write_epoch INTEGER,
            total_activity_count INTEGER
        );
    "#,
    )?;

    // Load and insert data
    info!("Loading account activity index...");
    let activity_map = load_account_activity_index(&index);
    if activity_map.is_empty() {
        return Err(anyhow::anyhow!("No activity entries loaded"));
    }

    info!("Loading snapshot...");
    let mut parser = SnapshotParser::new(&snapshot);
    let account_metadata = parser
        .parse_accounts(&activity_map)
        .map_err(|e| anyhow::anyhow!("Failed to parse snapshot: {}", e))?;

    info!("Inserting data into database...");

    // Speed up inserts further by disabling analysis during load
    conn.execute_batch(
        r#"
        PRAGMA analysis_limit=0;
        PRAGMA optimize;
    "#,
    )?;

    // Use a single large transaction for maximum speed
    let tx = conn.transaction()?;

    // Prepare statement once
    let mut stmt = tx.prepare(
        r#"
        INSERT INTO accounts VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#,
    )?;

    let mut inserted = 0;

    for (pubkey, activity) in &activity_map {
        if let Some(metadata) = account_metadata.get(pubkey) {
            let max_read = activity.top_read_epochs.iter().max().copied().unwrap_or(0) as i32;
            let max_write = activity.top_write_epochs.iter().max().copied().unwrap_or(0) as i32;
            let total_activity = activity.read_count + activity.write_count;

            // Convert arrays to JSON strings
            let read_epochs_str = format!(
                "[{}]",
                activity
                    .top_read_epochs
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let write_epochs_str = format!(
                "[{}]",
                activity
                    .top_write_epochs
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            );

            stmt.execute((
                &pubkey.to_string(),
                metadata.data_size as i64,
                metadata.lamports as i64,
                &metadata.owner.to_string(),
                metadata.executable as i32,
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

    info!(
        "Database created successfully with {} tracked accounts",
        inserted
    );
    Ok(())
}

fn load_account_activity_index(index_path: &Path) -> HashMap<Pubkey, AccountActivity> {
    info!(
        "Loading account activity index from: {}",
        index_path.display()
    );

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
        }
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

    // Get column names
    let column_names: Vec<String> = (0..column_count)
        .map(|i| stmt.column_name(i).unwrap_or("Unknown").to_string())
        .collect();

    // Collect all rows
    let rows: Result<Vec<Vec<String>>, _> = stmt
        .query_map([], |row| {
            let mut values = Vec::new();
            for i in 0..column_count {
                let value: String = match row.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => "NULL".to_string(),
                    rusqlite::types::ValueRef::Integer(n) => n.to_string(),
                    rusqlite::types::ValueRef::Real(f) => format!("{}", f),
                    rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).to_string(),
                    rusqlite::types::ValueRef::Blob(_) => "<BLOB>".to_string(),
                };
                values.push(value);
            }
            Ok(values)
        })?
        .collect();

    let rows = rows?;

    if rows.is_empty() {
        println!("No results found.");
        return Ok(());
    }

    // Create table with builder
    let mut builder = Builder::default();

    // Add header row
    builder.push_record(column_names);

    // Add data rows
    for row in rows {
        builder.push_record(row);
    }

    // Build and style the table
    let mut table = builder.build();
    table.with(Style::modern());

    println!("{}", table);

    Ok(())
}

fn parse_template_params(params: Vec<String>) -> Result<HashMap<String, String>> {
    let mut param_map = HashMap::new();

    for param in params {
        let parts: Vec<&str> = param.splitn(2, '=').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid parameter format: '{}'. Expected 'key=value'",
                param
            ));
        }
        param_map.insert(parts[0].to_string(), parts[1].to_string());
    }

    Ok(param_map)
}

fn substitute_template_params(template: &str, params: &HashMap<String, String>) -> String {
    let mut result = template.to_string();

    for (key, value) in params {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, value);
    }

    result
}

fn load_template(template_name: &str) -> Result<String> {
    let template_path =
        PathBuf::from("snapshot-analyzer/templates").join(format!("{}.sql", template_name));

    match std::fs::read_to_string(&template_path) {
        Ok(content) => Ok(content),
        Err(_) => {
            // Try from current directory if relative path doesn't work
            let template_path = PathBuf::from("templates").join(format!("{}.sql", template_name));
            std::fs::read_to_string(&template_path)
                .map_err(|e| anyhow::anyhow!("Failed to load template '{}': {}", template_name, e))
        }
    }
}

fn apply_default_params(template_name: &str, param_map: &mut HashMap<String, String>) {
    // Define defaults for each template
    match template_name {
        "staleness" => {
            param_map
                .entry("current_epoch".to_string())
                .or_insert("600".to_string());
            param_map
                .entry("lookback_epochs".to_string())
                .or_insert("50".to_string());
        }
        "most_active" => {
            param_map
                .entry("limit".to_string())
                .or_insert("20".to_string());
        }
        "owner_analysis" => {
            param_map
                .entry("min_accounts".to_string())
                .or_insert("1".to_string());
        }
        "balance_range" => {
            param_map
                .entry("min_lamports".to_string())
                .or_insert("0".to_string());
            param_map
                .entry("max_lamports".to_string())
                .or_insert("NULL".to_string());
        }
        "read_write_ratio" => {
            param_map
                .entry("limit".to_string())
                .or_insert("20".to_string());
            param_map
                .entry("min_activity".to_string())
                .or_insert("10".to_string());
        }
        "index_only_staleness" => {
            param_map
                .entry("current_epoch".to_string())
                .or_insert("600".to_string());
            param_map
                .entry("lookback_epochs".to_string())
                .or_insert("50".to_string());
        }
        "index_only_staleness_breakdown" => {
            param_map
                .entry("current_epoch".to_string())
                .or_insert("600".to_string());
            param_map
                .entry("lookback_epochs".to_string())
                .or_insert("50".to_string());
        }
        "index_only_low_activity_count" => {
            param_map.entry("n".to_string()).or_insert("1".to_string());
        }
        "arbitrary_accounts" => {
            param_map.entry("n".to_string()).or_insert("20".to_string());
        }
        "index_only_arbitrary_accounts" => {
            param_map.entry("n".to_string()).or_insert("20".to_string());
        }
        "random_accounts" => {
            param_map.entry("n".to_string()).or_insert("20".to_string());
        }
        "index_only_random_accounts" => {
            param_map.entry("n".to_string()).or_insert("20".to_string());
        }
        "fee_payers" => {
            // No parameters needed for fee_payers
        }
        "nonce_accounts" => {
            // owner parameter is required - no default
        }
        _ => {
            // No defaults for unknown templates
        }
    }
}

fn run_template_query(database: PathBuf, template: String, params: Vec<String>) -> Result<()> {
    info!("Loading template: {}", template);
    let template_content = load_template(&template)?;

    info!("Parsing template parameters");
    let mut param_map = parse_template_params(params)?;

    info!("Applying default parameters");
    apply_default_params(&template, &mut param_map);

    info!("Substituting template parameters");
    let final_query = substitute_template_params(&template_content, &param_map);

    // Check for unresolved placeholders
    if final_query.contains("{{") && final_query.contains("}}") {
        let mut missing_params = Vec::new();
        let mut start = 0;
        while let Some(open) = final_query[start..].find("{{") {
            let open_pos = start + open + 2;
            if let Some(close) = final_query[open_pos..].find("}}") {
                let param_name = &final_query[open_pos..open_pos + close];
                missing_params.push(param_name.to_string());
                start = open_pos + close + 2;
            } else {
                break;
            }
        }

        if !missing_params.is_empty() {
            return Err(anyhow::anyhow!(
                "Template has unresolved parameters: {}. Please provide values using -p key=value",
                missing_params.join(", ")
            ));
        }
    }

    info!("Executing query");
    println!("Template: {}", template);
    println!("Parameters: {:?}", param_map);
    println!();

    run_query(database, final_query)
}

fn create_bloom_filter(snapshot: PathBuf, output: PathBuf, false_rate: f64) -> Result<()> {
    info!(
        "Creating bloom filter from snapshot: {}",
        snapshot.display()
    );
    info!("Output file: {}", output.display());
    info!("False positive rate: {}", false_rate);

    // Parse snapshot to get all pubkeys
    info!("Parsing snapshot for all pubkeys...");
    let mut parser = SnapshotParser::new(&snapshot);
    let all_pubkeys = parser
        .parse_all_pubkeys()
        .map_err(|e| anyhow::anyhow!("Failed to parse snapshot: {}", e))?;

    let num_pubkeys = all_pubkeys.len();
    info!("Found {} pubkeys in snapshot", num_pubkeys);

    if num_pubkeys == 0 {
        return Err(anyhow::anyhow!("No pubkeys found in snapshot"));
    }

    // Calculate optimal bloom filter size with 1GB limit
    const MAX_BITS: usize = 1024 * 1024 * 1024 * 8; // 1GB in bits
    info!("Creating bloom filter with max {} bits (1GB)", MAX_BITS);

    // Create bloom filter with optimal parameters
    let mut bloom: Bloom<Pubkey> = Bloom::random(num_pubkeys, false_rate, MAX_BITS);

    info!(
        "Bloom filter created with {} bits and {} hash functions",
        bloom.bits.len(),
        bloom.keys.len()
    );

    // Calculate actual memory usage
    let memory_usage_bits = bloom.bits.len();
    let memory_usage_bytes = memory_usage_bits / 8;
    let memory_usage_mb = memory_usage_bytes as f64 / (1024.0 * 1024.0);
    info!("Bloom filter memory usage: {:.2} MB", memory_usage_mb);

    // Add all pubkeys to the bloom filter
    info!("Adding {} pubkeys to bloom filter...", num_pubkeys);
    let mut added = 0;
    for pubkey in &all_pubkeys {
        bloom.add(pubkey);
        added += 1;

        // Progress reporting every 100k pubkeys
        if added % 100000 == 0 {
            info!("Added {} pubkeys...", added);
        }
    }

    info!("Successfully added all {} pubkeys to bloom filter", added);

    // Serialize and write bloom filter to disk
    info!("Serializing bloom filter...");
    let serialized_bloom = bincode::serialize(&bloom)
        .map_err(|e| anyhow::anyhow!("Failed to serialize bloom filter: {}", e))?;

    let serialized_size_mb = serialized_bloom.len() as f64 / (1024.0 * 1024.0);
    info!("Serialized bloom filter size: {:.2} MB", serialized_size_mb);

    info!("Writing bloom filter to disk...");
    std::fs::write(&output, &serialized_bloom).map_err(|e| {
        anyhow::anyhow!(
            "Failed to write bloom filter to {}: {}",
            output.display(),
            e
        )
    })?;

    info!("Bloom filter successfully written to: {}", output.display());
    println!("Bloom filter created successfully!");
    println!("  - Input pubkeys: {}", num_pubkeys);
    println!("  - False positive rate: {}", false_rate);
    println!("  - Memory usage: {:.2} MB", memory_usage_mb);
    println!("  - Serialized size: {:.2} MB", serialized_size_mb);
    println!("  - Hash functions: {}", bloom.keys.len());
    println!("  - Output file: {}", output.display());

    Ok(())
}

fn report_rent_paying_accounts(snapshot: PathBuf, output: PathBuf) -> Result<()> {
    let rent = Rent::default();
    info!(
        "Using rent config: lamports_per_byte_year={}, exemption_threshold={}, burn_percent={}",
        rent.lamports_per_byte_year, rent.exemption_threshold, rent.burn_percent
    );

    let mut parser = SnapshotParser::new(&snapshot);
    let report = parser
        .collect_rent_paying_accounts(&rent)
        .map_err(|e| anyhow::anyhow!("Failed to parse snapshot: {}", e))?;

    if report.stats.total_accounts == 0 {
        println!("No accounts found in snapshot.");
        return Ok(());
    }

    let rent_paying_pct =
        (report.stats.rent_paying_accounts as f64 / report.stats.total_accounts as f64) * 100.0;
    let rent_exempt_accounts = report.stats.total_accounts - report.stats.rent_paying_accounts;

    info!(
        "Writing {} rent-paying accounts to {}",
        report.stats.rent_paying_accounts,
        output.display()
    );
    let file = File::create(&output)
        .map_err(|e| anyhow::anyhow!("Failed to create output file {}: {}", output.display(), e))?;
    let mut writer = BufWriter::new(file);
    for pubkey in &report.accounts {
        writeln!(writer, "{}", pubkey)?;
    }

    println!("Total accounts: {}", report.stats.total_accounts);
    println!("Rent-exempt accounts: {}", rent_exempt_accounts);
    println!(
        "Rent-paying accounts: {} ({:.4}%)",
        report.stats.rent_paying_accounts, rent_paying_pct
    );
    println!(
        "Any rent-paying accounts: {}",
        if report.stats.rent_paying_accounts > 0 {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "Rent-paying account addresses written to: {}",
        output.display()
    );

    Ok(())
}

fn print_sysvar_check(name: &str, matches: Option<bool>, details: &str) {
    match matches {
        Some(true) => println!("[OK]   {name}: {details}"),
        Some(false) => println!("[FAIL] {name}: {details}"),
        None => println!("[SKIP] {name}: sysvar account not present in snapshot"),
    }
}

fn check_snapshot_sysvars(snapshot: PathBuf) -> Result<()> {
    info!(
        "Checking snapshot sysvars for consistency: {}",
        snapshot.display()
    );

    let mut parser = SnapshotParser::new(&snapshot);
    let BankSysvarSnapshotValues {
        bank_rent,
        snapshot_rent,
        bank_epoch_schedule,
        snapshot_epoch_schedule,
    } = parser
        .collect_bank_and_sysvar_values()
        .map_err(|e| anyhow::anyhow!("Failed to collect bank/sysvar values from snapshot: {e}"))?;

    println!("Snapshot sysvar consistency check");
    println!("  Snapshot: {}", snapshot.display());
    println!();

    let mut any_mismatch = false;

    // Rent
    let rent_matches = snapshot_rent
        .as_ref()
        .map(|rent_sysvar| rent_sysvar == &bank_rent);
    if let Some(false) = rent_matches {
        any_mismatch = true;
    }
    let rent_details = match snapshot_rent {
        Some(ref rent_sysvar) => format!(
            "bank rent = {:?}, sysvar rent = {:?}",
            bank_rent, rent_sysvar
        ),
        None => String::from("rent sysvar account not found"),
    };
    print_sysvar_check("rent", rent_matches, &rent_details);

    // Epoch schedule
    let epoch_matches = snapshot_epoch_schedule
        .as_ref()
        .map(|epoch_sysvar| epoch_sysvar == &bank_epoch_schedule);
    if let Some(false) = epoch_matches {
        any_mismatch = true;
    }
    let epoch_details = match snapshot_epoch_schedule {
        Some(ref epoch_sysvar) => format!(
            "bank epoch_schedule = {:?}, sysvar epoch_schedule = {:?}",
            bank_epoch_schedule, epoch_sysvar
        ),
        None => String::from("epoch_schedule sysvar account not found"),
    };
    print_sysvar_check("epoch_schedule", epoch_matches, &epoch_details);

    if any_mismatch {
        Err(anyhow::anyhow!(
            "Snapshot contains mismatches between Bank fields and sysvar accounts"
        ))
    } else {
        println!();
        println!("All checked Bank fields match their corresponding sysvar accounts.");
        Ok(())
    }
}

fn account_size_prefix_sums(snapshot: PathBuf, output: PathBuf) -> Result<()> {
    info!(
        "Collecting account sizes from snapshot: {}",
        snapshot.display()
    );
    let mut parser = SnapshotParser::new(&snapshot);
    let mut sizes = parser
        .collect_account_sizes()
        .map_err(|e| anyhow::anyhow!("Failed to collect account sizes: {}", e))?;

    if sizes.is_empty() {
        println!("No accounts found in snapshot.");
        return Ok(());
    }

    info!("Sorting {} accounts by size (largest first)", sizes.len());
    sizes.sort_by(|a, b| b.cmp(a));

    info!("Computing prefix sums");
    let mut prefix_sum: u64 = 0;
    let prefix_sums: Vec<u64> = sizes
        .iter()
        .map(|&s| {
            prefix_sum += s;
            prefix_sum
        })
        .collect();

    info!("Writing results to: {}", output.display());
    let file = File::create(&output)
        .map_err(|e| anyhow::anyhow!("Failed to create output file {}: {}", output.display(), e))?;
    let mut writer = BufWriter::new(file);
    writeln!(writer, "rank,size_bytes,prefix_sum_bytes")?;
    for (i, (&size, &psum)) in sizes.iter().zip(prefix_sums.iter()).enumerate() {
        writeln!(writer, "{},{},{}", i + 1, size, psum)?;
    }
    writer.flush()?;

    let total_accounts = sizes.len();
    let total_bytes: u64 = prefix_sums.last().copied().unwrap_or(0);

    println!("Account size prefix sums (largest to smallest)");
    println!("  Total accounts: {}", total_accounts);
    println!("  Total bytes:     {}", total_bytes);
    println!();
    println!("Power-of-2 prefix sum summary:");
    println!(
        "  {:>12}  {:>18}  {:>10}",
        "accounts", "prefix_sum_bytes", "pct_total"
    );
    let mut k: u32 = 0;
    loop {
        let n = (1usize << k).min(total_accounts);
        if n == 0 {
            break;
        }
        let psum = prefix_sums[n - 1];
        let pct = if total_bytes > 0 {
            (100.0 * psum as f64) / total_bytes as f64
        } else {
            0.0
        };
        println!("  {:>12}  {:>18}  {:>9.2}%", n, psum, pct);
        if n >= total_accounts {
            break;
        }
        k += 1;
    }
    println!();
    println!("Full output written to: {}", output.display());

    Ok(())
}

fn account_sizes_csv(snapshot: PathBuf, output: PathBuf) -> Result<()> {
    info!(
        "Collecting account (pubkey_suffix, size) from snapshot: {}",
        snapshot.display()
    );
    let mut parser = SnapshotParser::new(&snapshot);
    let pairs = parser
        .collect_account_pubkey_suffix_and_size()
        .map_err(|e| anyhow::anyhow!("Failed to collect account sizes: {}", e))?;

    if pairs.is_empty() {
        println!("No accounts found in snapshot.");
        return Ok(());
    }

    info!("Writing CSV to: {}", output.display());
    let file = File::create(&output)
        .map_err(|e| anyhow::anyhow!("Failed to create output file {}: {}", output.display(), e))?;
    let mut writer = BufWriter::new(file);
    writeln!(writer, "pubkey_suffix_hex,size")?;
    for (suffix, size) in &pairs {
        writeln!(writer, "{:016x},{}", suffix, size)?;
    }
    writer.flush()?;
    println!("Wrote {} account rows to {}", pairs.len(), output.display());
    Ok(())
}

/// Snapshot list entry: slot, hash, optional block_time (unix timestamp)
#[derive(serde::Deserialize)]
struct SnapshotListEntry {
    slot: u64,
    hash: String,
    #[serde(default)]
    block_time: Option<i64>,
}

fn download_nearest(
    rpc_url: String,
    snapshot_list_url: String,
    output_dir: PathBuf,
    download_base_url: Option<String>,
) -> Result<()> {
    use chrono::{TimeZone, Utc};

    // Allow env overrides so users can set defaults once (e.g. in .bashrc)
    let rpc_url = std::env::var("SOLANA_RPC_URL").unwrap_or(rpc_url);
    let snapshot_list_url = std::env::var("SNAPSHOT_LIST_URL").unwrap_or(snapshot_list_url);

    let client = reqwest::blocking::Client::new();
    let base = download_base_url.as_deref().unwrap_or_else(|| {
        let u = snapshot_list_url.trim_end_matches('/');
        u.rsplit_once('/').map(|(b, _)| b).unwrap_or(u)
    });
    let base = base.trim_end_matches('/');

    std::fs::create_dir_all(&output_dir)
        .map_err(|e| anyhow::anyhow!("Failed to create output dir: {}", e))?;

    // 0) First try static "latest snapshot" filenames.
    // Some providers expose these without requiring slot/hash discovery.
    let static_bases = [base, rpc_url.trim_end_matches('/')];
    if let Some(path) = try_download_static_latest_snapshot(&client, &static_bases, &output_dir)? {
        println!("Downloaded latest snapshot to {}", path.display());
        return Ok(());
    }

    // 1) Try snapshot-list endpoint first. This is the most reliable way to get slot+hash.
    let maybe_latest_from_list: Option<(u64, String, Option<i64>)> = (|| {
        let resp = client
            .post(&snapshot_list_url)
            .json(&serde_json::json!({}))
            .send()
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let list: Vec<SnapshotListEntry> = resp.json().ok()?;
        if list.is_empty() {
            return None;
        }
        list.into_iter()
            .max_by_key(|e| (e.block_time.unwrap_or(i64::MIN), e.slot))
            .map(|e| (e.slot, e.hash, e.block_time))
    })();

    // 2) Fallback: if no usable snapshot-list, ask RPC for highest snapshot slot
    // and scrape snapshot filenames from the download base URL to recover the hash.
    let (slot, hash, block_time) = if let Some((slot, hash, block_time)) = maybe_latest_from_list {
        (slot, hash, block_time)
    } else {
        warn!(
            "Could not use snapshot list URL {}; trying RPC + filename discovery fallback",
            snapshot_list_url
        );
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getHighestSnapshotSlot",
            "params": []
        });
        let resp: serde_json::Value = client
            .post(&rpc_url)
            .json(&body)
            .send()
            .map_err(|e| anyhow::anyhow!("RPC getHighestSnapshotSlot request failed: {}", e))?
            .json()
            .map_err(|e| anyhow::anyhow!("RPC getHighestSnapshotSlot JSON parse failed: {}", e))?;
        let target_slot = resp
            .get("result")
            .and_then(|r| r.get("full"))
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "RPC did not return getHighestSnapshotSlot.full. Response: {}",
                    resp
                )
            })?;

        let listing_text = client
            .get(base)
            .send()
            .ok()
            .and_then(|r| r.text().ok())
            .unwrap_or_default();
        let discovered = discover_snapshot_candidates(&listing_text);
        let maybe = discovered
            .iter()
            .filter(|(slot, _, _)| *slot <= target_slot)
            .max_by_key(|(slot, _, _)| *slot)
            .or_else(|| discovered.iter().max_by_key(|(slot, _, _)| *slot))
            .cloned();
        let (slot, hash, _ext) = maybe.ok_or_else(|| {
            anyhow::anyhow!(
                "Could not discover snapshot filenames from {}. \
                 Please provide --snapshot-list-url that returns JSON entries with slot/hash.",
                base
            )
        })?;

        let block_time = {
            let body = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getBlockTime",
                "params": [slot]
            });
            let resp: serde_json::Value = client
                .post(&rpc_url)
                .json(&body)
                .send()
                .ok()
                .and_then(|r| r.json().ok())
                .unwrap_or_default();
            resp.get("result").and_then(|v| v.as_i64())
        };
        (slot, hash, block_time)
    };

    let extensions = ["tar.zst", "tar.lz4"];
    for ext in &extensions {
        let filename = format!("snapshot-{}-{}.{}", slot, hash, ext);
        let url = format!("{}/{}", base, filename);
        let dest = output_dir.join(&filename);
        info!("Trying {} -> {}", url, dest.display());
        match client.get(&url).send() {
            Ok(resp) if resp.status().is_success() => {
                let mut out = File::create(&dest)
                    .map_err(|e| anyhow::anyhow!("Failed to create {}: {}", dest.display(), e))?;
                let mut body = resp;
                std::io::copy(&mut body, &mut out)
                    .map_err(|e| anyhow::anyhow!("Failed to write download: {}", e))?;
                let dt = block_time.and_then(|ts| Utc.timestamp_opt(ts, 0).single());
                println!(
                    "Downloaded latest discovered snapshot slot {}{} to {}",
                    slot,
                    dt.map(|t| format!(" (block time {})", t.to_rfc3339()))
                        .unwrap_or_default(),
                    dest.display()
                );
                return Ok(());
            }
            _ => continue,
        }
    }

    Err(anyhow::anyhow!(
        "Failed to download snapshot for slot {} (hash {}) from {}",
        slot,
        hash,
        base
    ))
}

fn try_download_static_latest_snapshot(
    client: &reqwest::blocking::Client,
    bases: &[&str],
    output_dir: &Path,
) -> Result<Option<PathBuf>> {
    let static_names = ["snapshot.tar.zst", "snapshot.tar.lz4", "snapshot.tar.bz2"];
    for base in bases {
        let base = base.trim_end_matches('/');
        for name in &static_names {
            let url = format!("{}/{}", base, name);
            let dest = output_dir.join(name);
            info!(
                "Trying static latest snapshot {} -> {}",
                url,
                dest.display()
            );
            if let Ok(resp) = client.get(&url).send() {
                if !resp.status().is_success() {
                    continue;
                }
                let mut out = File::create(&dest)
                    .map_err(|e| anyhow::anyhow!("Failed to create {}: {}", dest.display(), e))?;
                let mut body = resp;
                std::io::copy(&mut body, &mut out)
                    .map_err(|e| anyhow::anyhow!("Failed to write download: {}", e))?;
                return Ok(Some(dest));
            }
        }
    }
    Ok(None)
}

/// Parse a page/blob and extract snapshot filename candidates:
/// snapshot-<slot>-<hash>.tar.zst or .tar.lz4
fn discover_snapshot_candidates(text: &str) -> Vec<(u64, String, String)> {
    text.split(|c: char| {
        c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>' || c == '(' || c == ')'
    })
    .filter_map(|tok| {
        let token = tok.trim_matches('/');
        if !(token.starts_with("snapshot-")
            && (token.ends_with(".tar.zst") || token.ends_with(".tar.lz4")))
        {
            return None;
        }
        let parts: Vec<&str> = token.split('-').collect();
        if parts.len() < 3 {
            return None;
        }
        let slot = parts[1].parse::<u64>().ok()?;
        let hash_and_ext = &parts[2..].join("-");
        let (hash, ext) = if let Some(h) = hash_and_ext.strip_suffix(".tar.zst") {
            (h.to_string(), "tar.zst".to_string())
        } else if let Some(h) = hash_and_ext.strip_suffix(".tar.lz4") {
            (h.to_string(), "tar.lz4".to_string())
        } else {
            return None;
        };
        Some((slot, hash, ext))
    })
    .collect()
}

fn main() -> Result<()> {
    solana_logger::setup();

    let cli = Cli::parse();

    match cli.command {
        Commands::CreateDb {
            snapshot,
            index,
            output,
            index_only,
            snapshot_only,
        } => create_database(snapshot, index, output, index_only, snapshot_only),
        Commands::Query { database, query } => run_query(database, query),
        Commands::RunQuery {
            database,
            template,
            params,
        } => run_template_query(database, template, params),
        Commands::CreateBloom {
            snapshot,
            output,
            false_rate,
        } => create_bloom_filter(snapshot, output, false_rate),
        Commands::RentPaying { snapshot, output } => report_rent_paying_accounts(snapshot, output),
        Commands::CheckSysvars { snapshot } => check_snapshot_sysvars(snapshot),
        Commands::AccountSizePrefixSums { snapshot, output } => {
            account_size_prefix_sums(snapshot, output)
        }
        Commands::AccountSizesCsv { snapshot, output } => account_sizes_csv(snapshot, output),
        Commands::DownloadNearest {
            rpc_url,
            snapshot_list_url,
            output_dir,
            download_base_url,
        } => download_nearest(rpc_url, snapshot_list_url, output_dir, download_base_url),
    }
}
