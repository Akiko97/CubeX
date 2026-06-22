# CubeX Architecture

CubeX is organized as a small Rust workspace:

- `cubex-protocol`: binary frame format and shared message types.
- `cubex-plugin-sdk`: helpers for writing process-isolated plugins.
- `cubex-core`: TOML config loading, plugin process management, and route dispatch.
- `cubex-store`: append-only message logs and durable record files.
- `cubex-cli`: command line entrypoint.
- `plugins/*`: runnable example plugins.

The runtime uses one configuration file per instance. It does not read runtime
paths or plugin lists from environment variables.

Messages are serialized as length-prefixed binary frames. The current codec is
`bincode` over explicit Rust data structures. JSON is kept out of the hot path.
The shared message header stays small: id, source, topic, and payload. Business
context such as record keys or users belongs in `Payload::Record` fields.

Plugins run as separate processes. The host writes `PluginRequest` frames to
stdin and reads `PluginResponse` frames from stdout. Plugin logs and user-facing
output are returned as response data, so stdout remains reserved for the binary
protocol.

Routing is declarative. A route can match on source, topic, and payload kind,
then fan out to one or more plugin targets.
The host assigns the source of every emitted plugin message before routing, so
plugins cannot impersonate other plugins by filling the `source` field. Control
payloads are host-owned and are rejected when emitted by plugins.

Durability is split deliberately: the runtime can append emitted messages to an
event log, while plugins can use `cubex-store` for domain records. Both formats
are binary and serde-backed. Event log appends and reads reject malformed
messages. Record files are validated on load so persisted record keys remain
non-empty, unpadded, and consistent with their map entries; stored record
messages must also keep valid non-control IDs, sources, and topics.
The CLI can print binary event logs with `cubex events <path>` and record files
with `cubex records <path>`.

File and timer behavior lives in plugins rather than hardcoded runtime branches.
This keeps the core scheduler small while still supporting common source/sink
patterns through the same message protocol.

Network and cryptographic behavior follow the same rule. TCP and SHA-256 are
ordinary plugins, so the runtime remains a scheduler and protocol host rather
than a pile of special cases.
