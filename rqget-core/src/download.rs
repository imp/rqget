//! Run one guarded transfer and converge every post-session exit through shutdown.

use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Session, SessionOptions, TorrentStatsState,
};
use tokio::time::{MissedTickBehavior, interval, timeout};

use crate::{Request, destination::Destination, request::Kind};

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// An observation emitted without granting the caller access to engine internals.
#[derive(Debug)]
pub enum Event {
    Metadata,
    Transfer(Snapshot),
    Stopping,
}

/// Engine progress with checking distinguished from completed payload bytes.
#[derive(Debug)]
pub struct Snapshot {
    pub checking: bool,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub finished: bool,
    pub download_bytes_per_second: u64,
    pub upload_bytes_per_second: u64,
    pub connected_peers: u32,
    /// The engine's approximate human-readable ETA, when available.
    pub remaining: Option<String>,
}

impl From<&librqbit::TorrentStats> for Snapshot {
    fn from(stats: &librqbit::TorrentStats) -> Self {
        let live = stats.live.as_ref();
        Self {
            checking: matches!(stats.state, TorrentStatsState::Initializing { .. }),
            completed_bytes: stats.progress_bytes,
            total_bytes: stats.total_bytes,
            finished: stats.finished,
            download_bytes_per_second: live.map_or(0, |live| live.download_speed.as_bytes()),
            upload_bytes_per_second: live.map_or(0, |live| live.upload_speed.as_bytes()),
            connected_peers: live.map_or(0, |live| live.snapshot.peer_stats.live),
            remaining: live.and_then(|live| live.time_remaining.as_ref().map(ToString::to_string)),
        }
    }
}

/// Run a transfer with engine options, interruption, and a nonblocking observer.
///
/// # Errors
/// Returns contextual source, destination, engine, or shutdown errors. Interruption
/// returns its supplied exit status after stopping the session and releasing its lock.
pub async fn run(
    request: Request,
    options: SessionOptions,
    interrupted: impl Future<Output = Result<u8>>,
    mut observe: impl FnMut(Event),
) -> Result<u8> {
    tokio::pin!(interrupted);
    let source = match request.source.0 {
        Kind::File(path) => tokio::select! {
            biased;
            result = &mut interrupted => return result,
            bytes = tokio::fs::read(&path) => AddTorrent::from_bytes(bytes.with_context(|| format!("cannot read torrent file {path}"))?),
        },
        Kind::Magnet(uri) => AddTorrent::from_url(uri),
    };
    let destination = Destination::open(&request.output, request.resume)?;
    // Keep creation alive until it returns: dropping an in-progress engine constructor
    // could abandon its spawned tasks before a Session handle exists to stop them.
    let session =
        Session::new_with_opts(destination.root().to_owned().into_std_path_buf(), options)
            .await
            .context("cannot start torrent session")?;
    let result = async {
        let mut metadata_ticks = interval(REFRESH_INTERVAL);
        metadata_ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let resolving = session.add_torrent(source, Some(AddTorrentOptions { list_only: true, sub_folder: Some(String::new()), ..Default::default() }));
        tokio::pin!(resolving);
        let metadata = loop {
            tokio::select! {
                biased;
                result = &mut interrupted => return result,
                result = &mut resolving => break result.context("cannot resolve supported torrent metadata")?,
                _ = metadata_ticks.tick() => observe(Event::Metadata),
            }
        };
        let AddTorrentResponse::ListOnly(metadata) = metadata else { bail!("engine did not return preflight metadata"); };
        let overwrite = destination.prepare(&metadata)?;
        let AddTorrentResponse::Added(_, handle) = session.add_torrent(AddTorrent::from_bytes(metadata.torrent_bytes), Some(AddTorrentOptions { overwrite, sub_folder: Some(String::new()), ..Default::default() })).await.context("cannot start payload download")? else { bail!("engine did not start the requested transfer"); };
        let mut ticks = interval(REFRESH_INTERVAL);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                result = &mut interrupted => return result,
                _ = ticks.tick() => {
                    let stats = handle.stats();
                    ensure!(!matches!(stats.state, TorrentStatsState::Error), "transfer failed: {}", stats.error.as_deref().unwrap_or("unknown engine error"));
                    ensure!(!matches!(stats.state, TorrentStatsState::Paused), "transfer paused unexpectedly");
                    observe(Event::Transfer(Snapshot::from(&stats)));
                    if stats.finished { return Ok(0); }
                },
            }
        }
    }.await;
    observe(Event::Stopping);
    let stopped = timeout(SHUTDOWN_TIMEOUT, session.stop())
        .await
        .context("transfer shutdown exceeded five seconds");
    // Destination ownership remains held through shutdown, including failures.
    drop(destination);
    match (result, stopped) {
        (Err(error), Err(shutdown)) => {
            Err(error.context(format!("also failed to stop the session: {shutdown:#}")))
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
        (Ok(status), Ok(())) => Ok(status),
    }
}
