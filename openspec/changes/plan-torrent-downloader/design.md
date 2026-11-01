# Design

## Context

See `proposal.md` for motivation and scope. The compatibility milestone supplies the agreed dependencies and integration tests, and the first guarded download/resume path is now implemented. The requested workspace split is the next structural change. OpenSpec 1.16.0 supplies the `spec-driven` workflow. Initial milestone evidence is recorded in the repository's `docs/compatibility-spike.md`.

The requested library is treated as `librqbit`. Version 9.1.0 was inspected from its published crate, whose recorded upstream revision is `4309d38769ef93f6ee4f01a1ab147009a197fa8f` under `crates/librqbit`. Cargo.lock now resolves that version, and local tests exercise the dependency/storage seam. The full application workflow remains pending.

## Goals / Non-Goals

**Goals:** One complete command lifecycle; one owner of destination safety; library-owned protocol, piece hashing, and peer scheduling; deterministic local acceptance tests.

**Non-Goals:** A reusable downloader framework, separate library crate, backend traits, home-grown torrent parser or wire protocol, background service, or custom persistence engine. Scope expansions require a revised proposal and specs.

## Decisions

### 1. A foreground torrent CLI

Proposed public surface:

```text
rqget <SOURCE> -o <DIRECTORY> [--resume] [--quiet]
rqget --help
rqget --version
```

`SOURCE` is a quoted magnet URI containing a BitTorrent v1 infohash, or a local metainfo file. A local path need not have a `.torrent` suffix. Reject other URI schemes and stdin shorthand in this version. Do not use librqbit's broader `from_cli_argument` helper unchanged: it also accepts URLs, bare hashes, and stdin. Read local paths as filesystem paths, then pass bytes to the engine.

`-o/--output` is required and means a directory dedicated to this transfer, avoiding an implicit write into the working directory. Preserve metainfo relative filenames beneath that root; do not add a second torrent-name directory. Download every payload file; reject magnet file-selection parameters rather than silently interpreting them as “all files”. `--resume` explicitly authorizes repairing matching partial payload files. There is no unrestricted overwrite flag.

Status and diagnostics go to stderr. Use indicatif for terminal spinners and progress bars; redirected stderr uses phase transitions, periodic status snapshots, and a final summary without cursor controls. `--quiet` suppresses ordinary progress and success output, but retains errors. Stdout carries no payload or progress. Do not print complete magnet links or tracker credentials in normal diagnostics.

Exit statuses: `0` for help/version or verified completion, `2` for CLI usage errors, `1` for input/metadata/network/storage/runtime failures, `130` for Ctrl-C, and `143` for SIGTERM on Unix. A peerless torrent remains waiting and interruptible; no arbitrary overall timeout is introduced.

Alternative: wrapping the rqbit executable would require process supervision and parsing an external CLI. Embedding librqbit gives typed control over the lifecycle.

### 2. Keep ownership in three small modules

```text
main -> CLI parsing -> download lifecycle
                       |-> destination validation / ownership / lock
                       |-> librqbit session / metadata / transfer / stats
                       `-> progress and signal handling -> session stop -> exit
