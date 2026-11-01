# Proposal

## Why

A user has an rqbit seeder on the local network and needs to connect directly without relying on a tracker or public discovery. rqget currently exposes no initial-peer option even though librqbit supports it.

## What Changes

- Add repeatable `--peer <IP:PORT>` for IPv4 and bracketed IPv6 socket addresses.
- Use supplied peers during both magnet metadata resolution and payload downloading, including explicit resume.
- Keep ordinary discovery enabled and preserve existing behavior when no peer is supplied. Supplied peers bootstrap a transfer; they do not restrict its swarm.
- Reject malformed addresses as usage errors before filesystem or network activity, and document rqbit's peer-listening port.

Non-goals: hostname resolution, rqbit HTTP/API integration, automatic LAN scanning, peer-only mode, tracker flags, new retry/timeouts, or saved peer configuration.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `torrent-download`: Add explicit bootstrap peer input and its metadata/download behavior.

## Impact

The CLI adds a typed option; the core request carries socket addresses and its runner passes them to librqbit. Destination records and ownership/resume policies do not change. Existing fixtures prove direct transfers with discovery disabled. No dependencies are added.
