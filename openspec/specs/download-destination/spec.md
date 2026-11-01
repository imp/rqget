# download-destination Specification

## Purpose
Protect destination data by defining contained payload writes, exclusive writer ownership, and explicit verified reuse of a previous rqget transfer.

## Requirements

### Requirement: Filesystem paths have an explicit UTF-8 contract

rqget SHALL support UTF-8 source paths, destination paths, canonicalized roots, and payload filenames. It SHALL reject unsupported encodings with a contextual diagnostic before payload writes. It SHALL NOT perform lossy filename conversion or silently rename payload files to accommodate an encoding error.

#### Scenario: Unicode and spaces
- **WHEN** supported source, destination, or payload names contain Unicode characters or spaces
- **THEN** rqget preserves those names through path handling and file creation

#### Scenario: Non-UTF-8 input or destination
- **WHEN** a supplied filesystem path or its canonicalized form is not valid UTF-8
- **THEN** rqget reports the unsupported path encoding without a panic
- **AND** does not create payload files or substitute a different path

#### Scenario: Non-UTF-8 payload name
- **WHEN** resolved torrent metadata contains an unsupported filename encoding
- **THEN** rqget rejects the transfer before payload writes
- **AND** does not silently replace invalid filename bytes

### Requirement: New transfers require an unambiguous destination

A new transfer SHALL use a newly created or empty directory dedicated to that transfer. rqget SHALL create a versioned ownership record before payload writes. It SHALL refuse existing payload data without explicit resume authorization and SHALL NOT automatically adopt or delete unknown files.

#### Scenario: New destination
- **WHEN** the chosen destination does not exist and its parent permits creation
- **THEN** rqget creates the destination and records the resolved torrent identity and payload layout before downloading

#### Scenario: Empty existing directory
- **WHEN** the destination is an existing empty directory
- **THEN** rqget can initialize a new transfer there
- **AND** its own newly created lock infrastructure does not count as pre-existing payload data

#### Scenario: Existing unrelated files
- **WHEN** a new transfer targets a nonempty destination containing unrelated files
- **THEN** rqget refuses the transfer without changing those files
- **AND** explains that a dedicated empty destination is required

#### Scenario: Destination state cannot be written
- **WHEN** the ownership record cannot be written completely
- **THEN** rqget fails without writing payload files
- **AND** does not treat an incomplete record as authorization for resume

### Requirement: Payload paths remain within the destination

rqget SHALL validate the complete resolved payload layout before payload writes. It SHALL reject absolute paths, parent traversal, conflicting paths, reserved `.rqget` paths, and existing symlink components that escape or alias payload locations. No torrent metadata SHALL authorize a write outside the canonical destination root.

#### Scenario: Traversal in metainfo
- **WHEN** a torrent contains an absolute payload path or parent-directory traversal
- **THEN** rqget rejects the torrent before creating payload files
- **AND** files outside the destination remain unchanged

#### Scenario: Symlink in a payload path
- **WHEN** a payload path would follow an existing symlink beneath the destination
- **THEN** rqget rejects the transfer before writing through that path
- **AND** the symlink target remains unchanged

#### Scenario: Reserved state path
- **WHEN** torrent metadata names a payload under `.rqget`
- **THEN** rqget rejects the torrent before payload writes
- **AND** the ownership record and lock remain protected

#### Scenario: Duplicate or conflicting layout
- **WHEN** payload paths duplicate one another or require a path to be both a file and a directory
- **THEN** rqget rejects the complete layout before creating payload files

### Requirement: At most one rqget writer owns a destination

rqget SHALL acquire an exclusive destination writer lock before resolving or starting a transfer and retain it through shutdown. Another rqget process targeting the same canonical destination SHALL fail promptly without modifying payload or ownership state. Process exit SHALL allow a later invocation to acquire ownership without manual stale-lock deletion.

#### Scenario: Concurrent invocation
- **WHEN** another rqget process owns the destination lock
- **THEN** a second invocation fails with a destination-in-use diagnostic
- **AND** does not alter the first transfer's payload or record

#### Scenario: Directory alias
- **WHEN** two invocations address the same destination through different filesystem path spellings
- **THEN** they contend for the same exclusive writer ownership

#### Scenario: Previous process terminated
- **WHEN** an earlier process exits or is killed
- **THEN** a later invocation can acquire the released operating-system lock
- **AND** a leftover lock file is not itself treated as an active writer

### Requirement: Resume requires explicit matching ownership

The `--resume` flag SHALL permit existing-file writes only when a complete supported rqget record matches the resolved torrent identity and payload layout. Missing, corrupt, unsupported, or mismatched records SHALL fail without payload writes. Resume SHALL NOT serve as a general overwrite or import option.

#### Scenario: Matching interrupted transfer
- **WHEN** `--resume` targets a previous rqget destination with matching identity and layout
- **THEN** rqget may reuse and repair that transfer's payload files after validation

#### Scenario: Resume requested for another torrent
- **WHEN** the destination record identifies a different torrent or layout
- **THEN** rqget refuses resume and preserves all existing payload files

#### Scenario: Resume record unavailable
- **WHEN** `--resume` is supplied with a missing, corrupt, or unsupported-version record
- **THEN** rqget reports why resume is unsafe and fails without payload writes

#### Scenario: Existing download without resume flag
- **WHEN** a previous rqget destination is used without `--resume`
- **THEN** rqget refuses to overwrite or silently resume its payload
- **AND** explains the explicit resume command

### Requirement: Existing pieces are verified before reuse

Resume SHALL recheck existing pieces against the torrent hashes, reuse valid pieces, and download missing or corrupt pieces. File length or a previous completion record alone SHALL NOT establish correctness. An already complete destination SHALL be verified before success.

#### Scenario: Interrupted payload
- **WHEN** a matching destination contains some valid pieces and some missing pieces
- **THEN** rqget verifies and retains valid pieces and fetches the missing pieces
- **AND** the final bytes match the torrent payload

#### Scenario: Corrupt existing bytes
- **WHEN** a matching destination contains a corrupt piece
- **THEN** rqget detects it during checking and repairs it from peers
- **AND** does not report success while the corrupt piece remains

#### Scenario: Complete destination
- **WHEN** a matching destination already contains all expected valid bytes
- **THEN** rqget rechecks them and exits successfully without requiring a peer to provide payload bytes

### Requirement: Failures preserve recoverable data

rqget SHALL retain ownership metadata and partial payloads after interruption or transfer failure. It SHALL NOT automatically delete or rename user data during error cleanup. A subsequent explicit resume SHALL validate the destination again rather than trusting the previous process outcome.

#### Scenario: Interrupted download retained
- **WHEN** a transfer is interrupted after writing partial payload data
- **THEN** its state record and partial files remain available
- **AND** a later matching `--resume` can recheck them

#### Scenario: Transfer failure retained
- **WHEN** a network or storage failure terminates the transfer
- **THEN** rqget stops transfer activity and retains existing data
- **AND** the next invocation revalidates identity, paths, and pieces before reuse
