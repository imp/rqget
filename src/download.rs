//! Run one guarded transfer and converge every post-session exit through shutdown.

use std::{future::Future, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Session, SessionOptions, TorrentStatsState,
};
use tokio::time::{MissedTickBehavior, interval, timeout};

use crate::{
    cli::{Request, Source},
    destination::Destination,
    progress::Progress,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn run(
    request: Request,
    options: SessionOptions,
    interrupted: impl Future<Output = Result<u8>>,
    progress: &mut Progress,
) -> Result<u8> {
    tokio::pin!(interrupted);
    let source = match request.source {
        Source::File(path) => tokio::select! {
            biased;
            result = &mut interrupted => return result,
            bytes = tokio::fs::read(&path) => AddTorrent::from_bytes(bytes.with_context(|| format!("cannot read torrent file {path}"))?),
        },
        Source::Magnet(uri) => AddTorrent::from_url(uri),
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
                _ = metadata_ticks.tick() => progress.metadata(),
            }
        };
        let AddTorrentResponse::ListOnly(metadata) = metadata else { bail!("engine did not return preflight metadata"); };
        let overwrite = destination.prepare(&metadata)?;
        let handle = match session.add_torrent(AddTorrent::from_bytes(metadata.torrent_bytes), Some(AddTorrentOptions { overwrite, sub_folder: Some(String::new()), ..Default::default() })).await.context("cannot start payload download")? {
            AddTorrentResponse::Added(_, handle) => handle,
            _ => bail!("engine did not start the requested transfer"),
        };
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
                    progress.update(&stats);
                    if stats.finished { return Ok(0); }
                },
            }
        }
    }.await;
    progress.shutdown();
    let stopped = timeout(SHUTDOWN_TIMEOUT, session.stop())
        .await
        .context("transfer shutdown exceeded five seconds");
    // Destination ownership remains held through shutdown, including failures.
    drop(destination);
    match (result, stopped) {
        (Err(error), Err(shutdown)) => {
            Err(error.context(format!("also failed to stop the session: {shutdown:#}")))
        }
        (Err(error), _) => Err(error),
        (_, Err(error)) => Err(error),
        (Ok(status), Ok(())) => Ok(status),
    }
}
