use super::*;
use indicatif::InMemoryTerm;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().clone()).expect("formatter emits UTF-8")
    }
}

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn plain(quiet: bool) -> Result<(Progress, Capture)> {
    let capture = Capture::default();
    let progress = Progress::with_output(
        quiet,
        false,
        ProgressDrawTarget::hidden(),
        Box::new(capture.clone()),
    )?;
    progress.bar.disable_steady_tick();
    Ok((progress, capture))
}

fn snapshot(checking: bool, completed: u64, total: u64) -> Snapshot {
    Snapshot {
        checking,
        completed_bytes: completed,
        total_bytes: total,
        finished: false,
        download_bytes_per_second: 0,
        upload_bytes_per_second: 0,
        connected_peers: 2,
        remaining: None,
    }
}

#[tokio::test(start_paused = true)]
async fn redirected_heartbeat_is_periodic_and_does_not_replay_missed_ticks() -> Result<()> {
    let (mut progress, capture) = plain(false)?;
    progress.event(Event::Metadata);
    for _ in 0..29 {
        tokio::time::advance(Duration::from_secs(1)).await;
        progress.event(Event::Metadata);
    }
    assert_eq!(capture.text().lines().count(), 1);
    tokio::time::advance(Duration::from_secs(1)).await;
    progress.event(Event::Metadata);
    assert_eq!(capture.text().lines().count(), 2);
    tokio::time::advance(Duration::from_secs(300)).await;
    progress.event(Event::Metadata);
    progress.event(Event::Metadata);
    assert_eq!(capture.text().lines().count(), 3);
    assert!(!capture.text().contains('\u{1b}'));
    assert!(capture.text().contains("elapsed"));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn checking_and_retained_bytes_do_not_inflate_transfer_speed() -> Result<()> {
    let (mut progress, capture) = plain(false)?;
    progress.event(Event::Transfer(snapshot(true, 900, 1000)));
    tokio::time::advance(Duration::from_secs(1)).await;
    progress.event(Event::Transfer(snapshot(false, 900, 1000)));
    let text = capture.text();
    let checking = text.lines().next().expect("checking phase line");
    assert!(checking.contains("Checking existing files"));
    assert!(checking.contains("examined"));
    assert!(!checking.contains("down "));
    let downloading = text.lines().nth(1).expect("transfer phase line");
    assert!(downloading.contains("Downloading"));
    assert!(
        downloading
            .split("down ")
            .nth(1)
            .expect("rate")
            .starts_with('0')
    );
    assert!(downloading.contains("ETA unknown"));
    assert!(downloading.contains("elapsed"));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn waiting_unknown_totals_and_no_progress_are_honest() -> Result<()> {
    let (mut progress, capture) = plain(false)?;
    let mut stats = snapshot(false, 0, 0);
    stats.connected_peers = 0;
    progress.event(Event::Transfer(stats));
    tokio::time::advance(SNAPSHOT_INTERVAL).await;
    let mut stats = snapshot(false, 0, 0);
    stats.connected_peers = 0;
    progress.event(Event::Transfer(stats));
    let text = capture.text();
    assert!(text.contains("Waiting for peers"));
    assert!(text.contains("no new verified data"));
    assert!(!text.contains("NaN"));
    assert!(!text.contains('%'));
    assert!(!text.contains("Done"));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn quiet_hides_status_but_retains_diagnostics() -> Result<()> {
    let (mut progress, capture) = plain(true)?;
    progress.destination(camino::Utf8Path::new("out"));
    progress.event(Event::Metadata);
    progress.event(Event::Transfer(snapshot(false, 50, 100)));
    progress.finish(&Ok(0), camino::Utf8Path::new("out"));
    assert_eq!(capture.text(), "");
    writeln!(progress.writer(), "storage denied")?;
    assert_eq!(capture.text(), "storage denied\n");
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn interruption_before_metadata_does_not_suggest_an_unavailable_resume() -> Result<()> {
    let (progress, capture) = plain(false)?;
    let root = camino_tempfile::tempdir()?;
    progress.finish(&Ok(130), root.path());
    assert!(capture.text().contains("retry the same command"));
    assert!(!capture.text().contains("--resume"));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn terminal_bar_renders_percentage_and_engine_estimates() -> Result<()> {
    let terminal = InMemoryTerm::new(6, 160);
    let capture = Capture::default();
    let mut progress = Progress::with_output(
        false,
        true,
        ProgressDrawTarget::term_like(Box::new(terminal.clone())),
        Box::new(capture.clone()),
    )?;
    progress.bar.disable_steady_tick();
    let mut stats = snapshot(false, 50, 100);
    stats.remaining = Some("3m 26s".to_owned());
    stats.download_bytes_per_second = 2048;
    progress.event(Event::Transfer(stats));
    progress.bar.force_draw();
    let screen = terminal.contents();
    assert!(screen.contains("50%"), "{screen}");
    assert!(screen.contains("Downloading"), "{screen}");
    assert!(screen.contains("~3m 26s left"), "{screen}");
    assert!(screen.contains("2 peers"), "{screen}");
    writeln!(progress.writer(), "engine diagnostic")?;
    progress.bar.force_draw();
    assert_eq!(capture.text(), "engine diagnostic\n");
    assert!(terminal.contents().contains("Downloading"));
    progress.finish(&Ok(0), camino::Utf8Path::new("out"));
    assert_eq!(terminal.contents().trim(), "");
    assert!(capture.text().contains("Done"));
    Ok(())
}
