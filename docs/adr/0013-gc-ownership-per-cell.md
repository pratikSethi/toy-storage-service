# ADR 0013: GC ownership per cell

Status: accepted, partly built

## Context

Orphan chunks come from overwrites, failed PUTs and replica stragglers ([ADR 0011](0011-replica-recording-and-repair.md)). With more than one cell, we need to say who sweeps and over what.

## Decision

Each cell's sweep runs in FrontdoorService, against that cell's ObjectIndexService and that cell's nodes. There is one sweeper per cell. The sweep lists chunks the index references, lists chunks on the nodes, and deletes the difference once it is older than the minimum age.

**Today:** built for the one cell: FrontdoorService runs the sweep in remote mode, against that cell's ObjectIndexService and nodes. Not built: more than one cell, and the rule that only one FrontdoorService instance runs the sweep.

## Consequences

- A cell's GC never reads or deletes anything in another cell, which keeps the failure domain from [ADR 0008](0008-what-a-cell-is.md).
- With several FrontdoorService instances, only one may run a cell's sweep at a time. At toy scale that is a config flag. A lease comes later.
- Deletion safety rests on the minimum age. It has to be longer than the longest PUT.

## Alternatives considered

- **ChunkStoreService nodes sweep themselves.** Rejected: a node cannot see the index.
- **ObjectIndexService sweeps.** Rejected: it does not talk to nodes.
- **One global sweeper.** Rejected: it couples cells.
