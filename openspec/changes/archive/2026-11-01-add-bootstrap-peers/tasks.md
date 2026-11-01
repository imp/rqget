# Tasks

## 1. Typed CLI and request

- [x] 1.1 Add repeatable numeric IPv4/IPv6 `--peer` parsing and the core Request field. Passed no-peer and repeated IPv4/IPv6 parser tests plus malformed/missing/hostname process tests (status 2, no destination). README documents local metainfo, magnet, resume, repeatable peers, and rqbit's TCP peer port; actual executable help shows the option.

## 2. Bootstrap both transfer stages

- [x] 2.1 Pass peers to metadata and payload adds, preserving default discovery, destination policies, and common shutdown. Passed `explicit_peers_bootstrap_metainfo_and_magnets_and_repair_resume` with trackers/DHT/LSD disabled: both sources download and repair corrupted data, unavailable-first candidates work, and changed peer lists leave the ownership record identical. The existing no-peer test now also proves explicit unavailable-peer cancellation (130, state retained). Evidence and limits are in docs/acceptance.md.

## 3. Integration verification

- [x] 3.1 Passed formatting, strict locked workspace Clippy, all 42 workspace tests, and strict OpenSpec validation. Reviewed the diff: no dependencies, state-format changes, or discovery overrides. Implementation and evidence are committed with this checklist.

## Workflow follow-up

- Archive the completed change, validate canonical specs/archive, and commit the archive.
