use {
    log::*,
    rayon::prelude::*,
    solana_accounts_db::{
        account_storage::stored_account_info::StoredAccountInfo, accounts_db::AccountStorageEntry,
        accounts_file::StorageAccess,
    },
    solana_pubkey::Pubkey,
    solana_runtime::{
        snapshot_archive_info::{FullSnapshotArchiveInfo, SnapshotArchiveInfoGetter},
        snapshot_utils::verify_and_unarchive_snapshots,
    },
    std::{
        collections::HashMap,
        path::PathBuf,
        sync::{Arc, Mutex},
    },
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

    /// Common snapshot parsing setup - returns the unpacked snapshot data for processing
    fn setup_snapshot_parsing(
        &mut self,
    ) -> Result<(tempfile::TempDir, Vec<(u64, Arc<AccountStorageEntry>)>), Box<dyn std::error::Error>>
    {
        info!("Parsing snapshot: {}", self.snapshot_path.display());

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

        // Unarchive the snapshot
        let (unarchived_snapshots, _guard) = verify_and_unarchive_snapshots(
            &bank_snapshots_dir,
            &archive_info,
            None, // No incremental snapshot
            &account_paths,
            StorageAccess::File,
        )?;

        info!("Snapshot unpacked");

        // Collect storage entries to process
        let storage_entries: Vec<_> = unarchived_snapshots
            .full_storage
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();

        info!(
            "Processing {} storage entries with 16 parallel threads",
            storage_entries.len()
        );

        Ok((temp_dir, storage_entries))
    }

    /// Process storage entries in parallel with a custom processor function
    fn process_storage_entries<T, F>(
        &self,
        storage_entries: Vec<(u64, Arc<AccountStorageEntry>)>,
        initial_value: T,
        processor: F,
    ) -> T
    where
        T: Send + 'static + std::fmt::Debug,
        F: Fn(&Arc<Mutex<T>>, &StoredAccountInfo) + Send + Sync,
    {
        let shared_data = Arc::new(Mutex::new(initial_value));

        // Configure rayon to use 16 threads
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(16)
            .build()
            .expect("Failed to create thread pool");

        // Use rayon to process storage entries in parallel
        thread_pool.install(|| {
            storage_entries
                .par_iter()
                .for_each(|(slot, storage_entry)| {
                    // Scan this storage entry (this is the I/O heavy part)
                    let scan_result = storage_entry.accounts.scan_accounts(|_offset, account| {
                        processor(&shared_data, &account);
                    });

                    if let Err(e) = scan_result {
                        warn!("Failed to scan storage for slot {}: {}", slot, e);
                    }
                });
        });

        // Extract the final results
        Arc::try_unwrap(shared_data).unwrap().into_inner().unwrap()
    }

    pub fn parse_accounts(
        &mut self,
        activity_map: &HashMap<Pubkey, crate::AccountActivity>,
    ) -> Result<HashMap<Pubkey, crate::AccountMetadata>, Box<dyn std::error::Error>> {
        let (temp_dir, storage_entries) = self.setup_snapshot_parsing()?;
        let activity_map = activity_map.clone(); // Clone for move into closure

        let result = self.process_storage_entries(
            storage_entries,
            HashMap::new(),
            move |shared_accounts: &Arc<Mutex<HashMap<Pubkey, crate::AccountMetadata>>>,
                  account: &StoredAccountInfo| {
                let pubkey = *account.pubkey;

                if activity_map.contains_key(&pubkey) {
                    let metadata = crate::AccountMetadata {
                        lamports: account.lamports,
                        owner: *account.owner,
                        executable: account.executable,
                        data_size: account.data.len() as u64,
                    };

                    let mut accounts = shared_accounts.lock().unwrap();
                    accounts.insert(pubkey, metadata);
                }
            },
        );

        // Store temp_dir to keep it alive
        self.temp_dir = Some(temp_dir);

        info!("Found {} tracked accounts", result.len());
        Ok(result)
    }

    /// Parse all pubkeys from the snapshot (not filtered by activity index)
    pub fn parse_all_pubkeys(&mut self) -> Result<Vec<Pubkey>, Box<dyn std::error::Error>> {
        let (temp_dir, storage_entries) = self.setup_snapshot_parsing()?;

        let result = self.process_storage_entries(
            storage_entries,
            Vec::new(),
            |shared_pubkeys: &Arc<Mutex<Vec<Pubkey>>>, account: &StoredAccountInfo| {
                let mut pubkeys = shared_pubkeys.lock().unwrap();
                pubkeys.push(*account.pubkey);
            },
        );

        // Store temp_dir to keep it alive
        self.temp_dir = Some(temp_dir);

        info!("Found {} total pubkeys", result.len());
        Ok(result)
    }
}
