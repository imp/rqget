# Design

## Context

See proposal.md for motivation. The CLI validates arguments into a core Request. The runner first performs a list-only add for metadata, validates the destination, then adds resolved metainfo bytes for payload transfer. librqbit 9.1.0 AddTorrentOptions exposes `initial_peers: Option<Vec<SocketAddr>>` at both calls; the existing compatibility tests already use direct loopback peers.

## Goals / Non-Goals

**Goals:** Reuse the two-stage runner without bypassing metadata preflight, hash verification, destination ownership, resume validation, cancellation, or common shutdown.

**Non-Goals:** A peer abstraction, alternate backend, custom discovery/retry logic, or peer persistence.

## Decisions

- Parse a repeatable clap `--peer <IP:PORT>` directly into `Vec<std::net::SocketAddr>`. IPv6 uses `[address]:port`. This reuses standard parsing and clap's status-2 diagnostics instead of DNS resolution or a custom parser.
- Add `Request.peers`. Empty input maps to the engine's default `None`; otherwise clone the short list once for the metadata task and move it into the payload add. Both stages must receive it, because resolved bytes do not retain bootstrap addresses as torrent metadata.
- Do not alter SessionOptions, discovery, or tracker settings. Unreachable bootstrap peers are ordinary engine connection failures, not a new fatal condition or overall timeout. Keep the existing waiting and cancellation behavior.
- Peer addresses are invocation-only connection hints. Do not include them in ownership records; resume may use a different peer for the same identity/layout.
- Prove the wiring with trackerless local metainfo, magnet, and repair/resume transfers using existing loopback fixtures. Parser/process tests cover repetition, IPv4/IPv6, invalid syntax, and no destination side effects. Run strict Clippy and the workspace suite for Request call-site compatibility.

## Risks / Trade-offs

- An rqbit web/API port is not its peer port → README shows how to find the TCP peer listener and requires an active matching torrent.
- A stale peer address might never serve data → normal discovery/waiting/cancellation remains engine-owned. Document that this is bootstrap, not exclusive routing.
- Existing destination data remains sensitive to overwrite → keep all current record, path, lock, and piece-recheck gates; no record migration is needed.
