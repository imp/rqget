# Tasks

The proposal and specs define the first-version contract. Mark tasks complete only with implementation and verification evidence; the first milestone's results are recorded in `docs/compatibility-spike.md`. Scope changes belong in the proposal/specs before implementation, not in untracked feature additions.

## 1. Prove the dependency and storage seam

- [x] 1.1 Add the design's explicit dependencies (librqbit `9.1`, anyhow, camino/camino-tempfile, and indicatif included) and generate Cargo.lock with stable Rust; verify `cargo +stable clippy --locked --all-targets -- -D warnings`, inspect `cargo tree -e features`, confirm native arm64 with no translation, and record the supported compiler plus resolved TLS/features in README.md and the compatibility report. No direct tempfile/calendar-time or second diagnostic stack is needed; Rust 1.99 is the conservative supported baseline, verified with 1.99.1.
- [x] 1.2 Add repeatable small BitTorrent v1 fixtures and local seeder test support using camino-tempfile; prove locked librqbit 9.1.0 resolves a magnet, returns list-only metadata without payload writes, adds resolved bytes with the intended output layout, exposes verified completion/live errors, cancels on stop, and rechecks existing files. Local metainfo and magnet/multi-file tests passed without trackers/DHT, including exact bytes, peerless completion recheck, corruption detection, and injected fatal write failure.
- [x] 1.3 Verify the filesystem boundary with traversal, reserved-name, duplicate/conflicting-path, encoding, and symlink probes, and prove standard-library exclusive lock release after a subprocess is killed. Passing tests preserve outside sentinels. The report/design identify engine-owned validation and remaining rqget-owned raw UTF-8, reserved-path, prefix-conflict, and symlink checks; these are prerequisites of task 2.2, not a completed public destination guard.

## 2. Deliver a safe first download command

- [ ] 2.1 Implement argument parsing with camino paths and documented `SOURCE -o DIRECTORY`, help, version, and quiet behavior. Verify parser/process tests for missing/extra arguments and rejected URI/bare-hash/stdin/selection sources, plus Unicode/space-containing path success and non-UTF-8 path rejection without lossy conversion. Prove no network/destination side effects for help and usage errors; document the UTF-8 path contract and commands in README.md.
- [ ] 2.2 Implement the destination guard with camino paths, canonical root, nonblocking native writer lock, raw metadata UTF-8/prefix-conflict/reserved-path/symlink preflight, and ownership-record staging via same-directory camino-tempfile before atomic persist and payload writes. Verify new/empty destination success, unrelated-file refusal, path/symlink/state-name/encoding rejection, canonicalized-path conversion failure, record-write/persist failure, and two-process contention tests; document the dedicated-destination layout.
- [ ] 2.3 Replace the starter entry point with the concrete CLI → destination → session workflow for a local metainfo file and magnet source. Resolve metadata before actual add, download all files, observe engine fatal errors/verified completion, and shut down on all post-session exits. Verify end-to-end local single-file and multi-file downloads by comparing exact fixture bytes and asserting exit statuses; document success/error examples with the implementation.

## 3. Deliver explicit verified resume

- [ ] 3.1 Implement `--resume` as matching-record validation followed by permission to recheck/write existing payloads, with engine session/fast-resume/DHT persistence disabled. Verify missing/corrupt/unsupported/mismatched record and absent-flag rejection preserve file hashes, and unknown files are not adopted; document the resume contract and its explicit repair permission.
- [ ] 3.2 Prove partial resume and corruption repair using controlled local peers: interrupt a fixture transfer, retain its record/data, resume it, and compare the final bytes; corrupt one retained piece and prove it is detected/repaired. Verify an already complete destination succeeds after recheck with no payload peer, and add these scenarios alongside the resume code.

## 4. Deliver observable and interruptible operation

- [ ] 4.1 Add indicatif phase/progress rendering using engine speed/ETA, one-second stat samples, 30-second redirected heartbeats, quiet behavior, contextual anyhow errors, and coordinated engine logs on stderr. Verify checking is distinct from downloading, resumed bytes do not inflate transfer speed, unknown/zero totals and missing ETA format safely, stdout stays clean, redirected output has no cursor controls, logs/errors do not corrupt bars, and ordinary output avoids full source URIs/credentials; document output and `RUST_LOG` behavior.
- [ ] 4.2 Use standard Duration and Tokio monotonic Instant/interval/timeout for elapsed, no-progress, heartbeat, and shutdown timing; reuse indicatif duration formatters. Verify timing logic with controlled Tokio time (test-only `test-util` as needed), skip missed refresh backlogs, and document Jiff as the choice if calendar/timestamp handling is later required.
- [ ] 4.3 Add signal handling before network work and a common async shutdown path with the five-second bound. Verify process tests for Ctrl-C during unresolved metadata, checking, and transfer; SIGTERM on Unix; retained files/record; writer ownership through shutdown; and expected `130`/`143` statuses. Document interruption and hard-kill recovery limits.
- [ ] 4.4 Exercise fatal errors after live transfer begins, storage failure, no-peer waiting/cancellation, and shutdown timeout without using real disk exhaustion or a public swarm. Verify these cannot hang indefinitely waiting for completion, cannot return success, and cannot leave an active child download process; keep failure tests and diagnostics examples with lifecycle code.

## 5. Verify the complete contract

- [ ] 5.1 Run `cargo fmt --check`, `cargo +stable clippy --locked --all-targets -- -D warnings`, and `cargo +stable test --locked --all-targets`. Run the documented new-download → interrupt → resume commands against the controlled fixture and record results, resolved dependency/features, and native architecture. Confirm every scenario in both specs has focused evidence and no unrequested service, config, or protocol surface was added.

## Workflow follow-up

- Keep the baseline scope, including the required dedicated output directory, current before starting the next milestone. The dependency spike is implemented; the public download workflow remains pending.
- Select rqget's project license before distributing binaries; librqbit's Apache-2.0 metadata does not select this repository's license.
- Archive only after implementation tasks and their acceptance checks are complete. Keep proposal/specs current if user-directed scope changes arrive.
