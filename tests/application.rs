// The binary owns these modules; compile its concrete runner here without exposing a library API.
#[path = "../src/cli.rs"]
mod cli;
#[path = "../src/destination.rs"]
mod destination;
#[path = "../src/download.rs"]
mod download;
#[allow(
    dead_code,
    reason = "main wires the log writer; these tests exercise the transfer runner"
)]
#[path = "../src/progress.rs"]
mod progress;
#[allow(
    dead_code,
    reason = "shared dependency fixtures also serve the compatibility-only suite"
)]
mod support;
#[path = "support/tracker.rs"]
mod tracker;

use std::{future::pending, process::Command, time::Duration};

use anyhow::Result;
use camino::Utf8Path;
use clap::Parser;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ListOnlyResponse, SessionOptions,
};
use tokio::time::timeout;

fn request(source: &str, output: &Utf8Path, resume: bool) -> Result<cli::Request> {
    let mut args = vec!["rqget", source, "-o", output.as_str(), "--quiet"];
    if resume {
        args.push("--resume");
    }
    cli::Args::try_parse_from(args)?.validate()
}

fn local_options() -> SessionOptions {
    SessionOptions {
        dht: None,
        disable_trackers: true,
        disable_local_service_discovery: true,
        ..Default::default()
    }
}

async fn metadata(bytes: Vec<u8>, root: &Utf8Path) -> Result<ListOnlyResponse> {
    let client = support::session(root, false).await?;
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
    support::stop(&client).await?;
    match response? {
        AddTorrentResponse::ListOnly(metadata) => Ok(metadata),
        _ => anyhow::bail!("expected metadata"),
    }
}

#[test]
fn cli_rejects_invalid_invocations_without_destination_side_effects() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let destination = root.path().join("must-not-exist");
    for arguments in [
        vec!["--help"],
        vec!["--version"],
        vec![],
        vec!["source"],
        vec!["a", "b", "-o", destination.as_str()],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rqget"))
            .args(&arguments)
            .output()?;
        let informational = arguments == ["--help"] || arguments == ["--version"];
        assert_eq!(
            output.status.code(),
            Some(if informational { 0 } else { 2 })
        );
        assert!(!destination.exists());
    }
    for source in [
        "-",
        "0123456789abcdef0123456789abcdef0123456789",
        "https://private.example/token",
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef0123456789&%73%6f=0-18446744073709551615&tr=https://private.example/token",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rqget"))
            .args(["--", source])
            .output()?;
        assert!(!output.status.success());
        // Validate with otherwise valid arguments too; rejected sources never reach I/O.
        let output = Command::new(env!("CARGO_BIN_EXE_rqget"))
            .args(["-o", destination.as_str(), "--", source])
            .output()?;
        assert_eq!(output.status.code(), Some(1));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private.example"));
        assert!(!destination.exists());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn clap_rejects_non_utf8_sources() -> Result<()> {
    use std::os::unix::ffi::OsStringExt;
    let root = camino_tempfile::tempdir()?;
    let output = Command::new(env!("CARGO_BIN_EXE_rqget"))
        .arg(std::ffi::OsString::from_vec(vec![255]))
        .args(["-o", root.path().join("out").as_str()])
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(std::fs::read_dir(root.path())?.count(), 0);
    Ok(())
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
        let mut progress = progress::Progress::new(initial.quiet)?;
        assert_eq!(
            timeout(
                Duration::from_secs(30),
                download::run(
                    request(input.as_str(), &destination, false)?,
                    options,
                    pending(),
                    &mut progress
                )
            )
            .await??,
            0
        );
        fixture.assert_payload(&destination)?;
        assert!(destination.join(".rqget/state.json").is_file());
        assert!(
            download::run(initial, local_options(), pending(), &mut progress)
                .await
                .is_err()
        );
        // A complete matching transfer rechecks successfully with no peers or trackers.
        assert_eq!(
            timeout(
                Duration::from_secs(5),
                download::run(
                    request(input.as_str(), &destination, true)?,
                    local_options(),
                    pending(),
                    &mut progress
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
                download::run(
                    request(input.as_str(), &destination, true)?,
                    options,
                    pending(),
                    &mut progress
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn destination_preflight_rejects_unsafe_metadata_without_payload_writes() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let outside = root.path().join("outside");
    std::fs::write(&outside, b"unchanged")?;
    for (index, paths) in [
        vec![vec![b".rqget".as_slice(), b"state.json"]],
        vec![vec![b"file".as_slice()], vec![b"file".as_slice(), b"child"]],
        vec![vec![b"\xff".as_slice()]],
        vec![vec![b".".as_slice()]],
    ]
    .into_iter()
    .enumerate()
    {
        let destination = root.path().join(format!("out-{index}"));
        let guard = destination::Destination::open(&destination, false)?;
        let paths = paths.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let resolved = metadata(support::metainfo(&paths), &destination).await?;
        assert!(guard.prepare(&resolved).is_err());
        assert!(!destination.join(".rqget/state.json").exists());
        assert_eq!(std::fs::read_dir(&destination)?.count(), 1);
    }
    assert_eq!(std::fs::read(&outside)?, b"unchanged");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn destination_lock_records_and_resume_are_explicit() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let destination = root.path().join("out");
    let guard = destination::Destination::open(&destination, false)?;
    assert!(destination::Destination::open(&destination, false).is_err());
    let resolved = metadata(support::metainfo(&[&[b"payload"]]), &destination).await?;
    assert!(!guard.prepare(&resolved)?);
    drop(guard);
    assert!(destination::Destination::open(&destination, false).is_err());
    let guard = destination::Destination::open(&destination, true)?;
    assert!(guard.prepare(&resolved)?);
    std::fs::write(destination.join("unrelated"), b"keep me")?;
    assert!(guard.prepare(&resolved).is_err());
    assert_eq!(std::fs::read(destination.join("unrelated"))?, b"keep me");
    drop(guard);
    std::fs::write(destination.join(".rqget/state.json"), b"{incomplete")?;
    assert!(destination::Destination::open(&destination, true).is_err());
    assert_eq!(std::fs::read(destination.join("unrelated"))?, b"keep me");
    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_rejects_symlinks_and_aliases_share_the_lock() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let destination = root.path().join("out");
    let guard = destination::Destination::open(&destination, false)?;
    let resolved = metadata(support::metainfo(&[&[b"payload"]]), &destination).await?;
    guard.prepare(&resolved)?;
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&destination, &alias)?;
    assert!(destination::Destination::open(&alias, true).is_err());
    drop(guard);
    let outside = root.path().join("outside");
    std::fs::write(&outside, b"x")?;
    std::os::unix::fs::symlink(&outside, destination.join("payload"))?;
    let guard = destination::Destination::open(&alias, true)?;
    assert!(guard.prepare(&resolved).is_err());
    assert_eq!(std::fs::read(&outside)?, b"x");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_filename_aliases_are_rejected_before_payload_creation() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    std::fs::write(root.path().join("CaseProbe"), b"")?;
    let case_insensitive = root.path().join("caseprobe").exists();
    let destination = root.path().join("out");
    let guard = destination::Destination::open(&destination, false)?;
    let resolved = metadata(support::metainfo(&[&[b"A"], &[b"a"]]), &destination).await?;
    assert_eq!(guard.prepare(&resolved).is_err(), case_insensitive);
    assert!(!destination.join("A").exists());
    assert!(!destination.join("a").exists());
    Ok(())
}
