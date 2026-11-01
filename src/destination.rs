//! Own exclusive destination access and authorize contained payload writes.

use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::Write,
};

use anyhow::{Context, Result, bail, ensure};
use camino::{Utf8Path, Utf8PathBuf};
use librqbit::ListOnlyResponse;
use serde::{Deserialize, Serialize};

const STATE_DIRECTORY: &str = ".rqget";
const RECORD_NAME: &str = "state.json";
const LOCK_NAME: &str = "lock";

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PayloadFile {
    path: Utf8PathBuf,
    length: u64,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    info_hash: String,
    files: Vec<PayloadFile>,
}

/// Holds the stable kernel lock until the engine has shut down.
#[derive(Debug)]
pub(crate) struct Destination {
    root: Utf8PathBuf,
    state: Utf8PathBuf,
    _lock: File,
    previous: Option<Record>,
}

impl Destination {
    pub fn open(path: &Utf8Path, resume: bool) -> Result<Self> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("cannot create destination {path}"))?;
        let root = Utf8PathBuf::from_path_buf(std::fs::canonicalize(path)?)
            .map_err(|_| anyhow::anyhow!("canonical destination path is not UTF-8"))?;
        ensure!(root.is_dir(), "destination is not a directory");
        let state = root.join(STATE_DIRECTORY);
        match std::fs::symlink_metadata(&state) {
            Ok(metadata) => ensure!(
                metadata.is_dir() && !metadata.is_symlink(),
                "destination state must be a real directory"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&state)?
            }
            Err(error) => return Err(error).context("cannot inspect destination state"),
        }
        let lock_path = state.join(LOCK_NAME);
        ensure_regular_or_missing(&lock_path)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        lock.try_lock()
            .context("destination is in use or cannot be locked")?;
        for entry in std::fs::read_dir(&state)? {
            let entry = entry?;
            ensure!(
                entry.file_name() == LOCK_NAME || entry.file_name() == RECORD_NAME,
                "destination state contains an unknown file; inspect it before retrying"
            );
        }
        let record_path = state.join(RECORD_NAME);
        ensure_regular_or_missing(&record_path)?;
        let previous = if resume {
            let bytes = std::fs::read(&record_path)
                .context("resume requires a complete rqget state record")?;
            let record: Record =
                serde_json::from_slice(&bytes).context("invalid rqget state record")?;
            ensure!(record.version == 1, "unsupported rqget state version");
            Some(record)
        } else {
            ensure!(
                !record_path.exists(),
                "destination belongs to a previous download; use --resume"
            );
            ensure!(
                std::fs::read_dir(&root)?
                    .all(|entry| entry.is_ok_and(|e| e.file_name() == STATE_DIRECTORY)),
                "new download requires an empty dedicated destination"
            );
            None
        };
        Ok(Self {
            root,
            state,
            _lock: lock,
            previous,
        })
    }

    pub fn root(&self) -> &Utf8Path {
        &self.root
    }

    /// Validate metadata and existing paths, then publish ownership before any payload open.
    pub fn prepare(&self, metadata: &ListOnlyResponse) -> Result<bool> {
        let record = Record::from_metadata(metadata)?;
        self.check_filesystem_layout(&record)?;
        if let Some(previous) = &self.previous {
            ensure!(
                previous == &record,
                "resume torrent identity or payload layout does not match the destination"
            );
            self.check_existing_tree(&self.root, &record)?;
            Ok(true)
        } else {
            // Check again after metadata resolution while still holding exclusive ownership.
            ensure!(
                std::fs::read_dir(&self.root)?
                    .all(|entry| entry.is_ok_and(|e| e.file_name() == STATE_DIRECTORY)),
                "destination changed during metadata resolution"
            );
            let mut staged = camino_tempfile::NamedUtf8TempFile::new_in(&self.state)?;
            serde_json::to_writer(staged.as_file_mut(), &record)
                .context("cannot serialize destination record")?;
            staged.flush().context("cannot write destination record")?;
            staged
                .persist_noclobber(self.state.join(RECORD_NAME))
                .context("cannot publish destination record")?;
            Ok(false)
        }
    }

    fn check_filesystem_layout(&self, record: &Record) -> Result<()> {
        // Same filesystem: detect native case/Unicode aliases and file/directory conflicts.
        // Only this disposable state tree is touched, never the payload tree.
        let staging = camino_tempfile::tempdir_in(&self.state)?;
        for file in &record.files {
            let path = staging.path().join(&file.path);
            std::fs::create_dir_all(path.parent().context("payload path has no parent")?)
                .with_context(|| format!("conflicting payload path: {}", file.path))?;
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .with_context(|| {
                    format!(
                        "duplicate, conflicting, or unsupported payload path: {}",
                        file.path
                    )
                })?;
        }
        Ok(())
    }

    fn check_existing_tree(&self, directory: &Utf8Path, record: &Record) -> Result<()> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = Utf8PathBuf::from_path_buf(entry.path())
                .map_err(|_| anyhow::anyhow!("destination contains a non-UTF-8 path"))?;
            if path == self.state {
                continue;
            }
            let kind = entry.file_type()?;
            ensure!(
                !kind.is_symlink(),
                "destination payload path is a symlink: {path}"
            );
            let relative = path.strip_prefix(&self.root)?;
            if kind.is_dir() {
                ensure!(
                    record
                        .files
                        .iter()
                        .any(|f| f.path.starts_with(relative) && f.path != relative),
                    "destination contains an unrelated directory: {relative}"
                );
                self.check_existing_tree(&path, record)?;
            } else {
                ensure!(
                    kind.is_file() && record.files.iter().any(|f| f.path == relative),
                    "destination contains an unrelated or unsupported file: {relative}"
                );
            }
        }
        Ok(())
    }
}

impl Record {
    fn from_metadata(metadata: &ListOnlyResponse) -> Result<Self> {
        let mut files = Vec::new();
        let mut unique = BTreeSet::new();
        for details in metadata.info.iter_file_details() {
            if details.attrs().padding {
                continue;
            }
            let mut path = Utf8PathBuf::new();
            for (index, raw) in details.filename.iter_components_bytes().enumerate() {
                let part =
                    std::str::from_utf8(raw).context("torrent payload filename must be UTF-8")?;
                ensure!(
                    !part.is_empty()
                        && part != "."
                        && part != ".."
                        && !part.contains(['/', '\\', '\0']),
                    "invalid torrent payload path component"
                );
                ensure!(
                    index != 0 || !part.eq_ignore_ascii_case(STATE_DIRECTORY),
                    "torrent payload uses the reserved .rqget directory"
                );
                path.push(part);
            }
            ensure!(
                !path.as_str().is_empty() && !path.is_absolute(),
                "invalid torrent payload path"
            );
            ensure!(
                unique.insert(path.clone()),
                "duplicate torrent payload path: {path}"
            );
            files.push(PayloadFile {
                path,
                length: details.len,
            });
        }
        // Lexical prefix conflicts are rejected before the native filesystem probe.
        for path in &unique {
            for ancestor in path.ancestors().skip(1) {
                ensure!(
                    !unique.contains(ancestor),
                    "payload path is both a file and a directory: {ancestor}"
                );
            }
        }
        Ok(Self {
            version: 1,
            info_hash: metadata.info_hash.as_string(),
            files,
        })
    }
}

fn ensure_regular_or_missing(path: &Utf8Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => Ok(()),
        Ok(_) => bail!("destination state path is not a regular file: {path}"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("cannot inspect {path}")),
    }
}
