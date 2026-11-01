# Tasks

## 1. Typed CLI and request

- [ ] 1.1 Add repeatable numeric IPv4/IPv6 `--peer` parsing and the core Request field. Verify no-peer and repeated valid addresses with parser tests, malformed/missing/hostname values with process status-2/no-destination tests, and document rqbit usage in README.

## 2. Bootstrap both transfer stages

- [ ] 2.1 Pass peers to metadata and payload adds, preserving default discovery, destination policies, and common shutdown. Prove trackerless metainfo/magnet downloads, multiple candidates with one unavailable, and matching-record corruption repair using supplied peers. Verify unavailable-peer waiting stays cancellable and record the proof in docs/acceptance.md.

## 3. Integration verification

- [ ] 3.1 Pass formatting, strict workspace Clippy, workspace tests, and strict OpenSpec validation. Review the diff for dependency/state-format changes and commit the implementation and evidence.

## Workflow follow-up

- Archive the completed change, validate canonical specs/archive, and commit the archive.
