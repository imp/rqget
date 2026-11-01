use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use librqbit::{
    ManagedTorrentShared, TorrentMetadata,
    storage::{
        BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage,
        filesystem::FilesystemStorageFactory,
    },
};

/// Uses the library's own storage seam to fail payload writes without exhausting a real disk.
#[derive(Debug, Clone, Default)]
pub struct FailWrites(pub Arc<AtomicUsize>);

impl StorageFactory for FailWrites {
    type Storage = FailingStorage;

    fn create(
        &self,
        shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<Self::Storage> {
        Ok(FailingStorage {
            inner: Box::new(FilesystemStorageFactory::default().create(shared, metadata)?),
            attempts: Arc::clone(&self.0),
            gate: None,
            fail_writes: true,
        })
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

pub struct FailingStorage {
    inner: Box<dyn TorrentStorage>,
    attempts: Arc<AtomicUsize>,
    gate: Option<ReadGate>,
    fail_writes: bool,
}

impl std::fmt::Debug for FailingStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FailingStorage")
            .field("attempts", &self.attempts)
            .finish_non_exhaustive()
    }
}

impl TorrentStorage for FailingStorage {
    fn init(
        &mut self,
        shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<()> {
        self.inner.init(shared, metadata)
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buffer: &mut [u8]) -> anyhow::Result<()> {
        if let Some(gate) = &self.gate {
            gate.wait()?;
        }
        self.inner.pread_exact(file_id, offset, buffer)
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buffer: &[u8]) -> anyhow::Result<()> {
        if !self.fail_writes {
            return self.inner.pwrite_all(file_id, offset, buffer);
        }
        self.attempts.fetch_add(1, Ordering::Relaxed);
        Err(std::io::Error::other("injected payload write failure").into())
    }

    fn remove_file(&self, file_id: usize, filename: &Path) -> anyhow::Result<()> {
        self.inner.remove_file(file_id, filename)
    }

    fn remove_directory_if_empty(&self, path: &Path) -> anyhow::Result<()> {
        self.inner.remove_directory_if_empty(path)
    }

    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        self.inner.ensure_file_length(file_id, length)
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(Self {
            inner: self.inner.take()?,
            attempts: Arc::clone(&self.attempts),
            gate: self.gate.clone(),
            fail_writes: self.fail_writes,
        }))
    }
}

/// Pause fixture reads until the controlling test creates a release file.
#[derive(Debug, Clone)]
pub struct ReadGate {
    entered: camino::Utf8PathBuf,
    release: camino::Utf8PathBuf,
}

impl ReadGate {
    #[must_use]
    pub fn new(entered: camino::Utf8PathBuf, release: camino::Utf8PathBuf) -> Self {
        Self { entered, release }
    }

    fn wait(&self) -> anyhow::Result<()> {
        std::fs::write(&self.entered, b"checking")?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        while !self.release.exists() {
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "read gate was not released"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }
}

impl StorageFactory for ReadGate {
    type Storage = FailingStorage;

    fn create(
        &self,
        shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<Self::Storage> {
        Ok(FailingStorage {
            inner: Box::new(FilesystemStorageFactory::default().create(shared, metadata)?),
            attempts: Arc::new(AtomicUsize::new(0)),
            gate: Some(self.clone()),
            fail_writes: false,
        })
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}
