# Tasks

The proposal and specs define the first-version contract. All implementation and acceptance tasks are complete. The initial milestone is recorded in `docs/compatibility-spike.md`; final scenario coverage, commands, and proof limits are recorded in `docs/acceptance.md`. Scope changes belong in the proposal/specs before implementation, not in untracked feature additions.

## 1. Prove the dependency and storage seam

- [x] 1.1 Add the design's explicit dependencies (librqbit `9.1`, anyhow, camino/camino-tempfile, and indicatif included) and generate Cargo.lock with stable Rust; verify `cargo +stable clippy --locked --workspace --all-targets -- -D warnings`, inspect `cargo tree -e features`, confirm native arm64 with no translation, and record the supported compiler plus resolved TLS/features in README.md and the compatibility report. No direct tempfile/calendar-time or second diagnostic stack is needed; Rust 1.99 is the conservative supported baseline, verified with 1.99.1.
- [x] 1.2 Add repeatable small BitTorrent v1 fixtures and local seeder test support using camino-tempfile; prove locked librqbit 9.1.0 resolves a magnet, returns list-only metadata without payload writes, adds resolved bytes with the intended output layout, exposes verified completion/live errors, cancels on stop, and rechecks existing files. Local metainfo and magnet/multi-file tests passed without trackers/DHT, including exact bytes, peerless completion recheck, corruption detection, and injected fatal write failure.
- [x] 1.3 Verify the filesystem boundary with traversal, reserved-name, duplicate/conflicting-path, encoding, and symlink probes, and prove standard-library exclusive lock release after a subprocess is killed. Passing tests preserve outside sentinels. The report/design identify engine-owned validation and remaining rqget-owned raw UTF-8, reserved-path, prefix-conflict, and symlink checks; these are prerequisites of task 2.2, not a completed public destination guard.

## 2. Deliver a safe first download command

- [x] 2.0 Convert to a flat rqget/rqget-core/rqget-test workspace with a thin executable, shared development fixtures, inherited dependencies/package defaults, and repository-wide lints. Verified all 14 existing tests and strict workspace Clippy; integration tests no longer include production source files by path.

The guarded application path and flat workspace are implemented. CLI process
tests, private destination tests, and public core integration tests cover source
validation, local/magnet downloads, record publication, ownership and aliases,
corruption repair, fatal-write cleanup, progress, and interruption. The workspace
suite passes 39 tests, including child-process helper entry points.

- [x] 2.1 Implement argument parsing with camino paths and documented `SOURCE -o DIRECTORY`, help, version, and quiet behavior. Verify parser/process tests for missing/extra arguments and rejected URI/bare-hash/stdin/selection sources, plus Unicode/space-containing path success and non-UTF-8 path rejection without lossy conversion. Prove no network/destination side effects for help and usage errors; document the UTF-8 path contract and commands in README.md.
- [x] 2.2 Implement the destination guard with camino paths, canonical root, nonblocking native writer lock, raw metadata UTF-8/prefix-conflict/reserved-path/symlink preflight, and ownership-record staging via same-directory camino-tempfile before atomic persist and payload writes. Verify new/empty destination success, unrelated-file refusal, path/symlink/state-name/encoding rejection, canonicalized-path conversion failure, record-write/persist failure, and two-process contention tests; document the dedicated-destination layout.
- [x] 2.3 Replace the starter entry point with the concrete CLI → destination → session workflow for a local metainfo file and magnet source. Resolve metadata before actual add, download all files, observe engine fatal errors/verified completion, and shut down on all post-session exits. Verify end-to-end local single-file and multi-file downloads by comparing exact fixture bytes and asserting exit statuses; document success/error examples with the implementation.

## 3. Deliver explicit verified resume

- [x] 3.1 Implement `--resume` as matching-record validation followed by permission to recheck/write existing payloads, with engine session/fast-resume/DHT persistence disabled. Verify missing/corrupt/unsupported/mismatched record and absent-flag rejection preserve file hashes, and unknown files are not adopted; document the resume contract and its explicit repair permission.
- [x] 3.2 Prove partial resume and corruption repair using controlled local peers: interrupt a fixture transfer, retain its record/data, resume it, and compare the final bytes; corrupt one retained piece and prove it is detected/repaired. Verify an already complete destination succeeds after recheck with no payload peer. Passed core interruption and application tests cover throttled partial progress, retained valid pieces, exact-byte resume, corrupt-piece repair, and peerless recheck.

## 4. Deliver observable and interruptible operation

- [x] 4.1 Add indicatif phase/progress rendering using engine speed/ETA, one-second stat samples, 30-second redirected heartbeats, quiet behavior, contextual anyhow errors, and coordinated engine logs on stderr. Six renderer tests cover checking versus traffic, retained bytes, unknown/zero totals, missing ETA, quiet diagnostics, plain output, and actual terminal rendering/log coordination via InMemoryTerm. Process tests cover clean stdout and visible contextual errors; README documents output and `RUST_LOG`.
- [x] 4.2 Use standard Duration and Tokio monotonic Instant/interval/timeout for elapsed, no-progress, heartbeat, and shutdown timing; reuse indicatif duration formatters. Paused-time tests prove the 30-second heartbeat, skipped missed ticks, and independent five-second shutdown deadline. Production transfer timing has no wall-clock dependency. Jiff remains documented for future calendar requirements; test-util is dev-only.
- [x] 4.3 Add signal handling before network work and a common async shutdown path with the five-second bound. Child-process tests send real SIGINT during metadata/checking/downloading and SIGTERM during downloading, asserting 130/143, record retention and post-exit lock release. Core tests prove ownership at stopping, startup I/O cancellation, late shutdown interruption, and retained partial/complete data. README/report document hard-kill and durability limits.
- [x] 4.4 Exercise fatal errors after live transfer begins, storage failure, no-peer waiting/cancellation, and shutdown timeout without using real disk exhaustion or a public swarm. Injected storage failures remain errors even if a late signal arrives; controlled-time tests prove shutdown timeout; no-peer and metadata tests remain cancellable. Invalid lengths/backend metadata-task failures cannot unwind the caller or write payloads. Tests bound waits and reap child processes.

## 5. Verify the complete contract

- [x] 5.1 Passed `cargo +stable fmt --all --check`, `cargo +stable clippy --locked --workspace --all-targets -- -D warnings`, and `cargo +stable test --locked --workspace --all-targets` (39 tests). Automated fixtures exercise the documented new-download → interrupt → resume lifecycle at the core boundary; real child signals exercise the production execute/report boundary with local test options. `docs/acceptance.md` maps both specs to evidence, resolved dependency/features, and native arm64 proof (translation 0). No service, configuration, or protocol surface was added.

## Workflow follow-up

- Keep the baseline scope, including the required dedicated output directory, current before any later milestone. The first-version public download workflow is implemented and accepted by the recorded checks.
- Select rqget's project license before distributing binaries; librqbit's Apache-2.0 metadata does not select this repository's license.
- Archive only after implementation tasks and their acceptance checks are complete. Keep proposal/specs current if user-directed scope changes arrive.
