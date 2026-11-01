//! Render engine progress without treating checked or resumed bytes as new traffic.

use std::{
    io::{self, IsTerminal, Write},
    time::Duration,
};

use anyhow::Result;
use indicatif::{HumanBytes, HumanDuration, ProgressBar, ProgressStyle};
use librqbit::{TorrentStats, TorrentStatsState};
use tokio::time::Instant;

const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(30);

pub(crate) struct Progress {
    bar: ProgressBar,
    terminal: bool,
    quiet: bool,
    started: Instant,
    last_output: Instant,
    last_progress: Instant,
    last_bytes: u64,
    phase: &'static str,
    transfer_style: ProgressStyle,
    total_bytes: u64,
}

impl Progress {
    pub fn new(quiet: bool) -> Result<Self> {
        let terminal = io::stderr().is_terminal();
        let bar = if terminal && !quiet {
            ProgressBar::new_spinner()
        } else {
            ProgressBar::hidden()
        };
        bar.set_style(ProgressStyle::with_template("{spinner} {msg}")?);
        bar.enable_steady_tick(Duration::from_millis(120));
        let now = Instant::now();
        Ok(Self {
            bar,
            terminal,
            quiet,
            started: now,
            last_output: now,
            last_progress: now,
            last_bytes: 0,
            phase: "",
            transfer_style: ProgressStyle::with_template(
                "{spinner} {wide_bar} {percent:>3}% {msg}",
            )?,
            total_bytes: 0,
        })
    }

    pub fn writer(&self) -> LogWriter {
        LogWriter(self.bar.clone())
    }

    pub fn metadata(&mut self) {
        self.emit(
            "Resolving metadata",
            format!(
                "Resolving metadata... {} elapsed",
                HumanDuration(self.started.elapsed())
            ),
        );
    }

    pub fn update(&mut self, stats: &TorrentStats) {
        self.total_bytes = stats.total_bytes;
        let now = Instant::now();
        let checking = matches!(stats.state, TorrentStatsState::Initializing { .. });
        if checking || stats.progress_bytes != self.last_bytes {
            self.last_progress = now;
            self.last_bytes = stats.progress_bytes;
        }
        let totals = format!(
            "{} / {}",
            HumanBytes(stats.progress_bytes),
            HumanBytes(stats.total_bytes)
        );
        let (phase, message) = if checking {
            (
                "Checking",
                format!("Checking existing files  {totals} examined"),
            )
        } else if let Some(live) = &stats.live {
            let peers = live.snapshot.peer_stats.live;
            let phase = if peers == 0 && !stats.finished {
                "Waiting for peers"
            } else {
                "Downloading"
            };
            let eta = live
                .time_remaining
                .as_ref()
                .map_or_else(|| "unknown ETA".to_owned(), |eta| format!("~{eta} left"));
            let inactive = self.last_progress.elapsed();
            let waiting = if !stats.finished && inactive >= Duration::from_secs(10) {
                format!("  no new verified data for {}", HumanDuration(inactive))
            } else {
                String::new()
            };
            (
                phase,
                format!(
                    "{phase}  {totals}  down {}/s  up {}/s  {eta}  {peers} peers{waiting}",
                    HumanBytes(live.download_speed.as_bytes()),
                    HumanBytes(live.upload_speed.as_bytes())
                ),
            )
        } else {
            ("Preparing", format!("Preparing transfer  {totals}"))
        };
        if stats.total_bytes > 0 {
            self.bar.set_style(self.transfer_style.clone());
            self.bar.set_length(stats.total_bytes);
            self.bar
                .set_position(stats.progress_bytes.min(stats.total_bytes));
        }
        // Width and percentage are presentation only; the engine owns completion.
        self.emit(phase, message);
    }

    fn emit(&mut self, phase: &'static str, message: String) {
        if self.quiet {
            return;
        }
        let changed = self.phase != phase;
        self.phase = phase;
        if self.terminal {
            self.bar.set_message(message);
        } else if changed || self.last_output.elapsed() >= SNAPSHOT_INTERVAL {
            eprintln!("{message}");
            self.last_output = Instant::now();
        }
    }

    pub fn shutdown(&mut self) {
        self.emit("Stopping", "Stopping transfer...".to_owned());
    }

    pub fn finish(&self, outcome: &Result<u8>, destination: &camino::Utf8Path) {
        self.bar.finish_and_clear();
        if self.quiet {
            return;
        }
        match outcome {
            Ok(0) => eprintln!(
                "Done — {} verified in {} → {}",
                HumanBytes(self.total_bytes),
                HumanDuration(self.started.elapsed()),
                destination.as_str().escape_debug()
            ),
            Ok(_) => eprintln!("Interrupted — partial data retained; resume with --resume"),
            Err(_) => {}
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}

/// Route engine diagnostics around the progress display, including when progress is hidden.
#[derive(Clone)]
pub(crate) struct LogWriter(ProgressBar);

impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.suspend(|| io::stderr().write_all(bytes))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}
