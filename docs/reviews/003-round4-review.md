# Review 003: steps 9 to 11 and the plan for what follows

Scope: commits `83ad65a..e19d123`. Read-only review of `chunk-store/`, `object-index/`, `ts2-core/src/{remote,gc,object}.rs`, `frontdoor/`, `deploy/compose/compose.yaml`, against `docs/backlog.md`, reviews 001 and 002 and `docs/engineering/service-levels.md`. `cargo test --offline` was run and passes; no C++ build or Docker run was done, so nothing below was observed on a live stack. Where a claim is an inference from reading, it says so.

## Top 5 findings

### 1. A cleanup after an ambiguous `PutManifest` failure can delete chunks of a committed object

`ts2-core/src/object.rs:86-90` deletes every written chunk whenever `put_object` returns `Err`. `put_manifest` at `object.rs:129` now goes over gRPC and can fail with `Timeout` or `Unavailable` after the index already committed (response lost, index restarted after commit, client deadline of 5 s hit while SQLite fsyncs). The manifest then points at chunks that no longer exist, and the client saw a 504 or 503. This is the one path that turns a retryable error into data loss. The test `index_down_after_chunks_written_cleans_up` (`ts2-core/tests/remote.rs:400`) locks in the cleanup and cannot tell the two cases apart.

Fix: in `put_object`, delete chunks only when the error is definite (`BucketNotFound`, `InvalidKey`, `ObjectTooLarge`, errors raised before `put_manifest`). For `Timeout` and `Unavailable` from `put_manifest`, leave the chunks to GC (the 1 h min age protects them) and return the error. Add a test where the fake index commits and then returns `UNAVAILABLE`, and assert the chunks survive.

### 2. Placement ignores node health, and the node id in config is never checked against the node

