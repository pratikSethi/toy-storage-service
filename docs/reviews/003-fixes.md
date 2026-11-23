# Review 003 fixes

Fixes for the top five findings in [003-round4-review.md](003-round4-review.md), plus the referenced-chunk `NOT_FOUND` note from its error mapping section. One commit per finding. All of them follow the review's fix; deviations are listed at the end.

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **manifest**: the small record kept for each object. It holds the size and the ordered list of its chunks, with the node that holds each one.
> - **orphan chunk**: a chunk on a node that no manifest points to. A failed upload, an overwrite or a delete can leave one.
> - **garbage collection (GC)**: the background job that deletes orphan chunks once they are older than a minimum age.
> - **rendezvous hashing**: a way to pick nodes for a chunk. Every node gets a score for the chunk id and the highest scores win, so when a node leaves, only its chunks move.
> - **gRPC**: a way for services to call each other over the network (a remote procedure call, RPC), sending Protobuf messages over HTTP/2. The TS2 services talk to each other this way.

## 1. Cleanup after an ambiguous `PutManifest` failure

- Changed: `put_object` (`ts2-core/src/object.rs`) deletes the chunks it wrote only when `put_manifest` fails with `BucketNotFound`, `InvalidBucketName` or `InvalidKey`, which prove nothing was committed. Any other error from `put_manifest` (Timeout, Unavailable, DiskFull, Internal) leaves the chunks for GC. Errors raised before `put_manifest` (node down, object too large) still clean up.
- Commit: `4a831a2`
- Tests (`ts2-core/tests/remote.rs`): `index_commits_then_answers_unavailable_keeps_the_chunks` (fake index commits, then answers UNAVAILABLE, all chunks survive and the manifest is there), `definite_index_rejection_cleans_up_the_chunks`, `index_unavailable_before_commit_leaves_chunks_for_gc` (kept, then removed by a sweep). The old `index_down_after_chunks_written_cleans_up` is gone.

## 3. No eager deletes in remote mode

- Changed: `ChunkStore` gains `deletes_eagerly()` (default `true`). `RemoteChunkStore` returns `false`, so overwrite and `delete_object` leave the old chunks to GC. The in-process store keeps eager deletes because it has no GC.
- Commit: `49eadb3`
- Tests: `overwrite_keeps_the_old_chunks_until_gc` (old version still readable after the overwrite, gone after a sweep with a short min age), `delete_keeps_the_chunks_until_gc`.

## 2. Node id check and health-aware placement

- Changed (`ts2-core/src/remote.rs`, `frontdoor/src/main.rs`): at startup frontdoor calls `NodeInfo` on every node and refuses to start if `node_id` differs from the config id. A node that cannot be reached is retried for 10 s, then logged and left to the refresh. A background task (`TS2_HEALTH_INTERVAL_SECS`, default 5) re-probes every node; an unreachable node, one reporting `healthy=false` and one that now reports another id are left out of placement. A node that answered `RESOURCE_EXHAUSTED` to a put is left out for 60 s. `nodes()` returns only placeable nodes; if none qualifies it returns all of them, because a stale view should not refuse every write. GC uses the new `configured_nodes()` so it still lists every node. Placement itself (id prefix modulo node count) is unchanged.
- Commit: `17d361a`
- Tests: `startup_accepts_nodes_that_report_their_configured_id`, `startup_refuses_a_node_that_reports_another_id`, `startup_tolerates_a_node_that_is_not_up_yet`, `placement_skips_an_unhealthy_node_and_returns_when_it_recovers`, `placement_skips_a_node_that_swapped_ids_after_startup`, `placement_skips_a_node_that_just_ran_out_of_disk`, `placement_falls_back_to_all_nodes_when_none_looks_healthy`, `put_object_writes_every_chunk_to_the_healthy_node` (20 chunks, two nodes, one unhealthy, all land on the other).

## 4. PUT timeout and the orphan cleanup minimum age (`min_age`)