```

- `cli`: parse and validate arguments; no peer or file-write policy.
- `destination`: canonical root, rqget state record, exclusive lock, metainfo path preflight, and resume eligibility. It owns permission to enable engine overwrite.
- `download`: hold the session and destination guard, resolve metadata, add one torrent, observe engine state, handle signals, and stop the session on every post-creation exit. Keep progress rendering here until its complexity warrants a module.
- `main`: initialize diagnostics, run the command, and map its outcome to an exit status after cleanup. Do not call process exit inside engine code or bypass destructors.

Use concrete functions and owned guards. No trait for a single torrent backend, event bus, detached progress loop, or parallel second copy of the engine's transfer state.

Use anyhow with contextual errors at rqget's application boundaries, matching librqbit's existing error and storage-trait interfaces. Preserve the source chain and display useful context instead of flattening errors. A second report stack does not justify its dependencies or boundary adaptation here. Clear an active progress display before printing a terminal error, and coordinate engine log output with indicatif so logs do not overwrite the progress line. Render readable errors for both terminal and redirected output; quiet mode must not hide them or alter exit-status mapping.

### UTF-8 path ownership

Use camino's `Utf8PathBuf` for owned application paths and `&Utf8Path` for borrowed paths, including local input, destination, and recorded payload paths. Enable `serde1` for typed paths in the destination record. Use standard-library filesystem operations through `AsRef<Path>` and explicit, lossless conversions at librqbit APIs that own `std::path::PathBuf`. Convert returned standard paths fallibly; report invalid UTF-8 rather than using lossy conversion or unchecked unwraps.

UTF-8 is an explicit supported-path contract, including source/output arguments, canonicalized roots, and torrent payload names. Reject unsupported encodings before payload writes; do not let engine filename normalization silently rename an invalid filename before validation. Unicode names and spaces remain supported. Camino represents encoding; it does not replace canonicalization, symlink checks, or traversal validation.

The spike confirms raw non-UTF-8 metadata may be accepted and decoded by the engine. Inspect `ListOnlyResponse.info.iter_file_details()` and each filename's raw byte components before converting to application paths. Reject invalid UTF-8 there; inspecting only the normalized PathBuf would be too late.

### 3. Resolve metadata before granting payload writes

Use `Session::new_with_opts` and an initial `add_torrent` with `list_only=true` to obtain validated metadata and torrent bytes without creating payload files. Validate all paths and the destination record before adding those resolved bytes for the actual transfer. This avoids a second magnet lookup and fixes the identity/layout used for preflight. Set the engine's subfolder override to the empty relative path so the explicit destination is the payload root.

Use engine validation first. The destination layer must additionally reject absolute paths, parent traversal, duplicate/conflicting payload paths, symlink escapes, and payload paths using reserved `.rqget` state. Test the actual engine/storage boundary; passing a lexical check alone does not prove contained writes. Reject existing symlinks in payload path components before granting writes. The destination is user-controlled local storage; the writer lock coordinates rqget processes, not hostile local programs mutating the tree during the transfer.

The compatibility probes confirm engine rejection of traversal, embedded separators/absolute components, and duplicate real filenames. They also confirm that reserved `.rqget` names and file/directory prefix conflicts pass list-only validation, and overwrite mode opens an existing symlink. Enforce those additional checks in the destination owner before the real add; the spike does not enable an application overwrite/resume path.

Do not expose a seed-only mode or request upload disabling. Ordinary uploading during download is part of BitTorrent; after verified completion explicitly stop the session.

### 4. A small destination record, not session persistence

Use a reserved `.rqget` directory beneath the destination for a versioned JSON record and a stable lock file. The record contains the torrent v1 infohash and validated payload path/length layout. Do not store tracker secrets or arbitrary source URLs. Stage the complete record using camino-tempfile in that same state directory, then persist it atomically before permitting payload writes; retain it after completion or interruption. Same-directory staging avoids cross-filesystem rename assumptions. Reserve `.rqget` before passing metadata to storage. This makes camino-tempfile a runtime dependency, also reused by tests; a separate direct tempfile dependency is unnecessary.

Hold a nonblocking exclusive standard-library file lock for the entire job, including metadata resolution and shutdown. Keep the same lock inode; do not delete the lock file on release. Refuse another writer promptly. Use native `std::fs::File` locking rather than a lockfile-existence check or an extra locking dependency; verify compiler support in the dependency spike.

A new transfer accepts only a newly created or empty destination, ignoring only rqget's just-created lock infrastructure. Existing payload files require `--resume` and a readable, supported record matching the resolved infohash and complete layout. Missing, mismatched, or corrupt records fail without payload writes. Thus `--resume` is not permission to import arbitrary existing files. A failed record write cannot authorize payload writes; a leftover empty state directory can be diagnosed and safely retried without deleting user data.

Only after these checks set librqbit's `overwrite=true`. Keep session persistence, fast-resume, and DHT persistence disabled: re-add the supplied source and let the engine hash existing pieces. Good pieces are reused; corrupt/missing pieces are fetched again. Do not delete partial files on failure. This adds a small state record because the engine's overwrite flag alone does not establish destination ownership; a database and fast-resume cache are unnecessary.

### 5. Observe terminal state independently of progress

Phases are metadata resolution, checking, downloading/waiting for peers, and terminal success/failure/interruption. A checking byte count is not download completion. Display unknown totals honestly; avoid division by zero for empty payloads.

Use a compact header naming the transfer/destination and one active indicatif status display. During metadata resolution show a spinner and elapsed time without a guessed percentage. During initialization label progress as bytes checked. During live transfer show verified bytes/total, percentage, download/upload rates, approximate ETA when available, and connected peer count. Take speeds and ETA from librqbit's live stats, not indicatif's byte-delta estimator: resumed or checked bytes are not newly transferred bytes. Connected seeders and per-file counts are available for later presentation decisions; the default remains an aggregate display rather than a file dashboard.

Sample transfer stats once per second. Use the same samples to update the terminal display and to derive waiting messages from connected-peer count and time since verified progress. Do not infer a specific network failure from a zero rate alone. Redraw spinners independently of the stats rate. When redirected, emit a status snapshot every 30 seconds in addition to immediate phase changes and the final outcome; indicatif's hidden nonterminal bar alone would not provide that heartbeat. Skip missed refreshes instead of emitting a backlog. Keep success based on engine state, not rendered percentages.

Illustrative successive states (not concurrent bars):

```text
ubuntu.iso -> ./downloads/ubuntu
Resolving metadata...  8s elapsed
Checking existing files  38%  1.52 / 4.00 GiB examined
Downloading  38%  1.50 / 4.00 GiB  down 12.4 MiB/s  up 0.8 MiB/s  ~3m 26s left  7 peers
Done - 4.00 GiB verified in 5m 42s
```

Observe `ManagedTorrent::stats()` for state, errors, verified progress, and `finished`. Treat engine error state as failure even after downloading has started. Do not depend only on `wait_until_completed()`: its 9.1.0 implementation checks errors before obtaining the live state, then awaits that live object's completion. A periodic stats branch must remain able to detect later fatal errors. Success requires the engine's finished state with no fatal error, followed by session shutdown; no total-bytes heuristic or fabricated 100%.

Install signal handling before network work. Race cancellation against session creation, metadata resolution, checking, and transfer. Once a session exists, every success, error, and cancellation path reaches common async shutdown before releasing the destination lock. Bound graceful shutdown to five seconds; a shutdown timeout is reported, never converted into successful completion. A hard kill cannot guarantee cleanup; the next resume still rechecks pieces. librqbit's `stop()` pauses torrents, cancels tasks, and currently waits one second; it is not an fsync or power-loss durability guarantee.

### Time handling

- Represent refresh intervals, elapsed spans, timeouts, and engine ETAs with `std::time::Duration`.
- Measure elapsed time and time since progress with `tokio::time::Instant`, a monotonic clock that integrates with Tokio's timer testing. Use `tokio::time::interval` and `timeout` for scheduling and shutdown. Clock adjustments must not alter elapsed durations or timeout behavior.
- Format durations through indicatif's duration formatting helpers. Use the engine's optional ETA as an estimate; display it as unknown until available rather than inventing a finish time.
- If the product later needs wall-clock timestamps, calendar arithmetic, timezone conversion, or date parsing, use Jiff. Do not add it before such a requirement exists, and do not replace monotonic transfer timing with wall-clock subtraction.
- Verify refresh/heartbeat/shutdown timing with controlled Tokio time where possible; enable Tokio's `test-util` only for tests that need it. Indicatif animation timing is presentation, not the owner of transfer state or completion.

### 6. Explicit dependency features

These dependencies are now present in Cargo.toml and Cargo.lock. Features and versions were verified with strict Clippy and the local compatibility tests; most application-facing behavior is still pending.

| Dependency | Proposed constraint / features | Responsibility and tradeoff |
| --- | --- | --- |
| `librqbit` | `9.1`, `default-features=false`, `rust-tls` | Engine, metainfo, peers, trackers, DHT, storage, hashing. Use Cargo's normal compatible range and commit the tested resolution in Cargo.lock. Rust TLS avoids requiring an application-managed OpenSSL installation but still has native crypto/build dependencies. |
| `tokio` | `1`, `rt-multi-thread`, `macros`, `fs`, `time`, `signal` | Runtime required by the engine, local-file reads, timer, and cancellation. Declare features used by rqget rather than relying on transitive feature unification. |
| `clap` | `4`, `derive` | CLI parsing, help, validation, usage exit status. Prefer maintained parsing to a custom argument loop. |
| `anyhow` | `1` | Contextual application errors and direct compatibility with the engine's public storage API. One error stack shared with the dependency; miette is not needed. |
| `camino` | `1.2`, `serde1` | UTF-8 application paths with explicit fallible conversions at external boundaries; rejects non-UTF-8 paths rather than changing them. |
| `camino-tempfile` | `1.4` | Same-directory temporary ownership-record files and isolated UTF-8 test fixtures. Wraps tempfile; replaces rqget's direct tempfile dependency. |
| `indicatif` | `0.18`, `default-features=false`, `unicode-width` | Maintained spinners, bars, terminal width handling, and human duration formatting. Stats/speed/ETA remain engine-owned; no async stream or tracing-span progress integration needed. |
| `serde`, `serde_json` | `1`; serde `derive` | Versioned destination record; these already occur in the engine graph, so reuse the format ecosystem. Validate record version and contents before use. |
| `tracing-subscriber` | `0.3`, `fmt`, `env-filter` | Consume engine diagnostics on stderr, quiet by default except errors; opt-in diagnostics through `RUST_LOG`. No application tracing dependency until used. |

`9.1` means `>=9.1.0, <10.0.0`, including compatible 9.x minor releases; it is not restricted to 9.1 patch updates. Cargo.lock plus `--locked` builds gives reproducibility while permitting explicit compatible updates. The API evidence below remains evidence for 9.1.0, not an assertion about every future compatible release; rerun the focused acceptance suite whenever the lockfile changes.

Do not enable librqbit's HTTP server/client feature, web UI, PostgreSQL, watch, Prometheus, storage examples, UPnP serving, or upload-disabling features. Its default features are `default-tls` and `http-api-client`; disabling defaults does not remove mandatory HTTP/serialization/networking transitive dependencies. Trackers still need HTTP/TLS even though HTTP payload downloads are excluded. No direct tempfile, reqwest, torrent codec, crypto, database, filesystem-lock, or calendar-time crate is needed. Camino-tempfile wraps tempfile, and librqbit-dht brings chrono transitively.

Use stable Rust supporting edition 2024 and standard file locking. The declared project baseline is Rust 1.99, verified with stable 1.99.1 on native arm64; process translation is disabled. This is a conservative supported baseline, not a tested lower bound for the dependency graph. The toolchain file selects stable; Cargo.lock records the tested resolution. Keep all compiler checks on Clippy, with warnings denied, rather than a separate cargo check pass.

## Risks / Trade-offs

- A large engine brings substantial transitive dependencies despite a small CLI → inspect `cargo tree -e features`, build native, and document the resolved features; avoid promising a tiny or pure-Rust binary.
- The engine's filesystem opener is not a complete rqget containment policy, and resume enables existing-file writes → preflight metadata and the actual destination tree, retain the ownership record, hold the writer lock, and test malicious path/symlink fixtures before enabling resume.
- The library offers v1-oriented metadata types and requires a v1 magnet infohash → claim BitTorrent v1 only; reject unsupported metadata with a useful error.
- Magnet resolution and downloads can wait indefinitely without peers → show the waiting phase and keep signals effective. Local tests use explicit peers, with no public tracker or Internet availability dependency.
- JSON records are not an authentication system or a transactional download database → use a dedicated user-controlled destination and reject unsupported/corrupt records; no automatic cleanup or adoption of ambiguous data.

## Implementation and Validation Plan

Follow `tasks.md` in dependency order. Start with a compatibility spike using the exact selected library/features and a local seeded fixture before committing to API assumptions. Land tests and usage documentation with each behavior. Then prove local-file and magnet downloads, interruption and resume, corruption repair, fatal live errors, destination rejection, and two-writer exclusion.

There is no deployment or data migration in this milestone. During implementation, do not migrate unknown destination records or delete ambiguous files; reject them. Source rollback preserves payloads and state, whose version is validated on the next run.

## Verified Dependency Evidence

Inspected 2026-11-01 using the published crate and graph-backed symbol discovery; graph generation `2026-11-01T13:06:21Z`. Coverage checks reported no recorded gaps for the material `src/session.rs`, `src/torrent_state/mod.rs`, `src/torrent_state/stats.rs`, and `src/storage/filesystem/fs.rs` files. Examples were excluded from the graph and `examples/ubuntu.rs` was read directly; Cargo manifests were read directly. Coverage is best-effort, not proof that the crate is correct.

- [Package metadata](https://crates.io/crates/librqbit/9.1.0): version, Apache-2.0 license, upstream repository.
- [Session source](https://docs.rs/crate/librqbit/9.1.0/source/src/session.rs): `AddTorrentOptions` lines 251–301; `SessionOptions` and defaults lines 425–519; `stop` lines 1070–1086; `add_torrent` lines 1098–1186; metadata/list-only and storage setup lines 1231–1422. Confirms explicit overwrite, v1 magnet handling, resolved bytes, and list-only preflight.
- [Managed torrent source](https://docs.rs/crate/librqbit/9.1.0/source/src/torrent_state/mod.rs): `stats` lines 504–565 and `wait_until_completed` lines 592–614. Confirms error and finished fields and the need to observe errors throughout the lifecycle.
- [Filesystem storage](https://docs.rs/crate/librqbit/9.1.0/source/src/storage/filesystem/fs.rs): `init` lines 125–168 uses exclusive creation without overwrite and read/write open with overwrite. This does not prove symlink containment or safe rqget resume by itself.
- [Published manifest](https://docs.rs/crate/librqbit/9.1.0/source/Cargo.toml): feature definitions and mandatory dependencies. The upstream Ubuntu example enables overwrite unconditionally and omits rqget's ownership/shutdown policy; do not copy those choices.
- [Camino 1.2.6](https://docs.rs/crate/camino/1.2.6/source/README.md) and [camino-tempfile 1.4.1](https://docs.rs/crate/camino-tempfile/1.4.1/source/README.md): UTF-8 invariants, standard-path interoperability, serde support, and tempfile wrapping were checked against the published package documentation.
- [Indicatif 0.18.6](https://crates.io/crates/indicatif/0.18.6): published feature metadata and README documentation checked for terminal progress. The selected combined dependency graph passes strict Clippy and the compatibility tests.

Executed proof: local metainfo and magnet/multi-file downloads, exact-byte comparisons, peerless rechecks, same-size corruption detection, injected fatal write failure, unsafe-metadata probes, symlink-open behavior, and cross-process lock exclusion/release. See `docs/compatibility-spike.md` for reproduction commands and limits. Application-level destination enforcement, progress, interruption/resume, and corruption repair transfers remain later acceptance tasks.
