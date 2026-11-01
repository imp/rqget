# torrent-download Specification

## Purpose
Define a predictable foreground command for downloading one torrent, reporting its progress, and terminating with an accurate result.

## Requirements

### Requirement: One explicit source and destination

The CLI SHALL accept exactly one local metainfo file or magnet URI containing a BitTorrent v1 infohash, and a required `-o/--output` directory. It SHALL reject unsupported sources and extra positional sources with an actionable diagnostic. Help and version SHALL work without starting a download.

#### Scenario: Local file download
- **WHEN** a user supplies a valid local metainfo file and a permitted destination
- **THEN** the command downloads every payload file beneath that destination using the metainfo relative layout
- **AND** a local filename need not end in `.torrent`

#### Scenario: Magnet metadata resolution
- **WHEN** a user supplies a supported magnet URI
- **THEN** the command resolves metadata before granting payload writes
- **AND** reports the metadata-resolution phase while waiting

#### Scenario: Unsupported input
- **WHEN** the source is an HTTP/HTTPS URL, bare infohash, stdin shorthand, or pure v2 magnet URI
- **THEN** the command rejects the source without creating payload files

#### Scenario: File-selection request
- **WHEN** a magnet URI requests a subset of files
- **THEN** the command rejects that unsupported selection explicitly
- **AND** does not silently download a different set of files

#### Scenario: Invalid invocation
- **WHEN** the source or destination argument is missing or multiple sources are supplied
- **THEN** the command prints usage guidance and exits with status `2`
- **AND** does not start network activity or create destination state

#### Scenario: Help or version
- **WHEN** a user requests help or version
- **THEN** the command prints the requested information and exits with status `0`
- **AND** does not start network activity or create destination state

### Requirement: Verified completion ends the transfer

The command SHALL report successful download completion only when all payload pieces are verified and no fatal transfer error is present. It SHALL stop transfer activity before exiting with status `0`. It SHALL permit ordinary BitTorrent uploading during download and SHALL NOT continue seeding after shutdown.

#### Scenario: Completed transfer
- **WHEN** the engine reports every payload piece verified and no fatal error
- **THEN** the command stops its session, reports the destination, and exits with status `0`
- **AND** no download or seeding process remains running

#### Scenario: Unsupported zero-length payload
- **WHEN** metainfo describes a zero-length payload unsupported by the engine
- **THEN** the command reports the unsupported input and exits with status `1` without creating payload files
- **AND** progress formatting does not divide by zero or invent successful completion

### Requirement: Useful and script-safe progress

The command SHALL report metadata resolution, checking, downloading or waiting, and terminal outcome on stderr. It SHALL reserve stdout from progress and payload bytes. It SHALL suppress ordinary status with `--quiet` while retaining errors, and SHALL avoid complete source URIs or tracker credentials in ordinary messages.

#### Scenario: Interactive progress
- **WHEN** stderr is a terminal and quiet mode is disabled
- **THEN** progress shows the current phase and available verified byte totals, transfer rates, approximate remaining time, and connected peer count
- **AND** checking bytes are distinguished from downloaded bytes

#### Scenario: Unknown remaining time
- **WHEN** metadata size or a usable remaining-time estimate is unavailable
- **THEN** progress indicates the current phase and elapsed time without inventing a percentage or finish estimate

#### Scenario: Resumed transfer rate
- **WHEN** progress includes pieces retained from an earlier invocation
- **THEN** the displayed transfer rate reflects network transfer estimates rather than counting retained bytes as freshly downloaded data

#### Scenario: Redirected diagnostics
- **WHEN** stderr is redirected to a file or pipe
- **THEN** output contains readable phase transitions, periodic status snapshots while waiting or transferring, and a final outcome without cursor-control sequences
- **AND** stdout contains no progress messages

#### Scenario: Diagnostic during an active progress display
- **WHEN** an engine log or application error is emitted while terminal progress is active
- **THEN** the diagnostic remains readable without being overwritten by the progress display
- **AND** an application failure explains its context while preserving the defined failure status

#### Scenario: Quiet mode
- **WHEN** `--quiet` is supplied
- **THEN** routine progress and successful-completion output are suppressed
- **AND** failures still produce a diagnostic on stderr

#### Scenario: No available peers
- **WHEN** a transfer has no reachable peers and no fatal error
- **THEN** the command reports a waiting state and remains interruptible
- **AND** does not report success or apply an undocumented overall timeout

### Requirement: Duration tracking is monotonic

The command SHALL measure elapsed time, time since progress, refresh deadlines, and shutdown deadlines using a monotonic clock. Wall-clock adjustments SHALL NOT change those durations or trigger premature timeouts.

#### Scenario: Wall-clock adjustment
- **WHEN** the system wall clock is adjusted during a download
- **THEN** elapsed time and shutdown deadlines continue according to monotonic time
- **AND** the command does not show a negative elapsed duration or declare a false no-progress interval

### Requirement: Errors cannot become successful completion

The command SHALL report invalid metainfo, unsupported torrent formats, fatal network or engine failures, and filesystem failures with context on stderr and status `1`. It SHALL observe fatal errors throughout the transfer and stop any created session before exit. A graceful-shutdown timeout SHALL be a reported failure.

#### Scenario: Malformed metainfo
- **WHEN** a supplied local file cannot be decoded as supported metainfo
- **THEN** the command reports the input error and exits with status `1`
- **AND** no payload file is written

#### Scenario: Fatal error after transfer starts
- **WHEN** the engine enters a fatal error state after downloading has begun
- **THEN** the command observes the error, stops the session, and exits with status `1`
- **AND** never reports successful completion or waits forever for a completion notification

#### Scenario: Filesystem failure
- **WHEN** a payload write fails due to a filesystem error
- **THEN** the command reports the storage failure, stops the session, and exits with status `1`
- **AND** preserves partial data for a later explicit resume

### Requirement: Interruption reaches shutdown from every phase

The command SHALL handle Ctrl-C and, on Unix, SIGTERM during session startup, metadata resolution, checking, downloading, and shutdown. It SHALL stop any created session before releasing destination ownership, preserve partial data, and bound graceful shutdown to five seconds. Successful interruption SHALL exit with status `130` for Ctrl-C or `143` for SIGTERM.

#### Scenario: Interrupt metadata lookup
- **WHEN** Ctrl-C arrives while magnet metadata is unresolved
- **THEN** the command cancels the lookup, shuts down, and exits with status `130`
- **AND** does not wait for metadata resolution to finish

#### Scenario: Interrupt active transfer
- **WHEN** Ctrl-C arrives during checking or downloading
- **THEN** the command shuts down within the graceful-shutdown bound and exits with status `130`
- **AND** retains destination ownership until shutdown finishes

#### Scenario: Unix termination
- **WHEN** SIGTERM arrives while a transfer is active on Unix
- **THEN** the command follows the same shutdown and preservation behavior and exits with status `143`

#### Scenario: Shutdown fails to finish
- **WHEN** graceful shutdown exceeds five seconds
- **THEN** the command reports a shutdown failure and exits unsuccessfully
- **AND** does not claim verified successful completion
