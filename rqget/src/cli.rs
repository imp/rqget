//! Parse UTF-8 command arguments before filesystem or network activity.

use std::net::SocketAddr;

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
    /// Initial seeding peer (IP:PORT or [IPv6]:PORT); repeatable.
    #[arg(long = "peer", value_name = "IP:PORT")]
    peers: Vec<SocketAddr>,
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
            peers: self.peers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_peers_are_optional_and_preserve_repeated_ipv4_and_ipv6_addresses() -> Result<()> {
        let source = format!("magnet:?xt=urn:btih:{:040x}", 1);
        for (extra, expected) in [
            (vec![], vec![]),
            (
                vec![
                    "--peer",
                    "192.168.1.20:4240",
                    "--peer",
                    "[2001:db8::20]:4241",
                ],
                vec!["192.168.1.20:4240".parse()?, "[2001:db8::20]:4241".parse()?],
            ),
        ] {
            let args = Args::try_parse_from(
                ["rqget", source.as_str(), "-o", "out"]
                    .into_iter()
                    .chain(extra),
            )?;
            assert_eq!(args.validate()?.peers, expected);
        }
        Ok(())
    }
}
