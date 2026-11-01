# rqget

A torrent download CLI built on librqbit. The first guarded download path is
implemented; the complete first-version acceptance work is tracked in
[OpenSpec](openspec/changes/plan-torrent-downloader/README.md).

```sh
cargo run -- path/to/input.torrent -o ./download
cargo run -- 'magnet:?xt=urn:btih:...' -o ./download
cargo run -- path/to/input.torrent -o ./download --resume
```

The destination is dedicated to one torrent. Paths must be UTF-8. Resume
requires a matching rqget record and rechecks existing pieces; it is not a
general overwrite option. Use `--quiet` to suppress routine progress.

Ownership metadata and the stable writer lock live under `.rqget/` inside the
destination. Unrelated files, unsafe path aliases, and mismatched records cause
an error. Partial files are retained after failure. Retry a matching interrupted
transfer with the same source/destination and `--resume`.

Progress goes to stderr; stdout is reserved from status output. Engine diagnostics
are opt-in through `RUST_LOG` and are coordinated with the active progress bar.

## Development

The workspace has three flat sibling directories:

| Crate | Responsibility |
| --- | --- |
| `rqget/` | Thin executable: arguments, signals, terminal progress, and logging |
| `rqget-core/` | Source validation, destination ownership, and download lifecycle |
| `rqget-test/` | Shared local fixtures, used as a development dependency |

Dependency versions, package defaults, and Rust/Clippy lints live in the root
Cargo.toml; every member inherits them. The CLI is the default member, so
`cargo run -- ...` works from the workspace root. Check all members together:

Use stable Rust. The declared supported baseline is Rust 1.99; the compatibility
spike was verified with Rust 1.99.1 on native `aarch64-apple-darwin`. This is a
conservative project baseline, not a claim about librqbit's minimum compiler.
`rust-toolchain.toml` selects the installed stable channel. Cargo.lock records
the dependency resolution; use locked checks and review dependency updates.

```sh
cargo +stable fmt --all --check
cargo +stable clippy --locked --workspace --all-targets -- -D warnings
cargo +stable test --locked --workspace --all-targets
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
- url for source validation, including rejecting encoded magnet file-selection
  parameters before the engine expands them.

Jiff is the preferred choice if calendar/timestamp/timezone handling becomes
necessary. There is no direct calendar-time dependency today. librqbit's DHT
dependency brings chrono transitively. Miette is not in the resolved graph.

The workspace currently has 21 passing tests. The [compatibility report](docs/compatibility-spike.md)
records the initial dependency investigation; the destination policies identified
there are now enforced by `rqget-core`. Remaining first-version progress and
interruption acceptance checks are tracked in OpenSpec.
