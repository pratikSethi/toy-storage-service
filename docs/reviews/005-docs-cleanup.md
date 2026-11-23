# Review 005: docs cleanup

The docs were hard to follow for three reasons: they mixed built and planned things, they used short forms before defining them, and some read like instructions to parallel build workers. This page lists what changed, and every planned item that is not built. Code, config, proto, Makefile, compose and CI files were not touched. An ADR is an architecture decision record, a short note on one decision and why. Each claim was checked against the repo (`ls`, `git grep`, `Cargo.toml`, `vcpkg.json`, `compose.yaml`, the code).

## 1. Built versus planned

Commit: `docs: say what is built and what is only planned`.

| File | Change |
|---|---|
| `README.md` | New status line (what runs today). Diagram shows four nodes and no registry. Repo layout lists only directories that exist, with the missing ones under "Planned". Failure drills, Go and replication labelled planned. |
| `docs/design/000-system-overview.md` | Added "built or planned" note. Today diagram for one cell. Registry, cells, router, quotas, auth, load shedding, reload, `If-None-Match`, checksum header, backups, alerts, CDK, k3d, load balancer, extents, scrub labelled planned. Placement text now says rendezvous hashing with one copy of each chunk. PUT writes chunks one after another, not in parallel. Chunk file layout matches the C++ store. The orphan chunk cleanup minimum age default is fixed to 1 hour. |
| `docs/prd.md` | Requirements and milestones say what is built and what is not. Removed the generated client claim. |
| `docs/backlog.md` | v0.2 items that exist marked **Built**. Unbuilt items say so. |
| `docs/plan.md` | Steps 12 and 13 say "Planned, not built". Removed the generated client claim from step 5. |
| `docs/engineering/index.md` | Diagram and text match one cell with four nodes. v0.2 row says what is done and what is not. |
| `docs/engineering/components.md` | Status per component rewritten. New "Planned, not built" section. |
| `docs/engineering/service-levels.md`, `docs/engineering/operations.md` | Say that alert rules, review meetings and the control plane team do not exist yet. |
| `docs/guide/overview.md` | Removed "durable", since each object has one copy. |
| `docs/adr/0001` to `0013` | Status line made accurate ("accepted, built", "accepted, partly built", "accepted, not built yet"). Each has a **Today:** line under the Decision. Decisions were not changed. |
| `frontdoor/README.md`, `chunk-store/README.md` | Removed the in-process BucketRegistry line, the "hash of its bytes" chunk id claim and the old name "gateway". |

## 2. Terms defined before short forms

Commit: `docs: define terms before their short forms`.

- Every page that uses a term such as chunk, manifest, ETag, inline object, cell, failure domain, orphan chunk, garbage collection (GC), R, W, quorum, rendezvous hashing, compare-and-swap, RPC, gRPC, OTel, OTLP, fsync, SQLite, SLI, SLO, SLA, p99 or ADR now has a "Terms used here" block under its title. It defines only the terms that page uses. Files: `README.md` (inline definitions instead of a block), `chunk-store/README.md`, `cli/README.md`, `deploy/compose/README.md`, `frontdoor/README.md`, `object-index/README.md`, `proto/README.md`, `ts2-core/README.md`, `docs/adr/0001` to `0013`, `docs/backlog.md`, `docs/cli/index.md`, `docs/demo.md`, `docs/design/000-system-overview.md`, `docs/engineering/{components,debugging,frontdoor-stack,index,operations,service-levels}.md`, `docs/guide/{concepts,service-levels}.md`, `docs/plan.md`, `docs/ports.md`, `docs/prd.md`, `docs/reviews/001` to `004`, `docs/setup.md`.
- Headings renamed so none has an undefined short form. File names are unchanged, so no links broke.

| Old heading | New heading |
|---|---|
| ADR 0010: Replication R and W, and what 201 means | ADR 0010: Copies per chunk, and what a 201 reply means |
| ADR 0013: GC ownership per cell | ADR 0013: Orphan chunk cleanup runs per cell |
| Durability SLIs (`docs/engineering/service-levels.md`) | Durability indicators |
| ETag (`docs/guide/concepts.md`) | ETag (content fingerprint) |
| 4. PUT timeout and GC `min_age` (`docs/reviews/003-fixes.md`) | 4. PUT timeout and the orphan cleanup minimum age (`min_age`) |

`mkdocs.yml` nav titles for ADR 0010, ADR 0013 and review 002 changed to match, and reviews 003 (fixes), 004 and 005 were added to the Reviews nav.

## 3. Build-orchestration wording removed

Commit: `docs: replace build orchestration wording with plain history`, which also adds this page and its nav entry.

| File | Change |
|---|---|
| `docs/plan.md` | Steps 8 to 11b rewritten as plain history, without "Round 4 (a)", "step 0 owner", "workers (a) and (b)" or "forks". Commit hashes kept, and hashes added for steps 8 and 11. |
| `proto/README.md` | Replaced the step 0 owner rule with "Change `proto/` in its own commit, then update the C++ and Rust sides". |
| `docs/reviews/002-round3-review.md` | "Round 3" and "round 4" become steps 7 and 8 to 11. "(a)", "(b)", "(c)" become steps 9, 10 and 11. "Workers" and "owner" removed. Findings unchanged. |
| `docs/reviews/003-fixes.md` | "The next round" became "later steps". |
| `docs/reviews/003-round4-review.md` | Orphan wording fixed (see below). Left uncommitted. |
| `docs/reviews/004-placement.md` | Unchanged. The table quotes the real commit subject `plan: mark round 5 placement and docker speed`, which cannot be edited. |

