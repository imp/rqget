# Proposal

## Why

rqget needs a defined download workflow before the starter binary becomes an application. Reusing librqbit provides the torrent engine, leaving rqget responsible for CLI behavior, destination safety, progress, and process lifetime.

## What Changes

This is the first-version contract with the dependency milestone and first guarded CLI path implemented. Remaining acceptance work is tracked in tasks.md. The user's `librqbt` is interpreted as the published `librqbit` crate; 9.1.0 was verified on 2026-11-01.

- Provide `rqget <SOURCE> -o <DIRECTORY> [--resume] [--quiet]`, with exactly one magnet URI or local `.torrent` file per invocation.
- Download all files in a supported BitTorrent v1 torrent, verify pieces through the engine, report progress, stop the session, and exit after completion.
- Require an explicit destination dedicated to one torrent. Preserve partial downloads on errors or interruption; permit rechecking and resuming only with `--resume` and matching rqget ownership metadata.
- Define useful terminal and redirected output, stable exit statuses, and graceful interruption during metadata resolution, checking, and downloading.
- Record architecture, dependency features, library limitations, and implementation tasks with acceptance checks.

First-version non-goals: HTTP/HTTPS payload or remote `.torrent` downloads, pure BitTorrent v2, multiple concurrent downloads or queues, file selection, continued seeding after completion, daemon/API/GUI, streaming, config files, fast-resume databases, proxy controls, and automatic port forwarding. Normal BitTorrent uploading while a download is active remains enabled.

## Capabilities

### New Capabilities

- `torrent-download`: Single-transfer CLI inputs, progress, completion, failures, and shutdown.
- `download-destination`: Destination ownership, contained writes, one writer, partial-file preservation, and verified resume.

### Modified Capabilities

None. `openspec list --specs --json` returned an empty inventory at planning time.

## Impact

The flat workspace contains `rqget/` for CLI/presentation/signals, `rqget-core/` for the guarded download lifecycle and source validation, and `rqget-test/` for development-only fixtures. Package defaults, dependency versions, and Rust/Clippy lints are shared at the root. The implementation includes a lockfile, stable-toolchain configuration, and local regression tests.

Chosen dependencies: librqbit with the normal `9.1` compatibility range and committed Cargo.lock; Tokio; clap; anyhow for application errors and engine API compatibility; camino for UTF-8 paths; camino-tempfile for atomic state-file staging and test fixtures; indicatif for progress; serde/serde_json for the destination record; and tracing-subscriber for engine logs. Use standard durations and monotonic timers for transfer timing; prefer Jiff if calendar or timestamp handling becomes necessary. librqbit is Apache-2.0; the rqget project license remains a maintainer decision before distribution. See `design.md` and `docs/compatibility-spike.md` for the boundary decisions and executed proof.
