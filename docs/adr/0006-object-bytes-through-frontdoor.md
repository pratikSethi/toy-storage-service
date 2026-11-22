# ADR 0006: Object bytes flow through frontdoor

Status: accepted

## Context

Presigned URLs in S3 let a client skip the customer's own app server. They do not skip S3's front end: the bytes still pass through S3's front-end fleet. Storage nodes in S3 are never reachable by clients.

TS2 has to decide the same thing: do object bytes pass through frontdoor, or can a client talk to chunkstore nodes directly?

## Decision

Object bytes go through frontdoor. That gives one policy point (admission, auth, quota, limits), and one coordinator for chunking, hashing and replication. Frontdoor scales horizontally and is bound by NIC bandwidth. Chunkstore nodes are never client-reachable.

Two invariants keep a second path cheap to add later:

1. Chunking and placement logic live in the library crate `ts2-core`, not in the HTTP handler.
2. Chunkstore verifies the checksum on every write. From v0.2 it also requires a per-request token minted by frontdoor or the index.

Presigned URLs are an API feature in v0.3 (a signed URL that frontdoor accepts). They do not change the topology.

## Consequences

- Frontdoor NIC and CPU are the throughput ceiling for one request path. The fix is more frontdoor instances behind the load balancer.
- One place to enforce limits and emit metrics for every byte.
- Clients cannot bypass admission.

## Alternatives considered

- **Direct-to-cell thick client** for trusted internal bulk workloads, in the style of Ceph librados or HDFS clients. The client chunks and writes to chunkstore nodes itself. Deferred: it needs the tokens above and a client library built on `ts2-core`.
- **Redirect to a chunkstore node.** Rejected: exposes nodes to clients and splits the policy point.
