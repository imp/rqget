//! Render observations without treating checked or resumed bytes as new traffic.

use anyhow::Result;
use indicatif::{HumanBytes, HumanDuration, ProgressBar, ProgressDrawTarget, ProgressStyle};
use parking_lot::Mutex;
use rqget_core::{Event, Snapshot};
use std::{
    io::{self, IsTerminal, Write},
    sync::Arc,
    time::Duration,
};
use tokio::time::Instant;

const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(30);

#[cfg(test)]
mod tests;

type Output = Arc<Mutex<Box<dyn Write + Send>>>;

pub(crate) struct Progress {
    bar: ProgressBar,
    terminal: bool,
    quiet: bool,
    started: Instant,
    last_output: Instant,
    last_advance: Instant,
    last_bytes: u64,
    phase: &'static str,
    transfer_style: ProgressStyle,
    total_bytes: u64,
    output: Output,
}

impl Progress {
    pub fn new(quiet: bool) -> Result<Self> {
        let terminal = io::stderr().is_terminal();
        Self::with_output(
            quiet,
            terminal,
            ProgressDrawTarget::stderr(),
            Box::new(io::stderr()),
        )
    }

    fn with_output(
        quiet: bool,
        terminal: bool,
        target: ProgressDrawTarget,
        output: Box<dyn Write + Send>,
    ) -> Result<Self> {
        let bar = if terminal && !quiet {
            ProgressBar::new_spinner()
        } else {
            ProgressBar::hidden()
        };
        if terminal && !quiet {
            bar.set_draw_target(target);
        }
        bar.set_style(ProgressStyle::with_template("{spinner} {msg}")?);
        bar.enable_steady_tick(Duration::from_millis(120));
        let now = Instant::now();
        Ok(Self {
            bar,
            terminal,
            quiet,
            started: now,
            last_output: now,
            last_advance: now,
            last_bytes: 0,
            phase: "",
            transfer_style: ProgressStyle::with_template(
                "{spinner} {wide_bar} {percent:>3}%\n{msg}",
            )?,
            total_bytes: 0,
            output: Arc::new(Mutex::new(output)),
        })
    }

    pub fn writer(&self) -> LogWriter {
        LogWriter {
            bar: self.bar.clone(),
            output: Arc::clone(&self.output),
        }
    }

    pub fn destination(&self, path: &camino::Utf8Path) {
        if !self.quiet {
            let _ = writeln!(
                self.writer(),
                "Destination: {}",
                path.as_str().escape_debug()
            );
        }
    }

    pub fn event(&mut self, event: Event) {
        match event {
            Event::Metadata => self.emit(
                "Resolving metadata",
                format!(
                    "Resolving metadata... {} elapsed",
                    HumanDuration(self.started.elapsed())
                ),
            ),
            Event::Transfer(stats) => self.update(&stats),
            Event::Stopping => self.emit("Stopping", "Stopping transfer...".to_owned()),
        }
    }

    fn update(&mut self, stats: &Snapshot) {
        self.total_bytes = stats.total_bytes;
        if stats.checking || stats.completed_bytes != self.last_bytes {
            self.last_advance = Instant::now();
            self.last_bytes = stats.completed_bytes;
        }
        let totals = format!(
            "{} / {}",
            HumanBytes(stats.completed_bytes),
            HumanBytes(stats.total_bytes)
        );
        let (phase, message) = if stats.checking {
            (
                "Checking",
                format!("Checking existing files  {totals} examined"),
            )
        } else {
            let peers = stats.connected_peers;
            let phase = if peers == 0 && !stats.finished {
                "Waiting for peers"
            } else {
                "Downloading"
            };
            let eta = stats.remaining.as_ref().map_or_else(
                || {
                    format!(
                        "ETA unknown; {} elapsed",
                        HumanDuration(self.started.elapsed())
                    )
                },
                |eta| format!("~{eta} left"),
            );
            let inactive = self.last_advance.elapsed();
            let waiting = if !stats.finished && inactive >= Duration::from_secs(10) {
                format!("  no new verified data for {}", HumanDuration(inactive))
            } else {
                String::new()
            };
            (
                phase,
                format!(
                    "{phase}  {totals}  down {}/s  up {}/s  {eta}  {peers} peers{waiting}",
                    HumanBytes(stats.download_bytes_per_second),
                    HumanBytes(stats.upload_bytes_per_second)
                ),
            )
        };
        if stats.total_bytes > 0 {
            self.bar.set_style(self.transfer_style.clone());
            self.bar.set_length(stats.total_bytes);
            self.bar
                .set_position(stats.completed_bytes.min(stats.total_bytes));
        }
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
            let _ = writeln!(self.writer(), "{message}");
            self.last_output = Instant::now();
        }
    }

    pub fn finish(&self, outcome: &Result<u8>, destination: &camino::Utf8Path) {
        self.bar.finish_and_clear();
        if self.quiet {
            return;
        }
        match outcome {
            Ok(0) => {
                let _ = writeln!(
                    self.writer(),
                    "Done — {} verified in {} → {}",
                    HumanBytes(self.total_bytes),
                    HumanDuration(self.started.elapsed()),
                    destination.as_str().escape_debug()
                );
            }
            Ok(_) if destination.join(".rqget/state.json").is_file() => {
                let _ = writeln!(
                    self.writer(),
                    "Interrupted — download data retained; resume with --resume"
                );
            }
            Ok(_) => {
                let _ = writeln!(
                    self.writer(),
                    "Interrupted before payload transfer; retry the same command"
                );
            }
            Err(_) => {}
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}

/// Route diagnostics around the progress display, including when progress is hidden.
#[derive(Clone)]
pub(crate) struct LogWriter {
    bar: ProgressBar,
    output: Output,
}

impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bar.suspend(|| self.output.lock().write_all(bytes))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.output.lock().flush()
    }
}
