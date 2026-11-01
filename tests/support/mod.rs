pub mod fault_storage;

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

impl Fixture {
    pub async fn new(single_file: bool) -> Result<Self> {
        let root = camino_tempfile::tempdir()?;
        let names = if single_file {
            vec!["payload.bin"]
        } else {
            vec!["first.bin", "nested/second file-é.bin"]
        };
        let mut files = Vec::new();
        for (index, name) in names.into_iter().enumerate() {
            // Repeatable uneven file sizes make pieces cross file boundaries.
            let data = (0..(131_117 + index * 9_013))
                .map(|offset| u8::try_from((offset * 31 + index * 17) % 251).unwrap())
                .collect::<Vec<_>>();
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

    pub fn assert_payload(&self, destination: &Utf8Path) -> Result<()> {
        for (name, expected) in &self.files {
            let actual = std::fs::read(destination.join(name))?;
            assert_eq!(&actual, expected, "payload mismatch for {name}");
        }
        Ok(())
    }
}

/// A session restricted to explicitly supplied peers: no trackers, DHT, LSD, or persistence.
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

pub fn added(response: AddTorrentResponse) -> Result<Arc<ManagedTorrent>> {
    match response {
        AddTorrentResponse::Added(_, handle) => Ok(handle),
        AddTorrentResponse::AlreadyManaged(..) => bail!("unexpected duplicate torrent"),
        AddTorrentResponse::ListOnly(_) => bail!("expected a transfer handle"),
    }
}

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

/// Tiny deliberately adversarial metainfo fixtures, not an application codec.
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
