//! Repeatable loopback fixtures for rqget integration and storage-boundary tests.

pub mod fault_storage;
pub mod tracker;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use camino::Utf8Path;
use camino_tempfile::Utf8TempDir;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, CreateTorrentOptions, CreateTorrentResult,
    ListenerOptions, ManagedTorrent, Session, SessionOptions, TorrentStats, TorrentStatsState,
    create_torrent, spawn_utils::BlockingSpawner,
};
use tokio::time::{MissedTickBehavior, interval, timeout};

const TEST_TIMEOUT: Duration = Duration::from_secs(20);

pub struct Fixture {
    pub root: Utf8TempDir,
    pub torrent: CreateTorrentResult,
    pub files: Vec<(String, Vec<u8>)>,
}

impl std::fmt::Debug for Fixture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fixture")
            .field("root", &self.root)
            .field("info_hash", &self.torrent.info_hash())
            .finish_non_exhaustive()
    }
}

impl Fixture {
    /// Generate repeatable payload bytes and corresponding engine-produced metainfo.
    ///
    /// # Errors
    /// Returns filesystem or torrent-generation errors.
    pub async fn new(single_file: bool) -> Result<Self> {
        let root = camino_tempfile::tempdir()?;
        let names = if single_file {
            vec!["payload.bin"]
        } else {
            vec!["first.bin", "nested/second file-é.bin"]
        };
        let mut files = Vec::with_capacity(names.len());
        for (index, name) in names.into_iter().enumerate() {
            // Repeatable uneven file sizes make pieces cross file boundaries.
            let data = (0..(131_117 + index * 9_013))
                .map(|offset| u8::try_from((offset * 31 + index * 17) % 251))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let path = root.path().join(name);
            std::fs::create_dir_all(path.parent().context("fixture parent")?)?;
            std::fs::write(path, &data)?;
            files.push((name.to_owned(), data));
        }
        let source = if single_file {
            root.path().join(&files[0].0)
        } else {
            root.path().to_owned()
        };
        let torrent = create_torrent(
            source.as_std_path(),
            CreateTorrentOptions {
                name: Some(if single_file {
                    "payload.bin"
                } else {
                    "rqget-local-fixture"
                }),
                piece_length: Some(32_768),
                ..Default::default()
            },
            &BlockingSpawner::new(1),
        )
        .await?;
        Ok(Self {
            root,
            torrent,
            files,
        })
    }

    /// Compare downloaded files against the original fixture bytes.
    ///
    /// # Errors
    /// Returns a filesystem read error.
    ///
    /// # Panics
    /// Panics if a downloaded file differs from its expected bytes.
    pub fn assert_payload(&self, destination: &Utf8Path) -> Result<()> {
        for (name, expected) in &self.files {
            let actual = std::fs::read(destination.join(name))?;
            assert_eq!(&actual, expected, "payload mismatch for {name}");
        }
        Ok(())
    }
}

/// A session restricted to explicitly supplied peers: no trackers, DHT, LSD, or persistence.
///
/// # Errors
/// Returns an engine session initialization error.
pub async fn session(root: &Utf8Path, listen: bool) -> Result<Arc<Session>> {
    Session::new_with_opts(
        root.to_owned().into_std_path_buf(),
        SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            listen: listen.then(|| ListenerOptions {
                listen_addr: ([127, 0, 0, 1], 0).into(),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
}

/// Serve a checked fixture over an ephemeral loopback TCP listener.
///
/// # Errors
/// Returns a session, initialization, checking, or listener error.
pub async fn seed(fixture: &Fixture) -> Result<(Arc<Session>, SocketAddr)> {
    let session = session(fixture.root.path(), true).await?;
    let result = async {
        let handle = added(
            session
                .add_torrent(
                    AddTorrent::from_bytes(fixture.torrent.as_bytes()?),
                    Some(AddTorrentOptions {
                        sub_folder: Some(String::new()),
                        overwrite: true,
                        ..Default::default()
                    }),
                )
                .await?,
        )?;
        wait_for(&handle, |stats| stats.finished).await?;
        session.listen_addr().context("seeder did not listen")
    }
    .await;
    match result {
        Ok(address) => Ok((session, address)),
        Err(error) => {
            stop(&session).await?;
            Err(error)
        }
    }
}

/// Require a newly added transfer handle from an engine response.
///
/// # Errors
/// Returns an error for duplicate or list-only responses.
pub fn added(response: AddTorrentResponse) -> Result<Arc<ManagedTorrent>> {
    match response {
        AddTorrentResponse::Added(_, handle) => Ok(handle),
        AddTorrentResponse::AlreadyManaged(..) => bail!("unexpected duplicate torrent"),
        AddTorrentResponse::ListOnly(_) => bail!("expected a transfer handle"),
    }
}

/// Observe engine stats until a condition matches or the bounded wait fails.
///
/// # Errors
/// Returns an error for a fatal engine state or an observation timeout.
pub async fn wait_for(
    handle: &ManagedTorrent,
    condition: impl Fn(&TorrentStats) -> bool,
) -> Result<TorrentStats> {
    timeout(TEST_TIMEOUT, async {
        let mut ticks = interval(Duration::from_millis(10));
        ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            ticks.tick().await;
            let stats = handle.stats();
            if condition(&stats) {
                return Ok(stats);
            }
            if matches!(stats.state, TorrentStatsState::Error) {
                bail!(
                    "engine failed: {}",
                    stats.error.as_deref().unwrap_or("unknown error")
                );
            }
        }
    })
    .await
    .with_context(|| {
        format!(
            "timed out waiting for torrent; last stats: {}",
            handle.stats()
        )
    })?
}

/// Stop a session and verify that its cancellation token was cancelled.
///
/// # Errors
/// Returns an error if shutdown times out or cancellation was not applied.
pub async fn stop(session: &Session) -> Result<()> {
    timeout(Duration::from_secs(5), session.stop())
        .await
        .context("session shutdown timed out")?;
    anyhow::ensure!(
        session.cancellation_token().is_cancelled(),
        "session was not cancelled"
    );
    Ok(())
}

/// Resolve local fixture metainfo without creating payload files.
///
/// # Errors
/// Returns a session, metainfo, or bounded-shutdown error.
pub async fn metadata(bytes: Vec<u8>, root: &Utf8Path) -> Result<librqbit::ListOnlyResponse> {
    let client = session(root, false).await?;
    let response = client
        .add_torrent(
            AddTorrent::from_bytes(bytes),
            Some(AddTorrentOptions {
                list_only: true,
                sub_folder: Some(String::new()),
                ..Default::default()
            }),
        )
        .await;
    stop(&client).await?;
    match response? {
        AddTorrentResponse::ListOnly(metadata) => Ok(metadata),
        _ => bail!("expected fixture metadata"),
    }
}

/// Tiny deliberately adversarial metainfo fixtures, not an application codec.
#[must_use]
pub fn metainfo(paths: &[&[&[u8]]]) -> Vec<u8> {
    fn bytes(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(format!("{}:", value.len()).as_bytes());
        output.extend_from_slice(value);
    }
    let mut output = b"d4:infod5:filesl".to_vec();
    for path in paths {
        output.extend_from_slice(b"d6:lengthi1e4:pathl");
        for component in *path {
            bytes(&mut output, component);
        }
        output.extend_from_slice(b"ee");
    }
    output.extend_from_slice(b"e4:name7:fixture12:piece lengthi16384e6:pieces20:");
    output.extend_from_slice(&[0; 20]);
    output.extend_from_slice(b"ee");
    output
}
