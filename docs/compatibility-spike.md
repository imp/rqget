# Dependency compatibility spike

Completed on 2026-11-01 for OpenSpec tasks 1.1–1.3. This report covers the
dependency/storage seam; it does not mark the complete rqget CLI implemented.

## Toolchain and architecture

- Stable Rust **1.99.1**, host `aarch64-apple-darwin`.
- `uname -m`: `arm64`; `sysctl -n sysctl.proc_translated`: `0`.
- Both integration-test executables inspected with `file`: Mach-O arm64.
- Cargo.toml declares Rust **1.99** as the supported baseline. Older compilers
  have not been tested; no minimum-compiler search was performed.

## Locked dependency resolution

| Direct dependency | Locked version | Explicit features / defaults |
| --- | --- | --- |
| anyhow | 1.0.104 | Defaults; shared with engine APIs |
| camino | 1.2.6 | `serde1`; defaults |
| camino-tempfile | 1.4.1 | Defaults |
| clap | 4.6.7 | `derive`; defaults |
| indicatif | 0.18.6 | `unicode-width`; defaults disabled |
| librqbit | 9.1.0 | `rust-tls`; defaults disabled |
| serde | 1.0.229 | `derive`; defaults |
| serde_json | 1.0.154 | Defaults |
| tokio | 1.54.1 | `rt-multi-thread`, `macros`, `fs`, `time`, `signal`; defaults |
| tracing-subscriber | 0.3.23 | `fmt`, `env-filter`; defaults |

Reproduce the direct dependency list with `cargo +stable tree --locked --depth 1`.
`cargo +stable tree --locked -e features -i librqbit` confirms only its `rust-tls`
feature is enabled; the corresponding indicatif query confirms `unicode-width`.
No librqbit HTTP API/client, web UI, database, watch, storage-examples,
upload-disabling, or UPnP-server-adapter feature was added. Mandatory transitive
HTTP, UPnP-client, networking, and serialization crates remain. Rust TLS still
builds native crypto dependencies; this is not a pure-Rust build.

Anyhow is deliberately the application error dependency as well as the error
type required by librqbit's public storage trait. Miette was removed. There is
no direct tempfile or calendar-time dependency; camino-tempfile wraps tempfile,
and chrono is pulled in through librqbit-dht. Jiff remains the project choice
if wall-clock/calendar features are later needed.

## Executed proof

`cargo +stable test --locked --all-targets` passed **7 tests** (including the
subprocess lock-holder entry point), with none ignored. Strict Clippy over all
targets also passed. The durable tests are:

- `local_metainfo_download_and_piece_recheck`: generate a one-file torrent,
  read it from disk, preflight without payload writes, fetch via loopback,
  compare every byte, stop the seeder, recheck without peers, then corrupt
  same-sized data and prove the piece recheck detects it.
- `magnet_metadata_and_multifile_download`: resolve magnet metadata using an
  explicit local peer, perform list-only preflight, and download nested files
  with a Unicode/space-containing name. Pieces cross file boundaries. The same
  completion, exact-byte, peerless-recheck, and corruption checks apply.
- `payload_write_failure_is_visible_as_fatal_engine_state`: inject a write
  failure through the engine's own storage interface. Prove a payload write
  was attempted, completion stays false, and stats expose the fatal cause.
  No real filesystem is filled or made unwritable.
- `metadata_preflight_exposes_the_destination_policy_boundary`: verify engine
  rejection of traversal, absolute/separator-bearing components, and duplicate
  real paths. Record accepted reserved/conflicting/non-UTF-8 layouts without
  performing payload writes. An outside sentinel remains unchanged.
- `overwrite_mode_does_not_reject_a_symlink` (Unix): exclusive creation rejects
  an existing symlink, but overwrite mode opens it. A paused, same-size,
  peerless probe preserves the outside sentinel while demonstrating why rqget
  must reject links before granting resume writes.
- `exclusive_lock_blocks_another_process_and_releases_after_kill`: a separate
  process holds a standard-library exclusive lock; another opener (including
  a symlink alias on Unix) cannot acquire it. Killing/reaping the holder releases
  ownership while the stable lock inode remains.

All test sessions use bounded observation and shutdown; successful shutdown
must cancel the session token. The library's stop implementation pauses and
cancels, then waits briefly. These tests do not prove fsync/power-loss durability
or that the library joins every background task.

## Design consequences before application writes

The engine seam is usable; no backend abstraction or library fork is needed.
The spike also proves that engine validation alone does not implement rqget's
destination policy. Task 2.2 must enforce:

1. Reserved `.rqget` paths and file-versus-directory prefix conflicts.
2. Strict UTF-8 validation of **raw metadata components**, before the engine's
   encoding conversion; checking only the resulting PathBuf would miss this.
3. Existing symlink-component rejection, canonical-root containment, and the
   exclusive writer guard before any resume-enabled open.
4. A complete matching ownership record before `overwrite=true` is allowed.

The probe tests intentionally document what raw librqbit accepts; they are not
rqget's eventual acceptance policy. The complete destination guard, atomic
record persistence, public CLI, progress renderer, signals, corruption repair
download, and user-facing resume flow are still pending in later tasks.