`object.rs:43-45` picks `nodes[hash mod N]` and `object.rs:52` writes sequentially. With 3 nodes and one down, an object of k chunks fails with probability 1-(2/3)^k, so most multi-chunk PUTs fail while two nodes are healthy. `ChunkStoreService.NodeInfo` returns `healthy`, `node_id`, `cell` and free space, but nothing in `ts2-core` or `frontdoor/src/main.rs:49-57` calls it. A swapped address in `TS2_CHUNK_NODES` (`node-1=...` pointing at node 2's host) is accepted silently and every manifest then records the wrong node id.

Fix: at startup call `NodeInfo` on each node and refuse to start if the returned `node_id` differs from the config id. Refresh health in the background and make `place()` skip nodes that are unhealthy or that returned `RESOURCE_EXHAUSTED` recently. Decide this before the 3-node round, see "Next round".

### 3. Overwrite and delete remove chunks that a running GET still needs

`object.rs:146-150` (overwrite) and `object.rs:200-202` (delete) delete the old chunks at once. `object_stream` (`object.rs:178`) fetches chunk by chunk, so a GET that started before the overwrite gets `ChunkNotFound` partway through. The status line and headers are already sent, so the client sees a truncated body (inferred from the stream design; `handlers.rs` not traced in detail). The comment at `object.rs:144` knows about it. Eager deletion was added to save disk, and orphan GC already exists.

Fix: drop the eager deletes in remote mode and let GC remove them after `min_age`. Keep them for the in-process backend only if it matters. Update `overwrite_deletes_the_old_chunks_eagerly` (`remote.rs:365`) to assert the old chunks are still readable right after the overwrite and gone after a sweep with a short min age.

### 4. GC `min_age` is not tied to the longest PUT, and it mixes two clocks

`ts2-core/src/gc.rs:27-34` compares a node's file mtime (seconds, node clock) with frontdoor's clock. A chunk is aged from when it was written, not from when its PUT started or will commit. A PUT that writes its first chunk and commits more than `min_age` later loses that chunk to a sweep and then commits a manifest with a missing chunk. With the default 3600 s this needs a very slow client, but nothing enforces a bound: `max_object_bytes` is a size limit, not a time limit, and `compose.yaml:19` exposes `TS2_GC_MIN_AGE_SECS` with no floor.

Fix: add a total request timeout on PUT in frontdoor, and refuse to start when `min_age < 2 x` that timeout (or clamp it). Use `created_unix` only as a lower bound and add a margin for clock skew. In compose the nodes and frontdoor share a host clock, so this only bites on real hosts.

### 5. No retry, no keepalive, and no test that a service restarts mid-request

`remote.rs:84-87` uses `connect_lazy` with no HTTP/2 keepalive, no connect timeout and no retry anywhere. After a C++ service restarts, the first call on the stale connection fails with `UNAVAILABLE` and becomes a 503 even though the service is back (inferred from tonic's behaviour; not reproduced). `compose.yaml` has no `restart:` policy for `chunk-store` or `object-index`, and `depends_on` only orders startup. No test in `remote.rs` or the e2e targets kills and restarts a service. The three fake-server tests (`remote.rs:386-425`) drop a server for good, which is a different case.

Fix: set `http2_keep_alive_interval`, `connect_timeout` and `tcp_keepalive` on the `Endpoint` (`remote.rs:85`). Retry once on `UNAVAILABLE` for idempotent calls only (`GetChunk`, `DeleteChunk`, `ListChunks`, `GetManifest`, `List*`, and `PutChunk`, which is idempotent for the same id and sha256). Do not retry `PutManifest` without an idempotency key (see finding 1). Add `restart: unless-stopped`, and an e2e step that does `docker compose restart chunk-store` between two requests.

## Correctness of steps 9 to 11

### PUT ordering

Chunks are written and acknowledged one at a time (`object.rs:43-58`), then the manifest is committed (`object.rs:129`), then 201 is returned. The chunk node replies only after `commit()` returns, which includes both fsyncs and the directory fsync (`disk_store.cpp:228-247`). So "chunks durable before manifest" holds in the happy path. The break is the failure path in finding 1. The inline path (no chunks) skips this entirely and is correct.

The index replies after `COMMIT` with `synchronous = FULL` and `fullfsync` (`sqlite_index.cpp:271-276`), so "201 after commit" holds.

### Sidecar, rename and fsync ordering

`disk_store.cpp:225-247` fsyncs the data file, fsyncs the sidecar, creates the fan-out directory (and fsyncs the root if it was new), renames the sidecar, renames the data file, then fsyncs the chunk directory. That ordering is sound: a crash leaves a sidecar without data (ignored on read) and never data without a sidecar. The test at `disk_store_test.cpp:363` pins the order.

Gaps:

- A sidecar left without data (crash between the two renames) is never listed (`disk_store.cpp:384` skips `.sha256` names), never deleted by `remove`, and not cleaned at `open`. It leaks, small but permanent. Fix: have `open()` or `list()` report and remove sidecars with no data file, or let `remove()` and a scrub pass handle them.
- The sidecar holds only the full-chunk sha256, so a flipped bit in the sidecar itself reads as `DATA_LOSS` on a good chunk. Acceptable for now; scrub should repair from a replica.
- `begin_put` free-space check (`disk_store.cpp:305-310`) is per request, so concurrent PUTs can all pass it and fill the disk. The failure surfaces as `ENOSPC` mapped to `RESOURCE_EXHAUSTED`, so it is handled, just late.
- `remove()` deletes the sidecar first (`disk_store.cpp:346`), so a crash leaves data without a sidecar, which reads as corrupt rather than missing. A GET on a half-deleted chunk gives 500 instead of 404. Only matters for chunks that are still referenced, which GC does not delete.

### GC races

`gc.rs:20-26` lists chunks before refs, which is the right order and is tested (`remote.rs:506`). The `min_age` rule covers uncommitted PUTs, subject to finding 4. Other points:

- The sweep holds all listed chunks and all refs in memory (`gc.rs:20`, `remote.rs:196`). Known and documented in `plan.md`. Fine at toy scale.
- Several frontdoor replicas would each sweep. Deletes are idempotent, so this is wasteful rather than wrong. Decide which instance runs the sweep before a second frontdoor exists.
- `list_chunk_refs` pages across separate SQLite reads with no snapshot. Safe for GC because of the list order above, but note it for anyone who reuses it for audit.
- The sweep ignores `corrupt` entries from the node. A corrupt unreferenced chunk is deleted like any other, which is right. A corrupt referenced chunk is never reported anywhere. That belongs to scrub.

### Error mapping

`remote.rs:60-76` and `frontdoor/src/error.rs:79-102` give `UNAVAILABLE` 503, `DEADLINE_EXCEEDED` and the client timeout 504, `RESOURCE_EXHAUSTED` 507, `NOT_FOUND` from a chunk node 404. This matches the service-levels doc where 5xx counts as bad. Notes:

- `Code::ResourceExhausted` maps to `DiskFull` for the index too. If the index ever returns it for a different reason (too many requests, message too large), the client sees 507. Make the mapping depend on `Source`.
- A chunk node's `NOT_FOUND` on a referenced chunk is a lost chunk, not a missing object. It returns 404 `chunk-not-found`, which a client reads as "object missing". The SLO counts 4xx as good, so lost data would not burn error budget. Return 500 (or 502) for a referenced chunk that is missing and keep 404 for the real not-found.
- `INVALID_ARGUMENT` from the index always becomes `InvalidKey` (`remote.rs:73`), including bucket-name errors from the C++ validator. Check the response text for a bucket-name request.
- `DATA_LOSS` becomes 500 `internal`, with the detail hidden. Correct for the client, but make sure a metric counts it (the durability section of service-levels lists scrub failures as planned only).

### Deadlines and frame limits

Deadlines are 30 s, 10 s and 5 s, applied client-side with `tokio::time::timeout` (`remote.rs:48`). They are not sent as `grpc-timeout`, so a C++ handler keeps working after the client gives up. For `PutChunk` the server notices the cancel and removes its tmp files (`service.cpp:68`, tested at `service_test.cpp:345`). For the others it only wastes work. Prefer `Request::set_timeout` so both sides agree.

Frame limits are consistent: the client sends 1 MiB frames (`remote.rs:408`), the server rejects larger ones (`service.cpp:63`), and `SetMaxReceiveMessageSize` is 1 MiB plus 1 KiB (`service.h:15`). The tonic default decode limit of 4 MiB is enough for 1 MiB frames, but a manifest with thousands of chunks in `PutManifest` or `ListObjects` is bounded only by object size. At `chunk_bytes` of 4 MiB and a 5 GiB maximum, about 1,300 chunk refs fit comfortably.

A GET holds a whole chunk in memory on the node (`disk_store.cpp:324-332`) and again in frontdoor (`remote.rs:432-437`). Fine for 4 MiB chunks. It is also why the 10 s `GetChunk` deadline is generous.

### A service restarting mid-request

- Chunk node restarts during `PutChunk`: the client gets `UNAVAILABLE`, frontdoor deletes the chunks it wrote earlier in this PUT (correct), and the node clears `tmp/` at start (`disk_store.cpp:271-277`). Good.
- Chunk node restarts during a GET: 503 before the first byte, or a truncated body after it.
- Index restarts around `PutManifest`: finding 1.
- Frontdoor restarts mid-PUT: chunks already written are orphans. GC removes them after `min_age`. Correct.

### Tests that pass while the behaviour is wrong

- `index_down_after_chunks_written_cleans_up` and `node_down_mid_upload_is_unavailable_and_cleans_up` (`remote.rs:386-410`): the first asserts the unsafe cleanup in finding 1.
- `overwrite_deletes_the_old_chunks_eagerly` (`remote.rs:365`): asserts the behaviour in finding 3.
- The GC tests use fake servers that return `created_unix` directly (`remote.rs:473`). Nothing checks that the real node reports the write time. The disk-store test covers the mtime on the C++ side, but no test joins the two. The e2e run sets no `TS2_GC_MIN_AGE_SECS` to a small value, so a real sweep against real nodes is never exercised.
- The "drift" tests are separate: Rust in-process and C++ stores are each tested against their own expectations, not against one shared vector set. `tests/vectors/` does not exist yet (see `plan.md`).

### Drift between the Rust in-process and C++ implementations

- Chunk store: Rust has one file per chunk under a two-level fan-out (`chunk_store.rs:11`), no sidecar, and checksums checked by the caller (`object.rs:178`). C++ has a one-level fan-out, a sidecar and a node-side checksum. Different on-disk layouts are fine, but behaviour differs: the in-process store has no `list`, no `corrupt` flag, and ignores the node id, so GC and node-aware tests cannot run in-process.
- Index: the C++ index validates bucket names and keys (`sqlite_index.cpp:222-250`); the Rust one does not, and frontdoor validates before calling. In remote mode the validation runs twice with two implementations. Keep one rule set and put it in shared test vectors.
- Listing: Rust uses `substr(...) = prefix` (`object_index.rs:232`), C++ uses a computed prefix successor (`sqlite_index.cpp:457-463`) with BINARY collation. Same intent, different code. Non-ASCII keys and keys containing `%` or `_` should be in a shared vector.
- The Rust index puts the version in a manifest it returns, the C++ one assigns it in `put_manifest`. A test that overwrites twice and compares versions across both would catch drift.

## Next round plan review

Plan from the request: three ChunkStoreService nodes in compose, deterministic placement owned by `ts2-core`, a second cell with bucket-to-cell routing, replication with a W-of-R ack.

### Wrong or shaky premises

1. **"Placement over 3 nodes" and "R=3" cannot both be tested together.** With three nodes and R=3 every chunk lands on every node, so placement does nothing. Placement only shows up with more nodes than R, or with R=1. The plan needs 4 or 5 nodes in a cell (or R=2) to exercise it.
2. **`hash(chunk_id) mod N`** (`backlog.md` v0.2) does not fit replication. The chunk id is a random UUID (`object.rs:43`), so hashing adds nothing, and `mod N` changes every placement when N changes. It is safe today only because manifests record the node. For R distinct nodes use rendezvous (highest random weight) hashing over the healthy node set, and treat the manifest as the source of truth for reads.
3. **A manifest holds one node per chunk in Rust.** The proto has `repeated node_ids` (`remote.rs:121`), but `ChunkRef.node_id` is a single string and `manifest_from_pb` keeps only the first (`remote.rs:134-138`). Replication first needs `node_ids: Vec<String>` in `ts2-core`, and then the read path, delete, GC and `place()` all change. This is the largest piece of the round and the plan does not list it.
4. **"Second cell with bucket-to-cell routing" needs a registry that does not exist.** The backlog puts the BucketRegistryService in v0.4 as a Go service. Without it, routing has to live in frontdoor config or in the index. A bucket name is globally unique, ListBuckets spans cells, and CreateBucket must pick a cell and be race-free. Each of those is a decision, not an implementation detail.
5. **A cell is not defined in code.** `NodeInfo` has `cell` and `failure_domain`, and nothing reads them. Is a cell its own index plus its own set of chunk nodes plus its own frontdoor, or a group of nodes behind one shared index? The SLO doc draws availability per cell, which implies the first. Everything else depends on this.
6. **Step 12 in `plan.md` bundles API key auth with placement.** They share no code. Split it.
7. **The index is still one SQLite file.** The backlog says so (v0.4 "single point of loss"). A second cell doubles the number of single points. R=3 chunks do not protect the manifests.
8. **W=2 of R=3 needs a repair path, which is not planned.** Otherwise the third replica is missing forever. The backlog puts scrub and repair at v0.5, after the ack policy at v0.4.

### Items that touch the same files

- Placement (ts2-core), replication (ts2-core and frontdoor) and the trait change to `node_ids` touch the same files (`object.rs`, `remote.rs`, `chunk_store.rs`, `model.rs`). Build them one after another, in this order.
- Routing needs a client for the registry and a cell concept in config. That touches `frontdoor/main.rs`, `handlers.rs` and `lib.rs`, the same files as health-aware placement. Keep it in one change series.
- The ack policy changes the PUT result and the error mapping (what is the status when W of R succeed but one fails?), which is `frontdoor/src/error.rs` plus the SLO doc. Change code and doc together.
- Compose, e2e and the Dockerfiles change in every item. A shared compose file means each change has to be rebased on the previous one, so build them in order.
- C++ side: nothing in `chunk-store/` has to change for placement or W-of-R. A node does not know about replicas. If someone proposes node-to-node replication or a write token, that is a different item, with its own proto change.

### Order I would do it in

1. **Decisions (below), written as ADRs.** No code.
2. **Findings 1 to 5 above.** They are small and every later item relies on them, especially 1, 2 and 5.
3. **`node_ids: Vec` in `ts2-core`, health-aware rendezvous placement, three nodes in compose, R=1.** Needs 4 nodes to show anything useful; use 4. Reads fall back across `node_ids`, so the read path is ready for replicas.
4. **R=3 and W (parallel writes, ack at W, straggler handling, delete on all replicas, read fallback on `UNAVAILABLE`, `NOT_FOUND`, `DATA_LOSS`).** Add failure drills: kill a node during PUT, kill a node during GET, corrupt a replica.
5. **Under-replicated detection and a repair job** (the first slice of scrub). W=2 is not acceptable without it. W=3 can wait.
6. **Second cell and routing.** Last, because it needs the cell definition, the registry decision and the index backup story, and it does not change the chunk path.
7. API key auth, any time, separately.

### What must be decided first

1. **What a cell is** (index plus nodes plus frontdoor, or less). Write it in ADR form with the failure domain it is meant to contain.
2. **Where the bucket-to-cell map lives and who writes it**: frontdoor config (static, fast, no new service), the first cell's index, or the Go BucketRegistryService. CreateBucket and ListBuckets semantics follow from this.
3. **W and R and the meaning of 201.** With W < R, state that an acknowledged object can be under-replicated for a while and say what the SLO and the durability model assume. With W = R = 3, state what a single down node does to PUT availability (it makes PUT fail unless placement can pick another node; so R=3 of 3 nodes means one node down stops all writes). That alone is a reason to run four nodes.
4. **Who deletes the third replica when W is reached early**: the PUT path (cancel the straggler and delete it), or GC (it is unreferenced, so GC removes it after `min_age`). The manifest must list only acknowledged replicas, or every replica; pick one.
5. **How the manifest records replicas and how a repair changes it.** Repair is a manifest update, so `PutManifest` needs a compare-and-swap or version check, or repair can race with an overwrite and resurrect old chunks.
6. **Node ids are permanent.** They are written into every manifest, and the config id is never checked against `NodeInfo` (finding 2). Decide that the node reports its id, frontdoor verifies it at startup, and the id never changes.
7. **Which service owns GC when there are two cells.** Each cell's frontdoor sweeping its own index and nodes is the simple answer; say so.

## What was not checked

No Docker or C++ build, no sanitizer run, no live restart test. Findings 3 and 5 rest on reading the stream and tonic behaviour, not on a reproduction. `frontdoor/src/handlers.rs` was skimmed for the GET path, not traced.
