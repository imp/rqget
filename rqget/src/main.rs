//! Download a single torrent with explicit ownership of its destination.

mod cli;
mod progress;

#[cfg(test)]
mod tests;

use std::{io::Write, process::ExitCode};

use anyhow::{Context, Result};
use clap::Parser;
use librqbit::{DhtSessionConfig, ListenerOptions, SessionOptions};

#[tokio::main]
async fn main() -> ExitCode {
    let args = match cli::Args::try_parse() {
        Ok(args) => args,
        Err(error) => {
            let code = error.exit_code();
            let _ = error.print();
            return ExitCode::from(u8::try_from(code).unwrap_or(1));
        }
    };
    let result = execute(args, session_options()).await;
    ExitCode::from(report(result))
}

fn report(result: Result<u8>) -> u8 {
    match result {
        Ok(status) => status,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "rqget: {error:#}");
            1
        }
    }
}

async fn execute(args: cli::Args, options: SessionOptions) -> Result<u8> {
    let quiet = args.quiet;
    let request = args.validate()?;
    let destination = request.output.clone();
    let signals = Signals::new()?;
    let mut progress = progress::Progress::new(quiet)?;
    progress.destination(&destination);
    let writer = progress.writer();
    // Detailed engine logging is opt-in; tracker diagnostics may contain private URLs.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("off"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .try_init()
        .map_err(|error| anyhow::anyhow!("cannot initialize engine diagnostics: {error}"))?;
    let result = rqget_core::run(request, options, signals.receive(), |event| {
        progress.event(event);
    })
    .await;
    progress.finish(&result, &destination);
    result
}

fn session_options() -> SessionOptions {
    SessionOptions {
        dht: Some(DhtSessionConfig {
            persistence: None,
            ..Default::default()
        }),
        listen: Some(ListenerOptions::default()),
        ..Default::default()
    }
}

#[cfg(unix)]
struct Signals {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Signals {
    fn new() -> Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
        })
    }

    async fn receive(mut self) -> Result<u8> {
        tokio::select! {
            value = self.interrupt.recv() => { value.context("interrupt handler closed")?; Ok(130) },
            value = self.terminate.recv() => { value.context("termination handler closed")?; Ok(143) },
        }
    }
}

#[cfg(not(unix))]
struct Signals;

#[cfg(not(unix))]
impl Signals {
    fn new() -> Result<Self> {
        Ok(Self)
    }
    async fn receive(self) -> Result<u8> {
        tokio::signal::ctrl_c().await?;
        Ok(130)
    }
}
