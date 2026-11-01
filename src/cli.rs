//! Parse the command and validate sources before filesystem or network activity.

use anyhow::{Context, Result, bail, ensure};
use camino::Utf8PathBuf;
use clap::Parser;
use librqbit::Magnet;

/// Download one torrent into a dedicated directory, then exit.
#[derive(Debug, Parser)]
#[command(version, about)]
pub(crate) struct Args {
    /// A local torrent file or a quoted BitTorrent v1 magnet URI.
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
    quiet: bool,
}

#[derive(Debug)]
pub(crate) struct Request {
    pub source: Source,
    pub output: Utf8PathBuf,
    pub resume: bool,
    pub quiet: bool,
}

pub(crate) enum Source {
    File(Utf8PathBuf),
    Magnet(String),
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(path) => f.debug_tuple("File").field(path).finish(),
            Self::Magnet(_) => f.write_str("Magnet(<redacted>)"),
        }
    }
}

impl Args {
    pub fn validate(self) -> Result<Request> {
        let input = self.source;
        ensure!(
            input != "-",
            "stdin input is not supported; supply a local torrent file"
        );
        let source = if input.starts_with("magnet:") {
            let uri = url::Url::parse(&input).context("invalid magnet URI")?;
            // Reject before Magnet::parse, which eagerly expands selection ranges.
            ensure!(
                !uri.query_pairs().any(|(key, _)| key == "so"),
                "magnet file selection is not supported"
            );
            let magnet =
                Magnet::parse(&input).map_err(|_| anyhow::anyhow!("invalid magnet URI"))?;
            ensure!(
                magnet.as_id20().is_some(),
                "magnet must contain a BitTorrent v1 infohash"
            );
            Source::Magnet(input)
        } else {
            if url::Url::parse(&input).is_ok() {
                bail!("unsupported source URI; supply a magnet URI or local torrent file");
            }
            let path = Utf8PathBuf::from(input);
            ensure!(
                path.is_file(),
                "local torrent file does not exist or is not a regular file: {path}"
            );
            Source::File(path)
        };
        Ok(Request {
            source,
            output: self.output,
            resume: self.resume,
            quiet: self.quiet,
        })
    }
}
