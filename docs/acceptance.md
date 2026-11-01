# First-version acceptance

Completed on 2026-11-01 for the [archived OpenSpec change](../openspec/changes/archive/2026-11-01-plan-torrent-downloader/README.md).
The canonical [download](../openspec/specs/torrent-download/spec.md) and
[destination](../openspec/specs/download-destination/spec.md) specs define the contract.
The flat workspace contains
the thin `rqget` CLI, `rqget-core` lifecycle/destination owner, and dev-only
`rqget-test` fixtures. This report supersedes pending application work in the
historical [compatibility spike](compatibility-spike.md).

## Verification

```sh
cargo +stable fmt --all --check
cargo +stable clippy --locked --workspace --all-targets -- -D warnings
cargo +stable test --locked --workspace --all-targets
openspec validate --all --strict
openspec validate --archived --strict
```

The workspace test command passed **42 tests**, none ignored: 10 CLI unit tests,
3 CLI integration tests, 11 core unit tests, 5 application tests, 2 process-lock
tests, 6 interruption tests, and 5 engine compatibility tests. Counts include the
child-process helper entry points. Strict Clippy and formatting passed. The
initial milestone had 39 tests; explicit bootstrap-peer support adds three tests
and extends the existing no-peer cancellation test.

Stable Rust **1.99.1**, `aarch64-apple-darwin`; supported baseline **1.99**.
`file target/debug/rqget` reports Mach-O arm64, and
`sysctl -n sysctl.proc_translated` returns **0**. No Rosetta is used.

## Contract evidence

Each row covers the named scenarios in the two OpenSpec capabilities. Paths
below are relative to the repository root.

| Behavior / scenarios | Durable proof |
| --- | --- |
| Local file (any suffix), magnet metadata, all files and relative layout, verified completion | `rqget-core/tests/application.rs`: `application_resolves_a_magnet_and_downloads_a_single_file`, `application_downloads_then_rechecks_and_repairs_explicit_resume`; `rqget-core/tests/librqbit_compat.rs` also compares exact single/multi-file bytes and preflight without payload writes. |
| Unsupported sources, file selection including encoded keys, invalid invocation, help/version without side effects | `rqget/tests/cli.rs`: `cli_rejects_invalid_invocations_without_destination_side_effects`; source validation in `rqget-core/src/request.rs`. |
| Unicode/spaces; non-UTF-8 supplied/canonical/payload paths | CLI non-UTF-8 test, application multi-file fixture, and `rqget-core/src/destination/tests.rs`: `canonical_root_must_remain_utf8`, `destination_preflight_rejects_unsafe_metadata_without_payload_writes`. macOS can reject invalid filename bytes itself; the test accepts that specific OS refusal. |
| New/empty destination, unrelated files, record-publication failure | Destination tests: `destination_lock_records_and_resume_are_explicit`, `new_transfer_preserves_unrelated_files`, `failed_record_publication_never_creates_payload_files`. |
| Traversal, symlinks, reserved state, duplicates/prefix conflicts, native aliases/hard links | Destination preflight, symlink, hard-link and native-alias tests; engine compatibility probes preserve outside sentinels. |
| Concurrent writers, directory aliases, process exit/kill releases lock | `rqget-core/tests/file_lock.rs`; destination alias/lock tests; CLI signal tests reacquire ownership after child exit. The stable lock inode is retained. |
| Explicit matching resume; missing/corrupt/unsupported/mismatched records; missing flag | Destination record/resume tests and application download/recheck test preserve existing bytes on rejected operations. |
| Interrupted payload, valid-piece reuse, corruption repair, complete peerless recheck | `rqget-core/tests/interruption.rs`: `interrupted_partial_download_keeps_valid_pieces_and_resumes`; application download/recheck/repair test; engine compatibility rechecks. |
| Interactive progress, checking versus traffic, retained-byte rates, unknown ETA/totals, quiet, readable logs | Six tests in `rqget/src/progress/tests.rs`, including indicatif `InMemoryTerm` rendering of the actual bar, percentage, engine ETA, and diagnostic coordination. |
| Redirected plain output and 30-second heartbeat without backlog | `redirected_heartbeat_is_periodic_and_does_not_replay_missed_ticks`; process tests capture plain stderr and verify clean stdout. |
| No peers waits and remains interruptible | `no_peer_transfer_waits_until_cancelled_and_retains_its_record`; progress waiting/unknown-total test. |
| Monotonic elapsed/refresh/shutdown despite wall-clock changes | Production timing uses only standard Duration and Tokio Instant/interval/timeout. Paused Tokio-time tests advance heartbeat/deadline independently of wall time; no system clock is changed. |
| Malformed/unsupported metainfo, zero-length payloads and zero piece length | `malformed_metainfo_error_survives_quiet_mode`; `invalid_lengths_fail_without_payload_writes_or_unwinding_the_caller`. Engine metadata task failures reach cleanup without unwinding the caller. |
| Live fatal/storage failure, preserved data and error precedence | `core_reports_fatal_write_errors_and_releases_destination_ownership`, engine storage-fault test. An injected late signal cannot hide the fatal write error. |
| Metadata/checking/download Ctrl-C, Unix SIGTERM, lock release and retained record | `rqget/src/tests.rs`: `signals_reach_metadata_checking_and_transfer_processes` sends real SIGINT/SIGTERM to child processes; asserts 130/143, exit and released lock. Metadata interruption leaves no payload or record. |
| Session startup cancellation, interruption during shutdown, ownership through cleanup | Core interruption tests cancel stalled startup HTTP I/O and late shutdown; partial-download observer proves lock ownership at stopping. |
| Shutdown deadline cannot succeed | Core unit tests `shutdown_timeout_reports_failure_at_the_deadline` and `normal_shutdown_waits_for_completion` use controlled time; `interruption_cannot_masquerade_as_success` rejects a zero interruption status. |

