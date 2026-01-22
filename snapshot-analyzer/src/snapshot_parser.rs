use {
    log::*,
    rayon::prelude::*,
    solana_accounts_db::{
        account_storage::stored_account_info::StoredAccountInfo, accounts_db::AccountStorageEntry,
        accounts_file::StorageAccess,
    },
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    solana_runtime::{
        snapshot_archive_info::{FullSnapshotArchiveInfo, SnapshotArchiveInfoGetter},
        snapshot_utils::verify_and_unarchive_snapshots,
    },
    std::{collections::HashMap, path::PathBuf, sync::Arc},
};

#[derive(Debug, Clone, Copy)]
pub struct RentPayingAccountStats {
    pub total_accounts: u64,
    pub rent_paying_accounts: u64,
}

#[derive(Debug)]
pub struct RentPayingAccountReport {
    pub stats: RentPayingAccountStats,
    pub accounts: Vec<Pubkey>,
}

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
            "Processing {} storage entries with 32 parallel threads",
            storage_entries.len()
        );

        Ok((temp_dir, storage_entries))
    }

    /// Process storage entries in parallel (32 chunks), using per-thread accumulators
    /// - init:       creates a local accumulator for a chunk
    /// - processor:  updates the local accumulator for each account
    /// - combine:    merges two accumulators into one
    fn process_storage_entries<T, Init, Proc, Comb>(
        &self,
        storage_entries: Vec<(u64, Arc<AccountStorageEntry>)>,
        init: Init,
        processor: Proc,
        combine: Comb,
    ) -> T
    where
        T: Send + 'static,
        Init: Fn() -> T + Sync,
        Proc: Fn(&mut T, &StoredAccountInfo) + Send + Sync,
        Comb: Fn(T, T) -> T + Send + Sync,
    {
        // Configure rayon to use 32 threads
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(32)
            .build()
            .expect("Failed to create thread pool");

        // Split storage entries into 32 chunks
        let n_threads = 32usize;
        let chunk_size = (storage_entries.len() + n_threads - 1) / n_threads;

        let partials: Vec<T> = thread_pool.install(|| {
            if chunk_size == 0 {
                return Vec::new();
            }
            storage_entries
                .par_chunks(chunk_size)
                .map(|chunk| {
                    let mut local = init();
                    chunk.iter().for_each(|(slot, storage_entry)| {
                        if let Err(e) = storage_entry
                            .accounts
                            .scan_accounts(|_offset, account| processor(&mut local, &account))
                        {
                            warn!("Failed to scan storage for slot {}: {}", slot, e);
                        }
                    });
                    local
                })
                .collect()
        });

        // Reduce all partials into a final result (or init() if none)
        partials.into_iter().reduce(combine).unwrap_or_else(init)
    }

    pub fn parse_accounts(
        &mut self,
        activity_map: &HashMap<Pubkey, crate::AccountActivity>,
    ) -> Result<HashMap<Pubkey, crate::AccountMetadata>, Box<dyn std::error::Error>> {
        let (temp_dir, storage_entries) = self.setup_snapshot_parsing()?;
        let result = self.process_storage_entries(
            storage_entries,
            || HashMap::<Pubkey, crate::AccountMetadata>::new(),
            |local_accounts: &mut HashMap<Pubkey, crate::AccountMetadata>,
             account: &StoredAccountInfo| {
                let pubkey = *account.pubkey;
                if activity_map.contains_key(&pubkey) {
                    let metadata = crate::AccountMetadata {
                        lamports: account.lamports,
                        owner: *account.owner,
                        executable: account.executable,
                        data_size: account.data.len() as u64,
                    };
                    local_accounts.insert(pubkey, metadata);
                }
            },
            |mut a, b| {
                a.extend(b);
                a
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
            || Vec::<Pubkey>::new(),
            |local_pubkeys: &mut Vec<Pubkey>, account: &StoredAccountInfo| {
                local_pubkeys.push(*account.pubkey);
            },
            |mut a, mut b| {
                a.append(&mut b);
                a
            },
        );

        // Store temp_dir to keep it alive
        self.temp_dir = Some(temp_dir);

        info!("Found {} total pubkeys", result.len());
        Ok(result)
    }

    pub fn collect_rent_paying_accounts(
        &mut self,
        rent: &Rent,
    ) -> Result<RentPayingAccountReport, Box<dyn std::error::Error>> {
        let (temp_dir, storage_entries) = self.setup_snapshot_parsing()?;

        let report = self.process_storage_entries(
            storage_entries,
            || RentPayingAccountReport {
                stats: RentPayingAccountStats {
                    total_accounts: 0,
                    rent_paying_accounts: 0,
                },
                accounts: Vec::new(),
            },
            |local_report: &mut RentPayingAccountReport, account: &StoredAccountInfo| {
                if account.lamports == 0 {
                    return;
                }
                local_report.stats.total_accounts += 1;
                let data_len = account.data.len();
                let min_balance = rent.minimum_balance(data_len);
                if account.lamports < min_balance {
                    local_report.stats.rent_paying_accounts += 1;
                    local_report.accounts.push(*account.pubkey);
                }
            },
            |mut a, mut b| RentPayingAccountReport {
                stats: RentPayingAccountStats {
                    total_accounts: a.stats.total_accounts + b.stats.total_accounts,
                    rent_paying_accounts: a.stats.rent_paying_accounts
                        + b.stats.rent_paying_accounts,
                },
                accounts: {
                    a.accounts.append(&mut b.accounts);
                    a.accounts
                },
            },
        );

        self.temp_dir = Some(temp_dir);

        info!(
            "Rent-paying accounts: {} of {} total",
            report.stats.rent_paying_accounts, report.stats.total_accounts
        );
        Ok(report)
    }
}
