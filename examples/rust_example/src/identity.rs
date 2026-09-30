//! Same-host singleton ownership uses a kernel lock, never the existence of a file.
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
use uuid::Uuid;

/// Holding this object holds the lock. OS cleanup releases it even after a crash.
pub struct NodeLock {
    _file: File,
}
impl NodeLock {
    pub fn acquire(directory: &Path, id: Uuid) -> std::io::Result<Self> {
        // Shared host directory must already exist with the operator's chosen ownership.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join(format!("{id}.lock")))?;
        // std::fs locking avoids an extra platform crate on modern stable Rust.
        file.try_lock().map_err(std::io::Error::other)?;
        Ok(Self { _file: file })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_file_is_not_a_lock_and_drop_releases() {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let owner = NodeLock::acquire(dir.path(), id).unwrap();
        assert!(NodeLock::acquire(dir.path(), id).is_err());
        drop(owner);
        assert!(NodeLock::acquire(dir.path(), id).is_ok());
    }
}
