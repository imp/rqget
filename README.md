# rqget

A torrent download CLI being built on librqbit. The dependency compatibility
milestone is complete; the application binary is still the Cargo starter and
does not yet implement downloads. The proposed command and behavior live in
[OpenSpec](openspec/changes/plan-torrent-downloader/README.md).

## Development

Use stable Rust. The declared supported baseline is Rust 1.99; the compatibility
spike was verified with Rust 1.99.1 on native `aarch64-apple-darwin`. This is a
conservative project baseline, not a claim about librqbit's minimum compiler.
`rust-toolchain.toml` selects the installed stable channel. Cargo.lock records
the dependency resolution; use locked checks and review dependency updates.

```sh
cargo +stable fmt --check
cargo +stable clippy --locked --all-targets -- -D warnings
cargo +stable test --locked --all-targets
openspec validate plan-torrent-downloader --strict
```

The tests generate small repeatable payloads and metainfo using librqbit's own
torrent generator. Seeder and client communicate over loopback TCP with
ephemeral ports. Trackers, DHT, local-service discovery, UPnP port forwarding,
and persistence are disabled. Downloading Cargo dependencies initially needs
registry access; the test transfers do not require public peers or trackers.

## Dependency choices

- librqbit `9.1`, resolved to **9.1.0**, with defaults disabled and `rust-tls`.
- Tokio for the async runtime and monotonic timers; standard `Duration` for spans.
- clap for CLI arguments; indicatif for progress and duration formatting.
- anyhow for application errors and compatibility with the engine's storage API.
- camino with `serde1` for UTF-8 paths, and camino-tempfile for state-file staging
  and isolated test directories.
- serde/serde_json for the planned destination record; tracing-subscriber for
  engine diagnostics.

Jiff is the preferred choice if calendar/timestamp/timezone handling becomes
necessary. There is no direct calendar-time dependency today. librqbit's DHT
dependency brings chrono transitively. Miette is not in the resolved graph.

See the [compatibility report](docs/compatibility-spike.md) for tested behavior,
dependency features, and the destination-safety checks still needed before the
download CLI and resume feature can be exposed.