`docs/reviews/003-round4-review.md` is the owner's uncommitted file. Only its orchestration wording was fixed (title, a heading, "owner" and "worker" phrases). It is not staged or committed, and it is not in the nav. The plan links to it, so it needs committing before the docs are published.

## Planned, not built

Where each item is mentioned. "Overview" is `docs/design/000-system-overview.md`.

| Item | Mentioned in |
|---|---|
| BucketRegistryService and `BucketRegistry` module (no `bucket-registry/`, no Go code) | `README.md`, overview, `docs/prd.md`, `docs/backlog.md`, `docs/engineering/{index,components,service-levels}.md`, ADR 0002, 0004, 0007, 0009 |
| Go | `README.md`, overview, `docs/prd.md`, `docs/backlog.md`, ADR 0002, 0007 |
| Web UI (TypeScript, no `ui/`) | `README.md`, overview, `docs/engineering/components.md`, ADR 0002 |
| RocksDB index | overview, `docs/prd.md`, `docs/backlog.md`, `docs/engineering/index.md`, ADR 0002 |
| Client generated by openapi-python-client (CLI uses plain `httpx`) | ADR 0001, ADR 0002, `docs/engineering/components.md` (all say it is not used) |
| AWS CDK deploy (no `deploy/cdk/`), k3d and Helm, load balancer, buildx, GHCR, ECR | `README.md`, overview, `docs/backlog.md`, `docs/engineering/components.md` |
| More than one cell, bucket-to-cell map, cross-cell name check, ListBuckets fan-out | `README.md`, overview, ADR 0004, 0008, 0009, 0013, `docs/engineering/index.md`, `docs/prd.md`, `docs/plan.md` |
| Replication (three copies of each chunk), acknowledging a PUT after two of three copies are durable, stragglers (copies that finish late) | `README.md`, overview, ADR 0010, 0011, `docs/backlog.md`, `docs/prd.md`, `docs/engineering/{index,components}.md`, `docs/plan.md` |
| Repair job, scrub, under-replicated chunk metrics | ADR 0010, 0011, overview, `docs/prd.md`, `docs/backlog.md`, `docs/engineering/{index,service-levels}.md` |
| Version check (compare-and-swap) in `PutManifest` | ADR 0011 (reserved in `object_index.proto`) |
| Erasure coding, extent-based storage | `README.md`, overview, `docs/prd.md`, `docs/backlog.md`, `docs/engineering/{index,components}.md` |
| Failure drills | `README.md`, `docs/prd.md`, `docs/backlog.md`, `docs/engineering/index.md`, `docs/plan.md` |
| API key auth, per-request chunk token (check exists in ChunkStoreService, off by default) | `README.md`, overview, ADR 0006, `docs/prd.md`, `docs/backlog.md`, `docs/engineering/{index,components}.md`, `docs/plan.md`, `frontdoor/README.md` |
| Quotas and `GET /v1/quotas`, rate limits, load shedding (429, 503 with `Retry-After`) | overview, `docs/prd.md`, `docs/backlog.md`, `docs/guide/{capabilities,limits}.md`, `frontdoor/README.md` |
| Multipart and resumable upload, presigned URLs, object versioning, `If-Match`, `If-None-Match` | overview, `docs/prd.md`, `docs/backlog.md`, `docs/guide/capabilities.md` |
| Client checksum header `x-ts2-checksum-sha256` | overview, `docs/prd.md`, `docs/backlog.md` |
| Config reload (SIGHUP or file watch), `flags` block, config service | ADR 0005, overview, `docs/prd.md`, `docs/backlog.md`, `docs/engineering/{index,components}.md`, `frontdoor/README.md` |
| Jobs API (`/v1/jobs`), Temporal | overview, `docs/prd.md`, `docs/backlog.md` |
| Index backups and restore drill | `README.md`, overview, `docs/prd.md`, `docs/backlog.md`, `docs/plan.md`, `docs/engineering/components.md` |
| Alert rules, OTel metrics from the C++ services, request id in C++ logs, traces | overview, `docs/prd.md`, `docs/engineering/{components,operations,service-levels}.md` |
| Full-disk usage threshold, header, idle and minimum-rate timeouts, TLS | overview, `docs/prd.md`, `docs/plan.md` |
| `FileSystem` abstraction, `O_DIRECT` and io_uring | overview |
| Node join and leave, rebalance, rolling upgrade, S3 cold tier | overview, `docs/prd.md`, `docs/backlog.md`, `docs/engineering/index.md` |
| Only one FrontdoorService instance runs the cleanup sweep (lease) | ADR 0013 |
| GitHub Pages publishing | `docs/backlog.md`, `docs/plan.md` |
| Per-bucket region and availability zone, multi-region heat map data (synthetic) | `docs/guide/concepts.md`, `docs/engineering/operations.md`, `docs/backlog.md` |

## Not changed

Owner credits (the README author line, the operator line in `docs/prd.md`, the `pratiksethi.dev` link in `docs/backlog.md`), `.vscode/launch.json` and the code. Commit messages in git history still say "round 4 (c)" because history is not rewritten. Smaller short forms such as CRUD, REST, TLS and NIC are explained in a few places and not everywhere.
