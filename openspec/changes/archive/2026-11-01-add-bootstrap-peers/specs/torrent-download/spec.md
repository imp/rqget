# Torrent Download Delta

## ADDED Requirements

### Requirement: Explicit bootstrap peers

The CLI SHALL accept repeatable `--peer <IP:PORT>` options using numeric IPv4 or bracketed IPv6 socket addresses. It SHALL use supplied peers for magnet metadata resolution and payload transfer, including resume. These peers SHALL supplement ordinary discovery rather than restrict it. Omitting the option SHALL preserve existing discovery behavior. Peer hints SHALL NOT become destination ownership metadata.

#### Scenario: Direct local metainfo transfer
- **WHEN** a user supplies a local metainfo file and a reachable peer seeding that torrent
- **THEN** rqget can download and verify the payload from that peer without a tracker or DHT

#### Scenario: Direct magnet transfer
- **WHEN** a user supplies a v1 magnet URI and a reachable peer providing its metadata and payload
- **THEN** rqget uses the supplied peer for both metadata lookup and the subsequent verified payload download without requiring a tracker or DHT

#### Scenario: Multiple bootstrap peers
- **WHEN** `--peer` is repeated, including an unavailable peer and one seeding the torrent
- **THEN** rqget supplies all addresses as connection candidates and can complete using the available peer

#### Scenario: Address syntax
- **WHEN** a user supplies `--peer 192.168.1.20:4240` or `--peer '[2001:db8::20]:4240'`
- **THEN** rqget accepts the numeric socket address
- **AND** a malformed address, hostname, missing port, or missing option value instead produces a usage diagnostic and status `2` before destination or network activity

#### Scenario: Explicit peer on resume
- **WHEN** a user supplies `--resume` with a matching destination and a peer serving missing or corrupt pieces
- **THEN** rqget rechecks existing pieces and can repair the payload through that peer
- **AND** the peer hint does not change the destination's recorded identity or layout

#### Scenario: Unavailable peer remains interruptible
- **WHEN** a supplied peer is unavailable and no other peer provides the torrent
- **THEN** rqget remains waiting and interruptible under the existing lifecycle rules without claiming success
