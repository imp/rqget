mod support;

use std::{
    io::{Seek, SeekFrom, Write},
    sync::atomic::Ordering,
};

use anyhow::{Context, Result, ensure};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, TorrentStatsState,
    storage::StorageFactoryExt,
};
use tokio::time::{Duration, timeout};

use support::{Fixture, added, metainfo, seed, session, stop, wait_for};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn local_metainfo_download_and_piece_recheck() -> Result<()> {
    download_and_recheck(true, false).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn magnet_metadata_and_multifile_download() -> Result<()> {
    download_and_recheck(false, true).await
}

async fn download_and_recheck(single_file: bool, magnet: bool) -> Result<()> {
    let fixture = Fixture::new(single_file).await?;
    let (seeder, peer) = seed(&fixture).await?;
    let destination = camino_tempfile::tempdir()?;
    let client = session(destination.path(), false).await?;
    let result = async {
        let source = if magnet {
            AddTorrent::from_url(fixture.torrent.as_magnet().to_string())
        } else {
            // Exercise the local metainfo file/bytes boundary, not just the generator's buffer.
            let input = fixture.root.path().join("input.torrent");
            std::fs::write(&input, fixture.torrent.as_bytes()?)?;
            AddTorrent::from_bytes(tokio::fs::read(&input).await?)
        };
        let preflight = timeout(
            Duration::from_secs(20),
            client.add_torrent(
                source,
                Some(AddTorrentOptions {
                    list_only: true,
                    sub_folder: Some(String::new()),
                    initial_peers: Some(vec![peer]),
                    ..Default::default()
                }),
            ),
        )
        .await??;
        let AddTorrentResponse::ListOnly(resolved) = preflight else {
            anyhow::bail!("list-only did not return metadata");
        };
        assert_eq!(resolved.info_hash, fixture.torrent.info_hash());
        assert_eq!(resolved.output_folder, destination.path().as_std_path());
        assert_eq!(
            std::fs::read_dir(destination.path())?.count(),
            0,
            "preflight wrote files"
        );
        let handle = added(
            client
                .add_torrent(
                    AddTorrent::from_bytes(resolved.torrent_bytes.clone()),
                    Some(AddTorrentOptions {
                        sub_folder: Some(String::new()),
                        initial_peers: Some(vec![peer]),
                        ..Default::default()
                    }),
                )
                .await?,
        )?;
        let completed = wait_for(&handle, |stats| stats.finished).await?;
        assert!(matches!(completed.state, TorrentStatsState::Live));
        assert_eq!(completed.progress_bytes, completed.total_bytes);
        assert_eq!(completed.file_progress.len(), fixture.files.len());
        let live = completed.live.context("missing live progress")?;
        assert!(
            live.snapshot.fetched_bytes > 0,
            "download never fetched bytes"
        );
        // Confirm the rate and ETA accessors needed by the future renderer are public.
        let _rates_and_eta = (
            live.download_speed.as_bytes(),
            live.upload_speed.as_bytes(),
            live.time_remaining,
        );
        fixture.assert_payload(destination.path())?;

        // Stop peers before rechecking: completion must depend on disk hashes, not a swarm.
        stop(&seeder).await?;
        client.delete(handle.id().into(), false).await?;
        let rechecked = added(
            client
                .add_torrent(
                    AddTorrent::from_bytes(resolved.torrent_bytes.clone()),
                    Some(AddTorrentOptions {
                        sub_folder: Some(String::new()),
                        overwrite: true,
                        paused: true,
                        ..Default::default()
                    }),
                )
                .await?,
        )?;
        let checked = wait_for(&rechecked, |stats| {
            matches!(stats.state, TorrentStatsState::Paused)
        })
        .await?;
        assert!(checked.finished, "complete payload did not recheck");
        assert_eq!(checked.progress_bytes, checked.total_bytes);

        // A same-size corrupt file must not be mistaken for a complete download.
        client.delete(rechecked.id().into(), false).await?;
        let path = destination.path().join(&fixture.files[0].0);
        let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&[255])?;
        drop(file);
        let corrupted = added(
            client
                .add_torrent(
                    AddTorrent::from_bytes(resolved.torrent_bytes),
                    Some(AddTorrentOptions {
                        sub_folder: Some(String::new()),
                        overwrite: true,
                        paused: true,
                        ..Default::default()
                    }),
                )
                .await?,
        )?;
        let checked = wait_for(&corrupted, |stats| {
            matches!(stats.state, TorrentStatsState::Paused)
        })
        .await?;
        assert!(!checked.finished, "corruption went undetected");
        assert!(checked.progress_bytes < checked.total_bytes);
        Ok(())
    }
    .await;
    let client_shutdown = stop(&client).await;
    let seed_shutdown = stop(&seeder).await;
    result.and(client_shutdown).and(seed_shutdown)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn payload_write_failure_is_visible_as_fatal_engine_state() -> Result<()> {
    let fixture = Fixture::new(true).await?;
    let (seeder, peer) = seed(&fixture).await?;
    let destination = camino_tempfile::tempdir()?;
    let client = session(destination.path(), false).await?;
    let faults = support::fault_storage::FailWrites::default();
    let result = async {
        let handle = added(
            client
                .add_torrent(
                    AddTorrent::from_bytes(fixture.torrent.as_bytes()?),
                    Some(AddTorrentOptions {
                        sub_folder: Some(String::new()),
                        initial_peers: Some(vec![peer]),
                        storage_factory: Some(faults.clone().boxed()),
                        ..Default::default()
                    }),
                )
                .await?,
        )?;
        let failed = wait_for(&handle, |stats| {
            matches!(stats.state, TorrentStatsState::Error)
        })
        .await?;
        ensure!(
            faults.0.load(Ordering::Relaxed) > 0,
            "no payload write was attempted"
        );
        assert!(!failed.finished);
        assert!(
            failed
                .error
                .context("missing fatal error")?
                .contains("injected payload write failure")
        );
        Ok(())
    }
    .await;
    let client_shutdown = stop(&client).await;
    let seed_shutdown = stop(&seeder).await;
    result.and(client_shutdown).and(seed_shutdown)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metadata_preflight_exposes_the_destination_policy_boundary() -> Result<()> {
    let destination = camino_tempfile::tempdir()?;
    let sentinel = destination.path().join("outside-sentinel");
    std::fs::write(&sentinel, b"unchanged")?;
    let payload = destination.path().join("payload");
    std::fs::create_dir(&payload)?;
    let client = session(&payload, false).await?;
    let result = async {
        // The engine already rejects traversal, separators, and duplicate real paths.
        for paths in [
            vec![vec![b"..".as_slice(), b"outside-sentinel"]],
            vec![vec![b"/absolute".as_slice()]],
            vec![vec![b"nested\\escape".as_slice()]],
            vec![vec![b"same".as_slice()], vec![b"same".as_slice()]],
        ] {
            let borrowed = paths.iter().map(Vec::as_slice).collect::<Vec<_>>();
            let response = client
                .add_torrent(
                    AddTorrent::from_bytes(metainfo(&borrowed)),
                    Some(AddTorrentOptions {
                        list_only: true,
                        sub_folder: Some(String::new()),
                        ..Default::default()
                    }),
                )
                .await;
            assert!(response.is_err(), "engine accepted unsafe fixture");
        }
        // These valid-to-the-engine layouts require rqget policy before a real add.
        for paths in [
            vec![vec![b".rqget".as_slice(), b"state.json"]],
            vec![vec![b"file".as_slice()], vec![b"file".as_slice(), b"child"]],
            vec![vec![b"\xff".as_slice()]],
        ] {
            let borrowed = paths.iter().map(Vec::as_slice).collect::<Vec<_>>();
            let response = client
                .add_torrent(
                    AddTorrent::from_bytes(metainfo(&borrowed)),
                    Some(AddTorrentOptions {
                        list_only: true,
                        sub_folder: Some(String::new()),
                        ..Default::default()
                    }),
                )
                .await?;
            assert!(matches!(response, AddTorrentResponse::ListOnly(_)));
        }
        assert_eq!(std::fs::read_dir(&payload)?.count(), 0);
        assert_eq!(std::fs::read(&sentinel)?, b"unchanged");
        Ok(())
    }
    .await;
    result.and(stop(&client).await)
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn overwrite_mode_does_not_reject_a_symlink() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let outside = root.path().join("outside");
    std::fs::write(&outside, b"x")?;
    let destination = root.path().join("payload");
    std::fs::create_dir(&destination)?;
    std::os::unix::fs::symlink(&outside, destination.join("link"))?;
    let client = session(&destination, false).await?;
    let result = async {
        let torrent = metainfo(&[&[b"link"]]);
        let new_transfer = client
            .add_torrent(
                AddTorrent::from_bytes(torrent.clone()),
                Some(AddTorrentOptions {
                    sub_folder: Some(String::new()),
                    paused: true,
                    ..Default::default()
                }),
            )
            .await;
        assert!(
            new_transfer.is_err(),
            "exclusive creation should reject an existing link"
        );
        // Paused, same-sized, no peers: prove the opener follows the link without modifying it.
        let handle = added(
            client
                .add_torrent(
                    AddTorrent::from_bytes(torrent),
                    Some(AddTorrentOptions {
                        sub_folder: Some(String::new()),
                        overwrite: true,
                        paused: true,
                        ..Default::default()
                    }),
                )
                .await?,
        )?;
        wait_for(&handle, |stats| {
            matches!(stats.state, TorrentStatsState::Paused)
        })
        .await?;
        assert_eq!(std::fs::read(&outside)?, b"x", "outside sentinel changed");
        Ok(())
    }
    .await;
    result.and(stop(&client).await)
}
