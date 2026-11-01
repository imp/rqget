//! Validate source encodings and URI behavior before acquiring destination ownership.

use std::net::SocketAddr;

use anyhow::{Context, Result, ensure};
use camino::Utf8PathBuf;
use librqbit::Magnet;

/// One foreground transfer into a dedicated destination.
#[derive(Debug)]
pub struct Request {
    pub source: Source,
    pub output: Utf8PathBuf,
    pub resume: bool,
    /// Initial connection candidates for metadata and payload; discovery remains enabled.
    pub peers: Vec<SocketAddr>,
}

/// A validated local metainfo path or v1 magnet URI.
pub struct Source(pub(crate) Kind);

pub(crate) enum Kind {
    File(Utf8PathBuf),
    Magnet(String),
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Kind::File(path) => f.debug_tuple("File").field(path).finish(),
            Kind::Magnet(_) => f.write_str("Magnet(<redacted>)"),
        }
    }
}

impl Source {
    /// Validate one UTF-8 source without network activity or destination writes.
    ///
    /// # Errors
    /// Returns an error for unsupported URIs, selection parameters, missing v1
    /// infohashes, stdin shorthand, or a missing/non-file local path.
    pub fn parse(input: String) -> Result<Self> {
        ensure!(
            input != "-",
            "stdin input is not supported; supply a local torrent file"
        );
        if input.starts_with("magnet:") {
            let uri = url::Url::parse(&input).context("invalid magnet URI")?;
            // Reject before the engine eagerly expands selector ranges.
            ensure!(
                !uri.query_pairs().any(|(key, _)| key == "so"),
                "magnet file selection is not supported"
            );
            let magnet =
                Magnet::parse(&input).map_err(|_error| anyhow::anyhow!("invalid magnet URI"))?;
            ensure!(
                magnet.as_id20().is_some(),
                "magnet must contain a BitTorrent v1 infohash"
            );
            Ok(Self(Kind::Magnet(input)))
        } else {
            ensure!(
                url::Url::parse(&input).is_err(),
                "unsupported source URI; supply a magnet URI or local torrent file"
            );
            let path = Utf8PathBuf::from(input);
            ensure!(
                path.is_file(),
                "local torrent file does not exist or is not a regular file: {path}"
            );
            Ok(Self(Kind::File(path)))
        }
    }
}
