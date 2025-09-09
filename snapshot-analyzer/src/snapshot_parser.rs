use {
    log::*,
    rayon::prelude::*,
    solana_accounts_db::accounts_file::StorageAccess,
    solana_pubkey::Pubkey,
    solana_runtime::{
        snapshot_archive_info::{FullSnapshotArchiveInfo, SnapshotArchiveInfoGetter},
        snapshot_utils::verify_and_unarchive_snapshots,
    },
    std::{collections::HashMap, path::PathBuf, sync::{Arc, Mutex}},
};


pub struct SnapshotParser {
    pub snapshot_path: PathBuf,
    pub temp_dir: Option<tempfile::TempDir>,
}

impl SnapshotParser {
    pub fn new(snapshot_path: impl Into<PathBuf>) -> Self {
        Self {
            snapshot_path: snapshot_path.into(),
            temp_dir: None,
        }
    }

    pub fn parse_accounts(&mut self, activity_map: &HashMap<Pubkey, crate::AccountActivity>) -> Result<HashMap<Pubkey, u64>, Box<dyn std::error::Error>> {
        info!("Parsing snapshot directly: {}", self.snapshot_path.display());
        
        // Parse the snapshot archive info
        let archive_info = FullSnapshotArchiveInfo::new_from_path(self.snapshot_path.clone())?;
        info!("Found snapshot with slot: {}", archive_info.slot());

        // Create temporary directory for unpacking
        let temp_dir = tempfile::TempDir::new()?;
        let bank_snapshots_dir = temp_dir.path().join("bank_snapshots");
        std::fs::create_dir_all(&bank_snapshots_dir)?;

        // Create account paths
        let account_paths = vec![temp_dir.path().join("accounts")];
        for path in &account_paths {
            std::fs::create_dir_all(path)?;
        }

        info!("Unpacking snapshot...");
        
        // Unarchive the snapshot (this is the lightweight part)
        let (unarchived_snapshots, _guard) = verify_and_unarchive_snapshots(
            &bank_snapshots_dir,
            &archive_info,
            None, // No incremental snapshot
            &account_paths,
            StorageAccess::File,
        )?;

        info!("Snapshot unpacked, scanning for tracked accounts with rayon...");

        // Use Arc<Mutex<HashMap>> for thread-safe access
        let tracked_accounts = Arc::new(Mutex::new(HashMap::new()));
        
        // Collect storage entries to process
        let storage_entries: Vec<_> = unarchived_snapshots.full_storage
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();

        info!("Processing {} storage entries with 16 parallel threads", storage_entries.len());

        // Configure rayon to use 16 threads
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(16)
            .build()?;

        // Use rayon to process storage entries in parallel
        thread_pool.install(|| {
            storage_entries.par_iter().for_each(|(slot, storage_entry)| {
                let mut local_accounts = HashMap::new();
                
                // Scan this storage entry (this is the I/O heavy part)
                let scan_result = storage_entry.accounts.scan_accounts(|_offset, account| {
                    let pubkey = *account.pubkey;
                    
                    if activity_map.contains_key(&pubkey) {
                        let data_len = account.data.len() as u64;
                        local_accounts.insert(pubkey, data_len);
                    }
                });
                
                if let Err(e) = scan_result {
                    warn!("Failed to scan storage for slot {}: {}", slot, e);
                } else if !local_accounts.is_empty() {
                    // Batch insert into shared map
                    let mut shared_accounts = tracked_accounts.lock().unwrap();
                    shared_accounts.extend(local_accounts);
                }
            });
        });

        // Extract the final results
        let tracked_accounts = Arc::try_unwrap(tracked_accounts)
            .unwrap()
            .into_inner()
            .unwrap();

        // Store temp_dir to keep it alive
        self.temp_dir = Some(temp_dir);

        info!("Found {} tracked accounts", tracked_accounts.len());
        
        Ok(tracked_accounts)
    }

} 