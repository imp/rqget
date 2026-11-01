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
        let error = timeout(
            Duration::from_secs(10),
            run(
                request(input.as_str(), &destination, false)?,
                options,
                pending(),
                |_| {},
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
