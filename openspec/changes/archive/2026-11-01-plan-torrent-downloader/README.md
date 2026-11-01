# plan-torrent-downloader

Record the implemented initial rqget torrent CLI, librqbit integration, safety boundaries, and dependency choices.

Status: implementation and acceptance are complete, including explicit
record-checked resume, progress rendering, and graceful interruption.
The workspace suite has 39 passing tests with strict Clippy and formatting.
The flat rqget/rqget-core/rqget-test workspace and inherited lint policy are implemented.
See the repository's [compatibility report](../../../../docs/compatibility-spike.md).
Final scenario coverage and proof limits are in the [acceptance report](../../../../docs/acceptance.md).

- [Proposal and scope](proposal.md)
- [Architecture, dependency choices, and source evidence](design.md)
- [Download behavior](specs/torrent-download/spec.md)
- [Destination safety and resume](specs/download-destination/spec.md)
- [Tracked implementation and verification tasks](tasks.md)

Implemented v1 command: `rqget <SOURCE> -o <DIRECTORY> [--resume] [--quiet]`.
`SOURCE` is one magnet URI or local metainfo file. Download all files, report progress,
and exit after verified completion. The output directory is dedicated to one torrent;
resuming requires explicit permission and matching rqget state.

Dependency decisions: librqbit `9.1` with a committed lockfile; indicatif progress;
anyhow errors shared with the engine; camino UTF-8 paths and camino-tempfile staging/fixtures.
Use standard durations and monotonic Tokio timing; prefer Jiff if wall-clock or
calendar handling becomes necessary. Details and features are in the design.

The first version excludes HTTP payload downloads, continued seeding, file selection,
zero-length payloads, and a daemon/API/GUI.

Validation: `openspec validate --all --strict` and `openspec validate --archived --strict`.
