# ADR 0007: Multi-word service names

Status: accepted, built

## Context

Early docs used one-word names: gateway, registry, index, chunkstore. Words like "index" and "registry" are also ordinary nouns (a database index, a container registry), so a sentence such as "the index stores it" was ambiguous. Diagrams also did not say what each box was written in.

## Decision

Each service has a multi-word name that ends in `Service`, and every diagram and component list states its language.

| Service | Language | Directory |
|---|---|---|
| FrontdoorService | Rust | `frontdoor/` (binary `ts2-frontdoor`) |
| BucketRegistryService | Go from v0.4 | `bucket-registry/` |
| ObjectIndexService | C++ | `object-index/` |
| ChunkStoreService | C++ | `chunk-store/` |

Until v0.4 bucket registry logic is an in-process module of FrontdoorService called `BucketRegistry`. `ts2-core` is a Rust library, not a service. It holds the `ObjectIndex` and `ChunkStore` traits that FrontdoorService calls in process in v0.1 and over gRPC later. The trait names stay short because they are code, not services.

"ShardStore" is not used. It is the name of S3's own storage node software.

**Today:** BucketRegistryService and the in-process `BucketRegistry` module do not exist. The table row for it describes a planned service.

## Consequences

- Prose and diagrams are unambiguous about which thing is meant.
- Names are longer. Binaries and directories use the kebab-case form.
- Old names (gateway, registry, index, meta service, MetaStore, chunkstore, storage node) must not appear in new docs.

## Alternatives considered

- **Keep one-word names.** Rejected: ambiguous in prose.
- **Short acronyms (FDS, OIS).** Rejected: harder to read for a newcomer.
