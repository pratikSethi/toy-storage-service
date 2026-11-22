# 004: Placement over four nodes, Docker build speed

## What changed

- **Placement** (`ts2-core/src/placement.rs`). Rendezvous hashing: each placeable node scores `sha256(node_id ":" chunk_id)` and the chunk goes to the highest score. `rendezvous(chunk_id, nodes, r)` returns an ordered list of up to `r` nodes. `put_object` uses `r = 1`. Unhealthy nodes are skipped because `ChunkStore::nodes()` already lists only placeable ones. Node ids still come from `TS2_CHUNK_NODES` (`id=addr,...`, format unchanged). Existing manifests record their node, so reads are unaffected; only new chunks use the new function.
- **Docker** (both C++ Dockerfiles). The vcpkg install step uses `--mount=type=cache,target=/root/.cache/vcpkg`. The two Dockerfiles are identical up to that step, so the second image reuses the first one's layer. Chosen over a shared base image because it needs no extra image and no build ordering. `make up` and `make up-obs` now build the images one after another (`make images`). Explained in `docs/setup.md`.
- **Compose**: `chunk-store-1` to `chunk-store-4`, ids `node-1` to `node-4`, ports 50061 to 50064 (container only), volumes `chunk-data-1` to `-4`, one shared image `ts2-chunk-store`. The old `chunk-data` volume is no longer used.
- **Native e2e**: `make e2e-native` starts four chunk nodes (50151, 50153 to 50155) plus the index (50152). Ports are in `docs/ports.md`.
- **e2e test** `test_big_object_spreads_over_nodes`: uploads 40 MiB (10 chunks), checks the chunk count grew by 10 in total and on more than one node, downloads and compares bytes, deletes.

## Commits

| Commit | Message |
|---|---|
| bf242ab | docker: cache vcpkg binaries across C++ image builds |
| 26bc950 | ts2-core: rendezvous placement returning an ordered node list |
| 77df019 | compose: four chunk-store nodes with their own volumes and ports |
| 08b200e | e2e: 40 MiB object spreads over four chunk nodes |
| edcfac1 | plan: mark round 5 placement and docker speed |

## Checks run

| Check | Result |
|---|---|
| `cargo test --workspace` | pass (4 new placement tests: even spread over 4 nodes within +-20%, removing a node moves only its chunks, a node left out gets nothing, ordered list with R=3) |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo fmt --check` | pass |
| `make e2e-native` | 13 passed, 4 native nodes |
| `make up` then `make e2e-compose` | 13 passed against the 4-node compose stack, all six containers healthy |
| `docker compose config -q` | pass |

The cold Docker build ran once, in the background, `make images` with output in `target/make-images.log`. The first `vcpkg install` restarted without parallelism, and the whole cold build took about 60 minutes. Docker Desktop has 7.7 GiB, which is probably why. The object-index image then reused the chunk-store image's cached vcpkg layer (the `CACHED` lines in the log).

## Not verified

- A rebuild after a **source change** was not timed. The layer ordering says gRPC will not recompile, and the object-index build showed the layer reuse, but I did not edit a source file and rebuild.
- The BuildKit **cache mount** itself was never exercised: it only matters when `vcpkg.json` changes, and I did not change it after the cold build.
- The new test checks the spread by counting chunk files on each node (`tests/e2e/count_chunks.sh`), not through the manifest. See deviations.
- Stopping one node and reading with a node down was not run. R=1, so objects with a chunk on the stopped node would fail by design.
- CI (`.github/workflows`) was not run and was not changed; it does not know about four nodes.
- `make up-obs` was not run.

## Deviations

- **Manifest check.** Frontdoor has no endpoint that returns a manifest (`ts2 stat` is in the backlog), so the test counts chunk files per node instead. A `TS2_COUNT_CHUNKS` command supplies the counts; without it the test skips.
- The Docker Hub pull failed with EOF early on and worked on a later retry, so e2e-compose did run.
- The restart e2e test now restarts `chunk-store-1` (node-1) in both modes.
- `docs/plan.md` step 12 lost its placement part, which is done; it now reads API key auth only.
- The vcpkg layer cache was already shared by the two Dockerfiles before this change. The first compose build most likely ran both C++ cold builds in parallel; `make images` now serialises them.
