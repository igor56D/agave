use {
    log::*,
    solana_accounts_db::accounts_file::StorageAccess,
    solana_pubkey::Pubkey,
    solana_runtime::{
        snapshot_archive_info::{FullSnapshotArchiveInfo, SnapshotArchiveInfoGetter},
        snapshot_utils::verify_and_unarchive_snapshots,
    },
    std::{collections::HashMap, path::PathBuf},
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

        info!("Snapshot unpacked, scanning for tracked accounts...");

        // Scan ONLY for accounts we care about
        let mut tracked_accounts = HashMap::new();

        for entry in unarchived_snapshots.full_storage.iter() {
            let storage_entry = entry.value();
            
            // Scan this storage, but only collect accounts we care about
            let _ = storage_entry.accounts.scan_accounts(|_offset, account| {
                let pubkey = *account.pubkey;
                
                if activity_map.contains_key(&pubkey) {
                    let data_len = account.data.len() as u64;
                    tracked_accounts.insert(pubkey, data_len);
                }
            });
        }

        // Store temp_dir to keep it alive
        self.temp_dir = Some(temp_dir);

        info!("Found {} tracked accounts", tracked_accounts.len());
        
        Ok(tracked_accounts)
    }

} 