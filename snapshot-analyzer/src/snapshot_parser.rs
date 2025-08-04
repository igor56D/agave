use {
    log::*,
    solana_accounts_db::{
        accounts_file::StorageAccess,
        accounts_db::AccountStorageEntry,
    },
    solana_clock::Slot,
    solana_pubkey::Pubkey,
    solana_runtime::{
        snapshot_archive_info::{FullSnapshotArchiveInfo, SnapshotArchiveInfoGetter},
        snapshot_utils::verify_and_unarchive_snapshots,
    },
    std::{
        path::PathBuf,
    },
};

#[derive(Debug, Clone)]
pub struct AccountInfo {
    pub pubkey: Pubkey,
    pub data_len: u64,
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

    pub fn parse_accounts(&mut self) -> Result<Vec<AccountInfo>, Box<dyn std::error::Error>> {
        info!("Parsing snapshot: {}", self.snapshot_path.display());
        
        // Check if this is a full snapshot archive
        let archive_info = match FullSnapshotArchiveInfo::new_from_path(self.snapshot_path.clone()) {
            Ok(info) => info,
            Err(e) => {
                return Err(format!("Failed to parse snapshot archive info: {}", e).into());
            }
        };

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

        info!("Unpacking snapshot to temporary directory...");
        
        // Unarchive the snapshot
        let (unarchived_snapshots, _guard) = verify_and_unarchive_snapshots(
            &bank_snapshots_dir,
            &archive_info,
            None, // No incremental snapshot
            &account_paths,
            StorageAccess::File,
        )?;

        info!("Snapshot unpacked successfully, analyzing accounts...");

        // Now we need to parse the account files from the unarchived storage
        let mut accounts = Vec::new();
        
        // Iterate through the full storage map using proper DashMap iteration
        for entry in unarchived_snapshots.full_storage.iter() {
            let slot = *entry.key();
            let storage_entry = entry.value();
            debug!("Processing storage for slot: {}", slot);
            accounts.extend(self.parse_storage_entry(storage_entry, slot)?);
        }

        // Store temp_dir to keep it alive
        self.temp_dir = Some(temp_dir);

        info!("Parsed {} accounts from snapshot", accounts.len());
        Ok(accounts)
    }

    fn parse_storage_entry(&self, storage_entry: &AccountStorageEntry, slot: Slot) -> Result<Vec<AccountInfo>, Box<dyn std::error::Error>> {
        debug!("Parsing storage entry for slot: {}", slot);
        
        let mut accounts = Vec::new();
        
        // Use the storage entry's accounts scan method with proper callback signature
        storage_entry.accounts.scan_accounts(|_offset, account| {
            let account_info = AccountInfo {
                pubkey: *account.pubkey,
                data_len: account.data.len() as u64,
            };
            
            accounts.push(account_info);
        })?;
        
        Ok(accounts)
    }


} 