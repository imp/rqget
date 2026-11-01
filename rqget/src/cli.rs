//! Parse UTF-8 command arguments before filesystem or network activity.

use anyhow::Result;
use camino::Utf8PathBuf;
use clap::Parser;
use rqget_core::{Request, Source};

/// Download one torrent into a dedicated directory, then exit.
#[derive(Debug, Parser)]
#[command(version, about)]
pub(crate) struct Args {
    /// A local torrent file or a quoted v1 magnet URI.
    #[arg(value_name = "SOURCE")]
    source: String,
    /// Directory dedicated to this torrent.
    #[arg(short, long, value_name = "DIRECTORY")]
    output: Utf8PathBuf,
    /// Recheck and repair an existing matching rqget download.
    #[arg(long)]
    resume: bool,
    /// Suppress progress and success messages; retain errors.
    #[arg(short, long)]
    pub quiet: bool,
}

impl Args {
    pub fn validate(self) -> Result<Request> {
        Ok(Request {
            source: Source::parse(self.source)?,
            output: self.output,
            resume: self.resume,
        })
    }
}
