# rqget

A foreground torrent download CLI built on librqbit. It downloads one BitTorrent
v1 torrent, verifies its pieces, stops the session, and exits. The implemented
contract and its tests are recorded in the [acceptance report](docs/acceptance.md).

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
transfer with the same source/destination and `--resume`. If interrupted before
metadata resolution creates the ownership record, retry the original command
without adding `--resume`.

Progress goes to stderr; stdout is reserved from status output. Engine diagnostics
are opt-in through `RUST_LOG` and are coordinated with the active progress bar.

On a terminal, progress shows the phase, verified bytes, percentage, engine
download/upload rates, approximate ETA, and peers. Checking reports bytes examined;
retained pieces do not count as fresh network traffic. Unknown ETA shows elapsed
time. Redirected output uses plain phase changes and 30-second status heartbeats.

Ctrl-C cancels metadata resolution, checking, or downloading and exits with `130`;
SIGTERM on Unix exits with `143`. Shutdown has a five-second deadline and retains
the destination lock until cleanup ends. Exit `0` means verified completion
(or help/version), `1` means a runtime/input/storage failure, and `2` means a usage
error. A shutdown failure remains an error; signals do not hide prior fatal errors.
Hard kills cannot guarantee cleanup or durable disk writes; resume rechecks pieces.

Supported sources are local metainfo files and v1 magnet URIs. Remote metainfo or
HTTP payload URLs, pure v2 torrents, zero-length payloads, file selection, and
continued seeding are outside this version's scope. A torrent with no peers waits
and remains interruptible; there is no overall download timeout.

## Download from a LAN seeder

For an rqbit server seeding the same torrent, pass its LAN IP and TCP peer port:

```sh
cargo run -- input.torrent -o ./download --peer 192.168.1.20:4240
cargo run -- 'magnet:?xt=urn:btih:...' -o ./download --peer 192.168.1.20:4240
cargo run -- input.torrent -o ./download --resume --peer 192.168.1.20:4240
```

Replace the example address with your server's address. In rqbit's logs, look for
`Listening on TCP … for incoming peer connections`. Use that peer port and keep
the matching torrent active for seeding. The web/API port serves a different protocol.

Repeat `--peer` for multiple candidates. Numeric IPv6 uses brackets, for example
`--peer '[2001:db8::20]:4240'`. Hostnames are not accepted. The supplied peers are
used for both magnet metadata and payload transfer; normal discovery remains
enabled. They are connection hints, not an exclusive peer list or saved state.

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
openspec validate --all --strict
openspec validate --archived --strict
```

The tests generate small repeatable payloads and metainfo using librqbit's own
torrent generator. Seeder and client communicate over loopback TCP with
ephemeral ports. Tests disable public discovery, UPnP port forwarding, and
persistence; selected cases use a test-owned loopback HTTP tracker.
Downloading Cargo dependencies initially needs
registry access; the test transfers do not require public peers or trackers.

## Dependency choices

- librqbit `9.1`, resolved to **9.1.0**, with defaults disabled and `rust-tls`.
- Tokio for the async runtime and monotonic timers; standard `Duration` for spans.
  tokio-util supplies cancellation tokens and abort-on-drop task ownership.
- clap for CLI arguments; indicatif for progress and duration formatting.
- anyhow for application errors and compatibility with the engine's storage API.
- camino with `serde1` for UTF-8 paths, and camino-tempfile for state-file staging
  and isolated test directories.
- serde/serde_json for the destination record; tracing-subscriber for engine
  diagnostics; parking_lot for the shared synchronous progress/log output lock.
- url for source validation, including rejecting encoded magnet file-selection
  parameters before the engine expands them.

Jiff is the preferred choice if calendar/timestamp/timezone handling becomes
necessary. There is no direct calendar-time dependency today. librqbit's DHT
dependency brings chrono transitively. Miette is not in the resolved graph.

The workspace has 42 passing tests, including direct bootstrap peers, real process
signals, controlled timers, and terminal rendering. The [compatibility report](docs/compatibility-spike.md)
records the initial dependency investigation; the [acceptance report](docs/acceptance.md)
records the completed application checks and their limits. All packages remain
unpublished; the project license must be selected before distribution.
