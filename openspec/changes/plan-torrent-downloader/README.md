# plan-torrent-downloader

Define the initial rqget torrent CLI, librqbit integration, safety boundaries, and dependency choices before implementation.

Status: compatibility and guarded download milestones are complete, including
explicit record-checked resume. The workspace suite has 21 passing tests.
The flat rqget/rqget-core/rqget-test workspace and inherited lint policy are implemented.
See the repository's [compatibility report](../../../docs/compatibility-spike.md).

- [Proposal and scope](proposal.md)
- [Architecture, dependency choices, and source evidence](design.md)
- [Download behavior](specs/torrent-download/spec.md)
- [Destination safety and resume](specs/download-destination/spec.md)
- [Tracked implementation and verification tasks](tasks.md)

Proposed v1 command: `rqget <SOURCE> -o <DIRECTORY> [--resume] [--quiet]`.
`SOURCE` is one magnet URI or local metainfo file. Download all files, report progress,
and exit after verified completion. The output directory is dedicated to one torrent;
resuming requires explicit permission and matching rqget state.

Dependency decisions: librqbit `9.1` with a committed lockfile; indicatif progress;
anyhow errors shared with the engine; camino UTF-8 paths and camino-tempfile staging/fixtures.
Use standard durations and monotonic Tokio timing; prefer Jiff if wall-clock or
calendar handling becomes necessary. Details and features are in the design.

The first version excludes HTTP payload downloads, continued seeding, file selection,
and a daemon/API/GUI. This baseline is a proposal for review; it is not a claim that
those scope choices were separately confirmed by the user.

Planning validation: `openspec validate plan-torrent-downloader --strict`.
