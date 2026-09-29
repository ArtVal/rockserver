//! Filesystem storage implementation for prepared station-icon artifacts.
//!
//! Stores normalized WebP artifacts below a single canonical application directory,
//! using atomic write-and-rename mechanics so read paths never observe partial files.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use uuid::Uuid;

use super::domain::{IconStorage, IconStorageError, IconStorageKey};

/// Filesystem storage rooted at one canonical, application-owned directory.
#[derive(Clone, Debug)]
pub struct FilesystemIconStorage {
    root: PathBuf,
}

impl FilesystemIconStorage {
    /// Creates the storage root when necessary and resolves it before accepting any key.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, IconStorageError> {
        fs::create_dir_all(root.as_ref()).map_err(|_| IconStorageError::Unavailable)?;
        let root = fs::canonicalize(root.as_ref()).map_err(|_| IconStorageError::Unavailable)?;
        Ok(Self { root })
    }

    fn path(&self, key: &IconStorageKey) -> PathBuf {
        self.root.join(key.as_str())
    }
}

#[async_trait]
impl IconStorage for FilesystemIconStorage {
    async fn get(&self, key: &IconStorageKey) -> Result<Option<Vec<u8>>, IconStorageError> {
        match fs::read(self.path(key)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(IconStorageError::Unavailable),
        }
    }

    async fn put_atomic(&self, key: &IconStorageKey, bytes: &[u8]) -> Result<(), IconStorageError> {
        let target = self.path(key);
        if target.exists() {
            return Ok(());
        }
        let temporary = self.root.join(format!(".{}.tmp", Uuid::new_v4()));
        fs::write(&temporary, bytes).map_err(|_| IconStorageError::Unavailable)?;
        match fs::rename(&temporary, &target) {
            Ok(()) => Ok(()),
            Err(_) if target.exists() => {
                let _ = fs::remove_file(temporary);
                Ok(())
            }
            Err(_) => {
                let _ = fs::remove_file(temporary);
                Err(IconStorageError::Unavailable)
            }
        }
    }

    async fn exists(&self, key: &IconStorageKey) -> Result<bool, IconStorageError> {
        match fs::metadata(self.path(key)) {
            Ok(metadata) => Ok(metadata.is_file()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(IconStorageError::Unavailable),
        }
    }

    async fn delete(&self, key: &IconStorageKey) -> Result<(), IconStorageError> {
        match fs::remove_file(self.path(key)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(IconStorageError::Unavailable),
        }
    }
}