## Explicit bootstrap peers

The `add-bootstrap-peers` change adds repeatable `--peer <IP:PORT>` to seed
metadata and payload discovery from a known rqbit or other BitTorrent peer.
Numeric IPv4 and bracketed IPv6 are accepted; ordinary discovery stays enabled.
Peer hints are invocation-only, so the same destination can resume using a
different address without changing its ownership record. No dependencies or state
format changes were needed.

- `rqget/src/cli.rs` tests the absent option and repeated IPv4/IPv6 addresses
  through parsing and conversion into the core request.
- `rqget/tests/cli.rs` proves malformed addresses, missing ports/values, hostnames,
  unbracketed IPv6, and HTTP URLs produce status 2 before destination creation.
- `explicit_peers_bootstrap_metainfo_and_magnets_and_repair_resume` in
  `rqget-core/tests/application.rs` downloads both source forms with trackers,
  DHT, and local-service discovery disabled. An unavailable candidate precedes
  the working loopback seeder; each transfer completes with exact bytes. Each
  source then resumes after deliberate corruption, repairs every byte, and
  preserves the original ownership record while using a different peer list.
- `no_peer_transfer_waits_until_cancelled_and_retains_its_record` also covers an
  explicitly unavailable peer, proving status 130 and retained state.

Reproduce the transfer proof with:

```sh
cargo +stable test --locked -p rqget-core --test application explicit_peers_bootstrap_metainfo_and_magnets_and_repair_resume
```

The test uses an embedded librqbit loopback seeder; it does not claim live
validation against the user's LAN rqbit installation. README documents the
command and how to identify rqbit's TCP peer port.

## Controlled transfer reproduction

The documented user workflow is:

```sh
cargo run -- input.torrent -o ./download
# Ctrl-C after some verified progress, then:
cargo run -- input.torrent -o ./download --resume
```

Automated proof runs that lifecycle through the core request boundary against a
generated loopback fixture, throttled to 32 KiB/s. It interrupts after positive
verified progress, asserts status 130, record/partial-data retention and ownership
through stopping, then resumes to status 0 and compares every payload byte.
The application test separately corrupts retained bytes and proves repair, then
rechecks complete data without a payload peer. Reproduce those focused cases with:

```sh
cargo +stable test --locked -p rqget-core --test interruption interrupted_partial_download_keeps_valid_pieces_and_resumes
cargo +stable test --locked -p rqget-core --test application application_downloads_then_rechecks_and_repairs_explicit_resume
cargo +stable test --locked -p rqget --bin rqget signals_reach_metadata_checking_and_transfer_processes
```

The signal child uses the same private execute/report boundary as main with
test-only local engine options. It adds no production switches and does not test
public tracker/DHT availability. Fixture sessions use bounded waits, loopback
peers/trackers and no public discovery; dependencies initially require registry
access. Storage faults and a bounded read gate avoid disk exhaustion and hold
checking deterministically.

## Dependency and lifecycle limits

The [spike's dependency table](compatibility-spike.md#locked-dependency-resolution)
records the original versions, which remain locked. Additional direct dependencies
are `url` **2.5.8** (decoded source validation), `tokio-util` **0.7.20** with `rt`
(owned cancellation/abort-on-drop), and `parking_lot` **0.12.5** (synchronous shared
output lock). All were already in the engine's transitive graph. Dev-only features
are indicatif `in_memory` and Tokio `test-util`/fixture I/O features. librqbit is
**9.1.0**, defaults disabled with `rust-tls`; indicatif is **0.18.6**, defaults
disabled with `unicode-width` in production. No direct calendar-time crate is
needed; Jiff is the choice if calendar handling is added.

The destination is user-controlled storage. Preflight and an OS writer lock do
not defend against hostile local tree mutation during a transfer. librqbit stop
pauses/cancels and waits briefly; acceptance does not prove fsync/power-loss
durability or that every library background task is explicitly joined. Hard
kills release the OS lock, but later resume must still validate paths and hashes.
Zero-length payloads are unsupported by this backend. No public swarm,
cross-platform runtime matrix, release or publication is claimed. All packages
remain `publish = false`; select the repository license before distribution.
