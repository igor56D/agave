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

#[derive(Debug, Clone)]
pub struct AccountMetadata {
    pub lamports: u64,
    pub owner: Pubkey,
    pub executable: bool,
    pub data_size: u64,
}


fn create_database(snapshot: PathBuf, index: PathBuf, output: PathBuf) -> Result<()> {
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
    "#)?;
    
    // Load and insert data
    info!("Loading account activity index...");
    let activity_map = load_account_activity_index(&index);
    if activity_map.is_empty() {
        return Err(anyhow::anyhow!("No activity entries loaded"));
    }
    
    info!("Loading snapshot...");
    let mut parser = SnapshotParser::new(&snapshot);
    let account_metadata = parser.parse_accounts(&activity_map)
        .map_err(|e| anyhow::anyhow!("Failed to parse snapshot: {}", e))?;
    
    info!("Inserting data into database...");
    
    // Use a single large transaction for maximum speed
    let tx = conn.transaction()?;
    
    // Prepare statement once
    let mut stmt = tx.prepare(r#"
        INSERT INTO accounts VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#)?;
    
    let mut inserted = 0;
    
    for (pubkey, activity) in &activity_map {
        if let Some(metadata) = account_metadata.get(pubkey) {
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
    
    // Get column names
    let column_names: Vec<String> = (0..column_count)
        .map(|i| stmt.column_name(i).unwrap_or("Unknown").to_string())
        .collect();
    
    // Collect all rows
    let rows: Result<Vec<Vec<String>>, _> = stmt.query_map([], |row| {
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
    })?.collect();
    
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
            return Err(anyhow::anyhow!("Invalid parameter format: '{}'. Expected 'key=value'", param));
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
    let template_path = PathBuf::from("snapshot-analyzer/templates")
        .join(format!("{}.sql", template_name));
    
    match std::fs::read_to_string(&template_path) {
        Ok(content) => Ok(content),
        Err(_) => {
            // Try from current directory if relative path doesn't work
            let template_path = PathBuf::from("templates")
                .join(format!("{}.sql", template_name));
            std::fs::read_to_string(&template_path)
                .map_err(|e| anyhow::anyhow!("Failed to load template '{}': {}", template_name, e))
        }
    }
}

fn apply_default_params(template_name: &str, param_map: &mut HashMap<String, String>) {
    // Define defaults for each template
    match template_name {
        "staleness" => {
            param_map.entry("current_epoch".to_string()).or_insert("600".to_string());
            param_map.entry("lookback_epochs".to_string()).or_insert("50".to_string());
        },
        "most_active" => {
            param_map.entry("limit".to_string()).or_insert("20".to_string());
        },
        "owner_analysis" => {
            param_map.entry("min_accounts".to_string()).or_insert("1".to_string());
        },
        "balance_range" => {
            param_map.entry("min_lamports".to_string()).or_insert("0".to_string());
            param_map.entry("max_lamports".to_string()).or_insert("NULL".to_string());
        },
        "read_write_ratio" => {
            param_map.entry("limit".to_string()).or_insert("20".to_string());
            param_map.entry("min_activity".to_string()).or_insert("10".to_string());
        },
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

fn main() -> Result<()> {
    solana_logger::setup();

    let cli = Cli::parse();

    match cli.command {
        Commands::CreateDb { snapshot, index, output } => {
            create_database(snapshot, index, output)
        },
        Commands::Query { database, query } => {
            run_query(database, query)
        },
        Commands::RunQuery { database, template, params } => {
            run_template_query(database, template, params)
        },
    }
}