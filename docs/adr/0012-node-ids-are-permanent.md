# ADR 0012: Node ids are permanent

Status: accepted, built

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **manifest**: the small record kept for each object. It holds the size and the ordered list of its chunks, with the node that holds each one.
> - **ADR** (architecture decision record): a short note on one decision and why it was made.

## Context

Node ids are written into every manifest ([ADR 0011](0011-replica-recording-and-repair.md)). Today FrontdoorService takes the id from its own config and never checks it against what the node says it is. A mistyped address or a swapped data directory would send reads and writes to the wrong node without any error.

## Decision

Each ChunkStoreService node has a permanent id, created with its data directory. The node reports it in `NodeInfo`. At startup FrontdoorService calls `NodeInfo` on every configured node and refuses to use a node whose reported id differs from the config.

Ids never change and are never reused, even after a node is retired.

**Today:** built in commit `17d361a`. The context paragraph above describes the problem as it was before that commit.

## Consequences

- A wrong address or a wrong disk fails loudly at startup.
- Replacing hardware means the new machine either keeps the old id with the old data, or gets a new id and the old node is retired and repaired around.
- Retired ids stay in old manifests until repair rewrites them.

## Alternatives considered

- **Trust the config.** Rejected: silent misrouting.
- **Use the address as the id.** Rejected: addresses change, ids must not.
