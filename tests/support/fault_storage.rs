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
#[derive(Clone, Default)]
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
            attempts: self.0.clone(),
        })
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

pub struct FailingStorage {
    inner: Box<dyn TorrentStorage>,
    attempts: Arc<AtomicUsize>,
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
        self.inner.pread_exact(file_id, offset, buffer)
    }

    fn pwrite_all(&self, _file_id: usize, _offset: u64, _buffer: &[u8]) -> anyhow::Result<()> {
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
            attempts: self.attempts.clone(),
        }))
    }
}
