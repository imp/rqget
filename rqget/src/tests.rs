//! Exercise the application signal boundary in child processes with local engine options.

use super::*;
use librqbit::{limits::LimitsConfig, storage::StorageFactoryExt};
use rqget_test::{Fixture, fault_storage::ReadGate, seed, stop, tracker::Tracker};
use std::{
    num::NonZeroU32,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::time::{sleep, timeout};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signal_child() -> Result<()> {
    let Ok(source) = std::env::var("RQGET_CHILD_SOURCE") else {
        return Ok(());
    };
    let output = std::env::var("RQGET_CHILD_OUTPUT")?;
    let mode = std::env::var("RQGET_CHILD_MODE")?;
    let mut arguments = vec!["rqget", source.as_str(), "-o", output.as_str()];
    if mode == "quiet-error" {
        arguments.push("--quiet");
    }
    let args = cli::Args::try_parse_from(arguments)?;
    let mut options = SessionOptions {
        dht: None,
        disable_local_service_discovery: true,
        disable_trackers: true,
        ..Default::default()
    };
    if let Ok(tracker) = std::env::var("RQGET_CHILD_TRACKER") {
        options.disable_trackers = false;
        options.trackers.insert(tracker.parse()?);
    }
    if mode == "download" {
        options.ratelimits = LimitsConfig {
            download_bps: NonZeroU32::new(32 * 1024),
            ..Default::default()
        };
    }
    if mode == "checking" {
        options.default_storage_factory = Some(
            ReadGate::new(
                std::env::var("RQGET_CHILD_ENTERED")?.into(),
                std::env::var("RQGET_CHILD_RELEASE")?.into(),
            )
            .boxed(),
        );
    }
    let status = report(execute(args, options).await);
    std::process::exit(i32::from(status));
}

struct Process {
    child: Child,
    stderr: camino::Utf8PathBuf,
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Process {
    fn start(
        source: &str,
        output: &camino::Utf8Path,
        mode: &str,
        directory: &camino::Utf8Path,
        tracker: &str,
    ) -> Result<Self> {
        let stderr = directory.join("stderr");
        let child = Command::new(std::env::current_exe()?)
            .args(["--exact", "tests::signal_child", "--nocapture"])
            .env_remove("RUST_LOG")
            .env("RQGET_CHILD_SOURCE", source)
            .env("RQGET_CHILD_OUTPUT", output)
            .env("RQGET_CHILD_MODE", mode)
            .env("RQGET_CHILD_TRACKER", tracker)
            .env("RQGET_CHILD_ENTERED", directory.join("entered"))
            .env("RQGET_CHILD_RELEASE", directory.join("release"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&stderr)?)
            .spawn()?;
        Ok(Self { child, stderr })
    }

    async fn wait_for(&mut self, text: &str) -> Result<()> {
        timeout(Duration::from_secs(10), async {
            loop {
                let output = std::fs::read_to_string(&self.stderr)?;
                if output.contains(text) {
                    return Ok(());
                }
                anyhow::ensure!(
                    self.child.try_wait()?.is_none(),
                    "child exited before {text}: {output}"
                );
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .context("waiting for child phase timed out")?
    }

    async fn wait(&mut self) -> Result<std::process::ExitStatus> {
        timeout(Duration::from_secs(5), async {
            loop {
                if let Some(status) = self.child.try_wait()? {
                    return Ok(status);
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .context("child shutdown exceeded five seconds")?
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_metainfo_error_survives_quiet_mode() -> Result<()> {
    let root = camino_tempfile::tempdir()?;
    let input = root.path().join("bad.torrent");
    std::fs::write(&input, b"this is not bencoded metainfo")?;
    let output = root.path().join("out");
    let mut process = Process::start(
        input.as_str(),
        &output,
        "quiet-error",
        root.path(),
        "http://127.0.0.1:9/unused",
    )?;
    assert_eq!(process.wait().await?.code(), Some(1));
    let stderr = std::fs::read_to_string(&process.stderr)?;
    assert!(stderr.contains("rqget:"));
    assert!(stderr.contains("metadata"));
    assert!(!stderr.contains("Destination:"));
    assert!(!stderr.contains("Resolving metadata"));
    assert!(!stderr.contains('\u{1b}'));
    assert!(!output.join(".rqget/state.json").exists());
    assert_eq!(std::fs::read_dir(output)?.count(), 1);
    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signals_reach_metadata_checking_and_transfer_processes() -> Result<()> {
    let fixture = Fixture::new(false).await?;
    let (seeder, peer) = seed(&fixture).await?;
    let tracker = Tracker::start(peer)?;
    let result = async {
        for (mode, signal, expected, phase) in [
            ("metadata", "-INT", 130, "Resolving metadata"),
            ("checking", "-INT", 130, "Checking existing files"),
            ("download", "-INT", 130, "Downloading"),
            ("download", "-TERM", 143, "Downloading"),
        ] {
            let root = camino_tempfile::tempdir()?;
            let input = root.path().join("input.torrent");
            std::fs::write(&input, fixture.torrent.as_bytes()?)?;
            let source = if mode == "metadata" {
                format!("magnet:?xt=urn:btih:{:040x}", 1)
            } else {
                input.to_string()
            };
            let output = root.path().join("out");
            let mut process =
                Process::start(&source, &output, mode, root.path(), tracker.url.as_str())?;
            process.wait_for(phase).await?;
            assert!(
                Command::new("/bin/kill")
                    .arg(signal)
                    .arg(process.child.id().to_string())
                    .status()?
                    .success()
            );
            process.wait_for("Stopping transfer").await?;
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(output.join(".rqget/lock"))?;
            // The core observer test checks lock ownership synchronously at Stopping;
            // this process check proves release after the child has actually exited.
            // Let a deterministic checking operation finish while shutdown still owns the lock.
            std::fs::write(root.path().join("release"), b"release")?;
            assert_eq!(process.wait().await?.code(), Some(expected));
            lock.try_lock()?;
            assert_eq!(
                output.join(".rqget/state.json").exists(),
                mode != "metadata"
            );
        }
        Ok(())
    }
    .await;
    result.and(stop(&seeder).await)
}
