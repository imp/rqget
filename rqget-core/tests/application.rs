//! Exercise the public core API with local trackers and deterministic peers.

use anyhow::Result;
use camino::Utf8Path;
use librqbit::{SessionOptions, storage::StorageFactoryExt};
use rqget_core::{Request, Source, run};
use rqget_test as support;
use rqget_test::tracker;
use std::{future::pending, time::Duration};
use tokio::time::timeout;

fn request(source: &str, output: &Utf8Path, resume: bool) -> Result<Request> {
    Ok(Request {
        source: Source::parse(source.to_owned())?,
        output: output.to_owned(),
        resume,
        peers: Vec::new(),
    })
}

fn local_options() -> SessionOptions {
    SessionOptions {
        dht: None,
        disable_trackers: true,
        disable_local_service_discovery: true,
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_lengths_fail_without_payload_writes_or_unwinding_the_caller() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let empty = b"d4:infod6:lengthi0e4:name5:empty12:piece lengthi16384e6:pieces0:ee".to_vec();
    let mut invalid_piece =
        b"d4:infod6:lengthi1e4:name5:empty12:piece lengthi0e6:pieces20:".to_vec();
    invalid_piece.extend_from_slice(&[0; 20]);
    invalid_piece.extend_from_slice(b"ee");
    for (index, metainfo) in [empty, invalid_piece].into_iter().enumerate() {
        let input = root.path().join(format!("invalid-{index}.torrent"));
        let output = root.path().join(format!("out-{index}"));
        std::fs::write(&input, metainfo)?;
        timeout(
            Duration::from_secs(5),
            run(
                request(input.as_str(), &output, false)?,
                local_options(),
                pending(),
                |_| {},
            ),
        )
        .await?
        .expect_err("unsupported metainfo must fail the job");
        assert_eq!(std::fs::read_dir(&output)?.count(), 1);
        assert!(!output.join(".rqget/state.json").exists());
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(output.join(".rqget/lock"))?;
        lock.try_lock()?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn core_reports_fatal_write_errors_and_releases_destination_ownership() -> Result<()> {
    let fixture = support::Fixture::new(true).await?;
    let (seeder, peer) = support::seed(&fixture).await?;
    let tracker = tracker::Tracker::start(peer)?;
    let root = camino_tempfile::tempdir()?;
    let input = root.path().join("input.torrent");
    std::fs::write(&input, fixture.torrent.as_bytes()?)?;
    let destination = root.path().join("out");
    let result = async {
        let mut options = local_options();
        options.disable_trackers = false;
        options.trackers.insert(tracker.url.clone());
        options.default_storage_factory =
            Some(support::fault_storage::FailWrites::default().boxed());
        let (signal, receive) = tokio::sync::oneshot::channel();
        let mut signal = Some(signal);
        let interrupted = async { receive.await.map_err(anyhow::Error::from) };
        let error = timeout(
            Duration::from_secs(10),
            run(
                request(input.as_str(), &destination, false)?,
                options,
                interrupted,
                |event| {
                    if matches!(event, rqget_core::Event::Stopping)
                        && let Some(signal) = signal.take()
                    {
                        let _ = signal.send(143);
                    }
                },
            ),
        )
        .await?
        .expect_err("a fatal payload write must fail the download");
        assert!(format!("{error:#}").contains("injected payload write failure"));
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(destination.join(".rqget/lock"))?;
        lock.try_lock()?;
        assert!(destination.join(".rqget/state.json").is_file());
        Ok(())
    }
    .await;
    result.and(support::stop(&seeder).await)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn application_resolves_a_magnet_and_downloads_a_single_file() -> Result<()> {
    let fixture = support::Fixture::new(true).await?;
    let (seeder, peer) = support::seed(&fixture).await?;
    let tracker = tracker::Tracker::start(peer)?;
    let destination = camino_tempfile::tempdir()?;
    let result = async {
        let mut options = local_options();
        options.disable_trackers = false;
        options.trackers.insert(tracker.url.clone());
        let request = request(
            &fixture.torrent.as_magnet().to_string(),
            destination.path(),
            false,
        )?;
        let mut snapshots = Vec::new();
        let status = timeout(
            Duration::from_secs(30),
            run(request, options, pending(), |event| {
                if let rqget_core::Event::Transfer(snapshot) = event {
                    snapshots.push(snapshot);
                }
            }),
        )
        .await??;
        assert_eq!(status, 0);
        assert!(snapshots.last().is_some_and(|snapshot| snapshot.finished));
        fixture.assert_payload(destination.path())?;
        Ok(())
    }
    .await;
    result.and(support::stop(&seeder).await)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn explicit_peers_bootstrap_metainfo_and_magnets_and_repair_resume() -> Result<()> {
    let fixture = support::Fixture::new(false).await?;
    let (seeder, peer) = support::seed(&fixture).await?;
    let root = camino_tempfile::tempdir()?;
    let input = root.path().join("input.torrent");
    std::fs::write(&input, fixture.torrent.as_bytes()?)?;
    let unavailable = std::net::TcpListener::bind("127.0.0.1:0")?;
    let unavailable_peer = unavailable.local_addr()?;
    drop(unavailable);
    let result = async {
        for (name, source) in [
            ("metainfo", input.to_string()),
            ("magnet", fixture.torrent.as_magnet().to_string()),
        ] {
            let output = root.path().join(name);
            let mut initial = request(&source, &output, false)?;
            initial.peers = vec![unavailable_peer, peer];
            assert_eq!(
                timeout(
                    Duration::from_secs(20),
                    run(initial, local_options(), pending(), |_| {}),
                )
                .await??,
                0
            );
            fixture.assert_payload(&output)?;
            let record = std::fs::read(output.join(".rqget/state.json"))?;
            let damaged = output.join(&fixture.files[0].0);
            let mut bytes = std::fs::read(&damaged)?;
            bytes[0] ^= 255;
            std::fs::write(&damaged, bytes)?;
            let mut resumed = request(&source, &output, true)?;
            resumed.peers = vec![peer];
            assert_eq!(
                timeout(
                    Duration::from_secs(20),
                    run(resumed, local_options(), pending(), |_| {}),
                )
                .await??,
                0
            );
            fixture.assert_payload(&output)?;
            assert_eq!(std::fs::read(output.join(".rqget/state.json"))?, record);
        }
        Ok(())
    }
    .await;
    result.and(support::stop(&seeder).await)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn application_downloads_then_rechecks_and_repairs_explicit_resume() -> Result<()> {
    let fixture = support::Fixture::new(false).await?;
    let (seeder, peer) = support::seed(&fixture).await?;
    let tracker = tracker::Tracker::start(peer)?;
    let root = camino_tempfile::tempdir()?;
    let input = root.path().join("input é.torrent");
    std::fs::write(&input, fixture.torrent.as_bytes()?)?;
    let destination = root.path().join("output é");
    let run = async {
        let mut options = local_options();
        options.disable_trackers = false;
        options.trackers.insert(tracker.url.clone());
        let initial = request(input.as_str(), &destination, false)?;
        assert_eq!(
            timeout(
                Duration::from_secs(30),
                run(
                    request(input.as_str(), &destination, false)?,
                    options,
                    pending(),
                    |_| {}
                )
            )
            .await??,
            0
        );
        fixture.assert_payload(&destination)?;
        assert!(destination.join(".rqget/state.json").is_file());
        run(initial, local_options(), pending(), |_| {})
            .await
            .expect_err("an existing download requires --resume");
        // A complete matching transfer rechecks successfully with no peers or trackers.
        assert_eq!(
            timeout(
                Duration::from_secs(5),
                run(
                    request(input.as_str(), &destination, true)?,
                    local_options(),
                    pending(),
                    |_| {}
                )
            )
            .await??,
            0
        );
        let damaged = destination.join(&fixture.files[0].0);
        let mut bytes = std::fs::read(&damaged)?;
        bytes[0] ^= 255;
        std::fs::write(&damaged, bytes)?;
        let mut options = local_options();
        options.disable_trackers = false;
        options.trackers.insert(tracker.url.clone());
        assert_eq!(
            timeout(
                Duration::from_secs(30),
                run(
                    request(input.as_str(), &destination, true)?,
                    options,
                    pending(),
                    |_| {}
                )
            )
            .await??,
            0
        );
        fixture.assert_payload(&destination)?;
        Ok(())
    }
    .await;
    run.and(support::stop(&seeder).await)
}
