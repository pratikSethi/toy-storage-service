# ADR 0006: Object bytes flow through FrontdoorService

Status: accepted, mostly built

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **cell**: one ObjectIndexService plus its ChunkStoreService nodes. It is the unit a fault is meant to stay inside. One cell exists today.
> - **ADR** (architecture decision record): a short note on one decision and why it was made.

## Context

Presigned URLs in S3 let a client skip the customer's own app server. They do not skip S3's front end: the bytes still pass through S3's front-end fleet. Storage nodes in S3 are never reachable by clients.

TS2 has to decide the same thing: do object bytes pass through FrontdoorService, or can a client talk to ChunkStoreService nodes directly?

## Decision

Object bytes go through FrontdoorService. That gives one policy point (admission, auth, quota, limits), and one coordinator for chunking, hashing and replication. FrontdoorService scales horizontally and is bound by NIC bandwidth. ChunkStoreService nodes are never client-reachable.

Two invariants keep a second path cheap to add later:

1. Chunking and placement logic live in the library crate `ts2-core` inside FrontdoorService, not in the HTTP handler and not in ObjectIndexService.
2. ChunkStoreService verifies the checksum on every write. From v0.2 it also requires a per-request token minted by FrontdoorService.

Presigned URLs are an API feature in v0.3 (a signed URL that FrontdoorService accepts). They do not change the topology.

**Today:** object bytes flow through FrontdoorService and ChunkStoreService nodes are not published to the host. ChunkStoreService verifies the checksum on writes. The per-request token check exists in ChunkStoreService but is off by default, and FrontdoorService does not send a token. Presigned URLs are not built (v0.3).

## Consequences

- FrontdoorService network card (NIC) and CPU are the throughput ceiling for one request path. The fix is more FrontdoorService instances behind the load balancer.
- One place to enforce limits and emit metrics for every byte.
- Clients cannot bypass admission.

## Alternatives considered

- **Direct-to-cell thick client** for trusted internal bulk workloads, in the style of Ceph librados or HDFS clients. The client chunks and writes to ChunkStoreService nodes itself. Deferred: it needs the tokens above and a client library built on `ts2-core`.
- **Redirect to a ChunkStoreService node.** Rejected: exposes nodes to clients and splits the policy point.