- Changed: `AppState.put_timeout` bounds a whole PUT (`TS2_PUT_TIMEOUT_SECS`, default 900); expiry returns 504. In remote mode frontdoor refuses to start when `TS2_GC_MIN_AGE_SECS` is below twice the timeout. `sweep` adds `CLOCK_SKEW_MARGIN` (60 s) to `min_age` before comparing with the node's `created_unix`. Compose passes `TS2_PUT_TIMEOUT_SECS`. A PUT cancelled by the timeout leaves its chunks to GC, which is why the 2x rule matters.
- Commit: `f451f04`
- Tests: `put_that_never_finishes_times_out_with_504` (`frontdoor/tests/api.rs`, stalled body, nothing committed), `min_age_must_be_twice_the_put_timeout` (`frontdoor/src/gc.rs`), `gc_adds_a_clock_skew_margin_to_min_age`.

## 5. Keepalive, retry, restart

- Changed: every endpoint gets `connect_timeout` 3 s, `tcp_keepalive` 30 s and HTTP/2 keepalive (10 s interval, 5 s timeout, while idle). `retry_once_if_unavailable` repeats a call once after 100 ms on `UNAVAILABLE` for `GetChunk`, `PutChunk` (same id and bytes), `DeleteChunk`, `ListChunks`, `GetBucket`, `ListBuckets`, `GetManifest`, `ListObjects` and `ListChunkRefs`. Not repeated: `PutManifest` (no idempotency key), and also `CreateBucket`, `DeleteBucket` and `DeleteManifest`, because a repeat after a lost response would answer `BucketExists`, `BucketNotFound` or lose the removed chunk list. Compose: `restart: unless-stopped` for `chunk-store` and `object-index`. E2E: `test_survives_chunk_store_restart` uploads, restarts chunk-store, then uploads and downloads again. `TS2_RESTART_CHUNK_STORE` holds the restart command: `make e2e-native` uses `tests/e2e/restart_chunk_store_native.sh`, `make e2e-compose` uses `docker compose restart chunk-store`. Without it the test is skipped.
- Commit: `2d67bf5`
- Tests: `chunk_calls_retry_once_when_unavailable`, `chunk_calls_give_up_after_one_retry`, `index_reads_retry_once_but_put_manifest_does_not` (`ts2-core/tests/remote.rs`), and the e2e restart test.

## Referenced chunk missing on its node

- Changed: `Error::ChunkNotFound` now maps to 500 (detail hidden, logged as an error) instead of 404 `chunk-not-found`, so lost data counts against the error budget. Real object and bucket not-found stay 404. `frontdoor/README.md` updated.
- Commit: `07401e0`
- Test: `backing_service_errors_map_to_their_own_statuses` (`frontdoor/tests/errors.rs`).

## Checks run

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`: pass.
- `make e2e-native`: 12 passed, including the chunk-store restart test, against the locally built C++ services.
- `make e2e-compose`: not run. Docker is running, but `make up` rebuilds gRPC under vcpkg inside the image and did not finish in 25 minutes (memory warning, then a serial rebuild), so the stack was never up. The compose change is `restart: unless-stopped` plus one env var, checked only by reading.

## Nearby cases tested

Wrong id on one of two nodes at startup, node not up yet at startup, node swapping its id after startup, node unhealthy then recovered, all nodes unhealthy, node full then excluded while GC still lists it, retry exhausted after one attempt, PutManifest not retried, commit-then-lost-response, index rejecting with a definite error, GC chunk inside and outside the skew margin, overwrite and delete with a GET still open, stalled PUT body.

## Deviations and limits

- `CreateBucket`, `DeleteBucket` and `DeleteManifest` are not retried, which is narrower than "idempotent calls" read loosely; the review's list does not include them.
- `put_manifest` errors other than the three definite ones leave chunks to GC, which is wider than the review's wording ("Timeout and Unavailable"); an `Internal` error can also hide a commit, for example a decode failure after the index answered.
- A retry after a restart that is still booting can fail with 503. The e2e test waits one second before the second request.
- Placement is still modulo; rendezvous hashing and replicas come in later steps.
