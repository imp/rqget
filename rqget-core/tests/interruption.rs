//! Prove lifecycle interruption against controlled local transfers and startup I/O.

use std::{future::pending, num::NonZeroU32, time::Duration};

use anyhow::{Context, Result};
use librqbit::{SessionOptions, limits::LimitsConfig};
use rqget_core::{Event, Request, Source, run};
use rqget_test::{Fixture, seed, stop, tracker::Tracker};
use tokio::{io::AsyncReadExt, sync::oneshot, time::timeout};

fn options() -> SessionOptions {
    SessionOptions {
        dht: None,
        disable_local_service_discovery: true,
        disable_trackers: true,
        ..Default::default()
    }
}

fn request(source: String, destination: &camino::Utf8Path, resume: bool) -> Result<Request> {
    Ok(Request {
        source: Source::parse(source)?,
        output: destination.to_owned(),
        resume,
        peers: Vec::new(),
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interruption_during_shutdown_preserves_data_and_changes_the_exit_status() -> Result<()> {
    let fixture = Fixture::new(true).await?;
    let (seeder, peer) = seed(&fixture).await?;
    let tracker = Tracker::start(peer)?;
    let root = camino_tempfile::tempdir()?;
    let input = root.path().join("input.torrent");
    std::fs::write(&input, fixture.torrent.as_bytes()?)?;
    let destination = root.path().join("out");
    let result = async {
        let mut config = options();
        config.disable_trackers = false;
        config.trackers.insert(tracker.url.clone());
        let (signal, receive) = oneshot::channel();
        let mut signal = Some(signal);
        let interrupted = async { receive.await.context("shutdown signal sender dropped") };
        let status = timeout(
            Duration::from_secs(10),
            run(
                request(input.to_string(), &destination, false)?,
                config,
                interrupted,
                |event| {
                    if matches!(event, Event::Stopping)
                        && let Some(signal) = signal.take()
                    {
                        let _ = signal.send(143);
                    }
                },
            ),
        )
        .await??;
        assert_eq!(status, 143);
        fixture.assert_payload(&destination)?;
        Ok(())
    }
    .await;
    result.and(stop(&seeder).await)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interruption_cannot_masquerade_as_success() -> Result<()> {
    let fixture = Fixture::new(true).await?;
    let root = camino_tempfile::tempdir()?;
    let input = root.path().join("input.torrent");
    std::fs::write(&input, fixture.torrent.as_bytes()?)?;
    let destination = root.path().join("out");
    run(
        request(input.to_string(), &destination, false)?,
        options(),
        async { Ok(0) },
        |_| {},
    )
    .await
    .expect_err("only engine completion can produce status zero");
    assert!(!destination.exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_peer_transfer_waits_until_cancelled_and_retains_its_record() -> Result<()> {
    let fixture = Fixture::new(true).await?;
    let unavailable = std::net::TcpListener::bind("127.0.0.1:0")?;
    let unavailable_peer = unavailable.local_addr()?;
    drop(unavailable);
    for peers in [vec![], vec![unavailable_peer]] {
        let root = camino_tempfile::tempdir()?;
        let input = root.path().join("input.torrent");
        std::fs::write(&input, fixture.torrent.as_bytes()?)?;
        let destination = root.path().join("out");
        let mut request = request(input.to_string(), &destination, false)?;
        request.peers = peers;
        let (signal, receive) = oneshot::channel();
        let mut signal = Some(signal);
        let interrupted = async { receive.await.context("waiting-state observer dropped") };
        let status = timeout(
            Duration::from_secs(5),
            run(request, options(), interrupted, |event| {
                if let Event::Transfer(snapshot) = event
                    && !snapshot.checking
                    && snapshot.connected_peers == 0
                    && !snapshot.finished
                    && let Some(signal) = signal.take()
                {
                    let _ = signal.send(130);
                }
            }),
        )
        .await??;
        assert_eq!(status, 130);
        assert!(destination.join(".rqget/state.json").exists());
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interrupted_partial_download_keeps_valid_pieces_and_resumes() -> Result<()> {
    let fixture = Fixture::new(false).await?;
    let (seeder, peer) = seed(&fixture).await?;
    let tracker = Tracker::start(peer)?;
    let root = camino_tempfile::tempdir()?;
    let input = root.path().join("input.torrent");
    std::fs::write(&input, fixture.torrent.as_bytes()?)?;
    let destination = root.path().join("out");
    let result = async {
        let mut limited = options();
        limited.disable_trackers = false;
        limited.trackers.insert(tracker.url.clone());
        limited.ratelimits = LimitsConfig {
            download_bps: NonZeroU32::new(32 * 1024),
            ..Default::default()
        };
        let (signal, receive) = oneshot::channel();
        let mut signal = Some(signal);
        let mut retained_bytes = 0;
        let interrupted = async {
            timeout(Duration::from_secs(15), receive)
                .await
                .context("no partial progress observed")?
                .context("interruption sender dropped")
        };
        let status = run(
            request(input.to_string(), &destination, false)?,
            limited,
            interrupted,
            |event| match event {
                Event::Transfer(snapshot)
                    if !snapshot.checking && snapshot.completed_bytes > 0 && !snapshot.finished =>
                {
                    retained_bytes = snapshot.completed_bytes;
                    if let Some(signal) = signal.take() {
                        let _ = signal.send(130);
                    }
                }
                Event::Stopping => {
                    let file = std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(destination.join(".rqget/lock"))
                        .expect("owned lock exists");
                    assert!(
                        matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
                        "ownership was released before stopping"
                    );
                }
                _ => {}
            },
        )
        .await?;
        assert_eq!(status, 130);
        assert!(retained_bytes > 0);
        assert!(destination.join(".rqget/state.json").is_file());
        let mut resumed = options();
        resumed.disable_trackers = false;
        resumed.trackers.insert(tracker.url.clone());
        let status = timeout(
            Duration::from_secs(20),
            run(
                request(input.to_string(), &destination, true)?,
                resumed,
                pending(),
                |_| {},
            ),
        )
        .await??;
        assert_eq!(status, 0);
        fixture.assert_payload(&destination)?;
        Ok(())
    }
    .await;
    result.and(stop(&seeder).await)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metadata_lookup_is_interruptible_without_creating_payloads() -> Result<()> {
    let peer = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    let tracker = Tracker::start(peer.local_addr()?)?;
    let destination = camino_tempfile::tempdir()?;
    let mut config = options();
    config.disable_trackers = false;
    config.trackers.insert(tracker.url.clone());
    let (signal, receive) = oneshot::channel();
    let mut signal = Some(signal);
    let source = format!("magnet:?xt=urn:btih:{:040x}", 1);
    let interrupted = async { receive.await.context("signal sender dropped") };
    let status = timeout(
        Duration::from_secs(5),
        run(
            request(source, destination.path(), false)?,
            config,
            interrupted,
            |event| {
                if matches!(event, Event::Metadata)
                    && let Some(signal) = signal.take()
                {
                    let _ = signal.send(130);
                }
            },
        ),
    )
    .await??;
    assert_eq!(status, 130);
    assert!(!destination.path().join(".rqget/state.json").exists());
    assert_eq!(std::fs::read_dir(destination.path())?.count(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_http_io_can_be_cancelled_before_a_session_is_returned() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let input = root.path().join("input.torrent");
    std::fs::write(&input, b"not needed before startup finishes")?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let mut config = options();
    config.blocklist_url = Some(format!("http://{}/blocked", listener.local_addr()?));
    let (started, receive) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let mut bytes = [0; 1024];
        let count = stream.read(&mut bytes).await?;
        assert!(count > 0);
        let _ = started.send(());
        // Wait for the cancelled constructor to close its request connection.
        timeout(Duration::from_secs(5), stream.read(&mut bytes)).await??;
        Ok::<_, anyhow::Error>(())
    });
    let interrupted = async {
        receive.await.context("startup request did not arrive")?;
        Ok(143)
    };
    let destination = root.path().join("out");
    let status = timeout(
        Duration::from_secs(5),
        run(
            request(input.to_string(), &destination, false)?,
            config,
            interrupted,
            |_| {},
        ),
    )
    .await??;
    assert_eq!(status, 143);
    server.await??;
    assert!(!destination.join(".rqget/state.json").exists());
    Ok(())
}
