# Review 002: the proto and C++ scaffold (step 7) and the plan for steps 8 to 11

Read-only review of commits `0f8703f^..HEAD` (proto, C++ scaffold, CI, operation label, dashboard, ports) against `docs/plan.md`, the overview, the ADRs and review 001. Paths are relative to the repo root. One check was run: `protoc` from `chunk-store/build/debug/vcpkg_installed` on both protos. Nothing was built, edited or committed.

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **manifest**: the small record kept for each object. It holds the size and the ordered list of its chunks, with the node that holds each one.
> - **inline object**: an object under 64 KiB. It is stored inside its manifest instead of as chunks.
> - **orphan chunk**: a chunk on a node that no manifest points to. A failed upload, an overwrite or a delete can leave one.
> - **garbage collection (GC)**: the background job that deletes orphan chunks once they are older than a minimum age.
> - **R**: the number of copies of each chunk. R=3 means three copies. Only R=1 (one copy) is built.
> - **gRPC**: a way for services to call each other over the network (a remote procedure call, RPC), sending Protobuf messages over HTTP/2. The TS2 services talk to each other this way.
> - **fsync**: a system call that forces written data onto the disk before it returns, so a crash cannot lose it.
> - **SQLite**: a small database that lives in one file.
> - **p99**: the value that 99 of 100 requests beat. A p99 latency of 100 ms means 99 requests in 100 finish within 100 ms.
> - **ADR** (architecture decision record): a short note on one decision and why it was made.

## Top 5 findings

1. **The two protos cannot be compiled together.** `proto/ts2/v1/object_index.proto:118-125` redefines `NodeInfoRequest` and `NodeInfoResponse`, which already exist in package `ts2.v1` at `chunk_store.proto:107-117`. Running `protoc -I proto proto/ts2/v1/chunk_store.proto proto/ts2/v1/object_index.proto` fails with `"ts2.v1.NodeInfoRequest" is already defined in file "ts2/v1/chunk_store.proto"`. The C++ scaffold builds only because each CMake project compiles one proto. Step 11 (tonic, one `build.rs` or crate for both) fails on the first build. Fix before steps 9 and 10 start: rename to `ChunkStoreNodeInfo*` and `ObjectIndexNodeInfo*` (or move to a shared `node.proto`, which makes both services return the same message).
2. **Nothing defines where a chunk's checksum is stored, and the on-disk format is not shared with the Rust store.** `chunk_store.proto:7` says the sha256 is "stored beside the id and verified on read", and `GetChunk` (`:24-25`) re-verifies. The existing `LocalChunkStore` (`ts2-core/src/chunk_store.rs:8,53-66`) stores raw bytes only, with no checksum. Step 9 must pick one format before it starts: a sidecar `<id>.sha256` breaks the single-rename atomicity, while a footer or header breaks byte-compatibility with the Rust data dir. Recommendation: write data and sidecar to `tmp/`, fsync both, rename the sidecar first and the data file last, fsync the dir. A data file without a sidecar is treated as corrupt (and reported by `ListChunks`/startup scan). State that C++ and Rust `data/` dirs are not interchangeable.
3. **Orphan GC has no owner and no data path.** `chunk_store.proto:37-38` says GC is "owned by ObjectIndexService", and `plan.md` item 9 puts it in `object-index/`. But the index has no RPC that returns the set of referenced chunk ids (`ListObjects` returns `ObjectSummary`, which has no chunk ids; `object_index.proto:79-84`). It also has no client and no address list for the chunk nodes. Review 001 and ADR 0003 put placement in FrontdoorService, so the Rust side is the only process that knows both. Either add `ListChunkRefs(page)` to ObjectIndexService and make GC a frontdoor background task (recommended: reuses the tonic clients from item (c), no C++ gRPC client in the index), or specify the index as a gRPC client of every chunk node. The ordering rule matters either way: list chunks first, then list manifests, then delete only chunks older than the minimum age (`ChunkInfo.created_unix`, `chunk_store.proto:103`). The reverse order deletes the chunk of an upload that commits between the two lists.
4. **Steps 9 and 10 are not independent of unfinished contract work, and step 11 cannot start from the plan as written.** The shared inputs are `proto/`, `.github/workflows/ci.yml`, `docs/ports.md` and `.dockerignore`. Also, `plan.md` item 9 lists only manifest RPCs, but `ts2-core/src/lib.rs:20-32` (`ObjectIndex`) needs all of `CreateBucket`, `GetBucket`, `DeleteBucket`, `ListBuckets`. Step 11 swaps the trait impl, so step 10 must implement the bucket RPCs too (`object_index.proto:16-26`), with the not-empty check atomic with the delete. Add a step before them (one commit series): fix finding 1, fix the contract gaps below, add Rust proto codegen (`protoc` in the Rust CI job and in `frontdoor/Dockerfile`), and fix the Docker context (finding 5). Then build steps 9 and 10.
5. **Docker and CI build gRPC from source several times, and the context is 3.5 GB.** `.dockerignore` lists `target/ site/ .venv/ .git/ data/` but not `chunk-store/build` or `object-index/build`. Both are 3.5 GB (`du`), and both Dockerfiles run `COPY chunk-store chunk-store` after `COPY proto proto`, so the host `build/debug` tree, including `vcpkg_installed`, is sent to the daemon and copied into the layer. Add `**/build/` and `**/vcpkg_installed/`. Separately, each service has its own `vcpkg.json`, so CI, `chunk-store/Dockerfile:14` and `object-index/Dockerfile:14` each build grpc, protobuf and abseil cold, four builds in all for one set of dependencies. Details in the CI section.

## Proto contracts

Streaming shape and chunk size. `PutChunk` is client-streaming with a header then `data` frames (`chunk_store.proto:48-53`). That fits the 4 MiB chunk (`config/frontdoor.yaml:8`). Specify the maximum frame size (suggest 256 KiB to 1 MiB) and set `max_receive_message_size` on the server to match. gRPC's default limit is 4 MiB per message, which is exactly `chunk_bytes`, so a client that sends a whole chunk in one frame fails with `RESOURCE_EXHAUSTED`. `GetChunk` has the same issue in reverse (`:77-79`). Say how many frames to expect and that the server never sends a frame larger than the limit.

Write path vs the Rust trait. `ChunkStore::put(chunk_id, Bytes)` (`ts2-core/src/lib.rs:38`) takes a whole chunk. The proto header needs `expected_sha256` and `expected_length` before the first byte (`chunk_store.proto:55-62`). `write_chunk` computes the checksum after `put` (`ts2-core/src/object.rs:39-47`). The client adapter has the whole `Bytes`, so hash before sending, but the code path must change. No design problem, just include it in step 11.

`write_token`. The header documents `PERMISSION_DENIED` when the token is missing (`chunk_store.proto:19,60-61`). ADR 0006:18 and `plan.md` item 8 say tokens arrive in v0.2 ("adds the per-request token"). The field cannot be both required and absent for v0.1. State "empty token accepted when `TS2_REQUIRE_TOKEN` is off" in the proto comment and the C++ config, and keep the token out of steps 9 to 11.

Chunk id validation. The proto says "random 128-bit, lowercase hex" (`:6`). The Rust store accepts any hex string of 4 or more characters, including uppercase (`ts2-core/src/chunk_store.rs:25-30`). The C++ store must accept exactly 32 lowercase hex characters, and reject everything else with `INVALID_ARGUMENT`. The id becomes a path, so this is the traversal guard. On macOS (case-insensitive default volume) uppercase ids would also collide. Tighten the Rust side too.

Idempotency and tmp names. `PutChunk` "already exists with the same bytes succeeds" (`:16`). Say how "same" is decided (compare the stored sidecar sha256, not the data) and what a different payload returns (`ALREADY_EXISTS`; ids are random, so this is a bug signal). The Rust store uses `tmp/<chunk_id>` (`chunk_store.rs:55`). Two concurrent puts of one id would share a tmp file. Use `tmp/<id>.<random>` in C++.

Error model. `PutChunk` maps a length or checksum mismatch to `DATA_LOSS` (`:20`). gRPC defines `DATA_LOSS` as unrecoverable loss. A mismatch in transit is a retryable client-side fault, so use `INVALID_ARGUMENT` or `ABORTED` for put, and keep `DATA_LOSS` for the read-time check (`:29`). `ts2-core/src/error.rs:1-23` has `Corrupt` but no `Unavailable`, `Timeout`, `ChunkNotFound` or `DiskFull` variants. Step 11 needs them to map `UNAVAILABLE`, `DEADLINE_EXCEEDED`, `NOT_FOUND`, `RESOURCE_EXHAUSTED`, and the frontdoor error mapper (`frontdoor/src/error.rs`) must turn them into 503/507, not 500. Also say what the client does on `UNAVAILABLE`: retry budget, deadline per RPC (suggest 30 s for chunk puts, 5 s for index calls).

`GetChunk` verification. The server "re-verifies the stored checksum for whole-chunk reads before the last frame" (`:24-25`). Frames already sent cannot be recalled, so the only signal is a final `DATA_LOSS` status. The frontdoor already re-verifies against the manifest checksum (`object.rs:142`), which is the stronger check. Say so, and require the server to read the whole file and verify before sending the first frame for ranged reads too (review 001 finding). The simplest correct server: read whole chunk (max 4 MiB), verify, then stream slices.

`ListChunks`. Ascending ids and an opaque `page_token` are fine. Define the token as the last id returned. Specify the default and maximum `page_size` (`:88-89` says 0 means node's choice, but no maximum). `created_unix` comes from file mtime; say so and note that restoring a data dir with `cp`/`rsync` without `-p` resets mtimes and makes every chunk look young (safe direction). Add a `min_age_seconds` filter field only if GC runs on the index side.

`ObjectIndexService` vs `ts2-core`. Field names: Rust `ChunkRef.checksum` (`model.rs:19`) vs proto `ChunkRef.sha256` (`object_index.proto:59`). Pick one; the proto name is better, and the Rust JSON in SQLite is not wire format. `node_id` is a single string (`:56`) while v0.4 needs R=3 replicas and the overview mentions erasure coding: add `repeated string replica_node_ids` or make `node_ids` repeated before frontdoor depends on it. Proto3 field additions are compatible, so this is cheap now and a data migration later. `Manifest.created_at` is set by frontdoor (`object.rs:112-115`), and the index assigns `version` only. State that in the proto comment (`object_index.proto:28-31`). `optional bytes inline_data` (`:74`) matches `Option<Vec<u8>>`, and an empty object has `Some(empty)` (`model.rs:34`), so the C++ side must test `has_inline_data()`, not `!empty()`.

Validation claims the index cannot keep. `object_index.proto:31` promises `INVALID_ARGUMENT (key too long ...)`, but `max_key_bytes` is frontdoor config (`model.rs:8`). Either pass a limit to the index via `TS2_MAX_KEY_BYTES`, or delete the clause. The index should still reject empty keys, NUL and control characters, `..` segments and bad bucket names, because it is a separate trust boundary once it is a network service. Share the rules as a table of test vectors (a JSON file under `tests/vectors/`) that the Rust and C++ tests both read.

Overwrite and delete. `PutManifest` returns the new manifest only (`:94`). The replaced manifest's chunks become orphans, which is the known open question from overview section 12. Returning `previous` in `PutManifestResponse` costs one field now and lets frontdoor delete old chunks eagerly, with GC as the safety net. There is no compare-and-set (`If-Match`, `expected_version`). Leave it out of v0.1, but reserve field numbers.

Pagination. `ListObjects` uses `start_after` plus `is_truncated` plus `next_start_after` (`:104-116`), matching the Rust `ListPage` (`model.rs:56-60`). Define `max_keys = 0` (default 1000?) and the maximum. `ListBuckets` and `ListChunks` differ (`ListBuckets` is unpaged). Fine for v0.1; note it.

Versioning. Package `ts2.v1` is right. Add a `buf.yaml` (or at least `buf lint` and `buf breaking` in CI) because two languages generate from these files and nothing else catches a renumbering. Add `reserved` statements as soon as a field is removed.

## C++ scaffold, CMake, vcpkg

- `chunk-store/CMakeLists.txt` and `object-index/CMakeLists.txt` are near-copies (about 40 lines differing in three names). Steps 9 and 10 can be built independently, and each edits its own copy, but both will want the proto generation block and common warnings. Move the `ts2_proto` target to `proto/CMakeLists.txt` (or `cmake/ts2_proto.cmake`) included by both. Cheap to do in step 8, and it avoids two diverging fixes for finding 1.
- `import "google/protobuf/timestamp.proto"` (`object_index.proto:5`) with `IMPORT_DIRS ${PROTO_ROOT}` only (`CMakeLists.txt:20,25`). This works locally (arm64-osx, vcpkg protoc finds its bundled includes next to the tool dir). I could not verify it on Linux. If the Linux CI job fails at the first object-index configure with "File not found: google/protobuf/timestamp.proto", add `${Protobuf_INCLUDE_DIRS}` to `IMPORT_DIRS`.
- `catch_discover_tests` runs the test binary at build time, so it needs the sanitizer runtime to be loadable in the build environment (UBSan only, fine).
- Presets (`CMakePresets.json:16`): `-fsanitize=undefined` only. Steps 9 and 10 write file and pointer code, so add an `asan` preset (`-fsanitize=address,undefined`) and run it in CI for the store and index tests. TSan is the right check for the streaming handlers and the SQLite connection sharing, but gRPC under TSan produces noise, so make it optional.
- `chunk-store/src/main.cpp:25-33`: `server->Wait()` with no signal handling. Docker stop sends SIGTERM, so the process dies without draining in-flight puts and without flushing. Add `Shutdown(deadline)` on SIGTERM. Also no `ResourceQuota` or sync thread cap: the synchronous gRPC server creates a thread per pending call, and `fsync` blocks it. Set `SetSyncServerOption(MAX_POLLERS/NUM_CQS)` or move to the callback API; for a toy, set a max concurrent streams limit and document it.
- `chunk-store/src/service.cpp:7-19`: `healthy` is `!ec` from `std::filesystem::space`. Step 9 should also make it false when `tmp/` or the data dir is not writable, and add the full-disk threshold from `plan.md` step 8.
- `main.cpp:21`: `TS2_DATA_DIR` defaults to `"."`. A C++ store started by hand writes chunks into the working directory. Default to `./data`, which is gitignored (`.gitignore`, `data/`).
- Both Dockerfiles use `-DCMAKE_BUILD_TYPE=Release` with no `-DNDEBUG`/sanitizer issues, fine. The runtime stage is `ubuntu:24.04` with static vcpkg libs, so `libstdc++6` is present. No `HEALTHCHECK`, no `curl`, and `NodeInfo` is not the standard `grpc.health.v1`. Compose cannot health-check these containers. Either add `grpc.health.v1.Health` (links `grpc++_reflection` or `grpcpp/health_check_service_interface.h`), or add a `--health` flag to each binary that calls its own `NodeInfo` and exits 0 or 1, and use it in compose.

## CI and Docker

`.github/workflows/ci.yml`:

- `on: push` plus `on: pull_request` (`:3-5`) runs every PR twice. Restrict `push` to `main` or add a `concurrency` group with `cancel-in-progress: true`.
- Cold build time. vcpkg builds grpc, protobuf, abseil, re2, c-ares, openssl, zlib, upb and spdlog for both Debug and Release by default. On a 4-core runner expect 25 to 40 minutes cold per service. Add a custom triplet `x64-linux-release` (or `VCPKG_BUILD_TYPE=release` in an overlay triplet) and pass `-DVCPKG_TARGET_TRIPLET`; that roughly halves it. The Debug preset then links release deps, which is fine for these tests.
- Cache key (`:50`). `hashFiles(<service>/vcpkg.json)` does not include the vcpkg commit. `/usr/local/share/vcpkg` on `ubuntu-latest` is whatever the runner image ships and changes with image updates. The binary cache entries are keyed by ABI hash, so a vcpkg update just misses them and the job silently rebuilds from source. Pin the baseline: add `"builtin-baseline": "<commit>"` to each `vcpkg.json`.
- `actions/cache@v4` saves only when the job succeeds. If the first run is red or times out, nothing is cached and the next run starts cold again. Split into `actions/cache/restore` and `actions/cache/save` with `if: always()`.
- Two services mean two cache keys and two cold builds of the same dependencies (the `restore-keys` prefix on `:51` lets the second one reuse the first, but only if the first has already saved). Use one `vcpkg.json` for the repo (put it at the root, add `sqlite3`), one key, and a single cached restore step. Both Dockerfiles can then share a `deps` stage or a base image published to GHCR.
- No `protoc` for Rust. When step 11 adds tonic, the `rust` job needs `protoc` (apt `protobuf-compiler`, or the `protoc-bin-vendored` crate), and `frontdoor/Dockerfile` needs the same.
- No Docker build in CI. Both Dockerfiles are unverified: `VCPKG_FORCE_SYSTEM_BINARIES=1` on Ubuntu 24.04 needs `perl` (openssl) and `python3` (some ports), and `build-essential cmake ninja-build git curl zip unzip tar pkg-config` does not list `perl` or `python3`. Add a `docker build` job on `main` only, and run it once by hand before step 9.
- The arm64 note at `:7-8`: the demo target is arm64 (overview), CI is x64. Dependencies are the same, but `F_FULLFSYNC` and arm64-specific atomics are not exercised. Add `macos-latest` for `chunk_store_tests` only (about 10 minutes with a warm cache); this is the only way the macOS fsync path in step 9 gets run anywhere other than the author's laptop.
- Python job (`:23-33`) is fine and is skipped quietly if the directories are missing, which hides a rename. Acceptable.

Docker details: `chunk-store/Dockerfile:12-14` installs deps in a layer keyed on `vcpkg.json` alone, good. But `vcpkg install` runs from `chunk-store/` with no toolchain, so the packages land in `chunk-store/vcpkg_installed`, and the later `cmake -B /out` with the toolchain installs again into `/out/vcpkg_installed`. It only avoids a rebuild because of vcpkg's default binary cache under `$HOME/.cache/vcpkg` (works, but implicit). Cleaner: `RUN cmake -S chunk-store -B /out ...` on a stub tree containing only `vcpkg.json` and a trivial `CMakeLists.txt`, or set `VCPKG_INSTALLED_DIR`. The runtime image should also drop root: it does (`useradd ts2`), but `--home-dir /data` plus a named compose volume gets ownership from the image only on first mount. Fine, note for the compose step.

## Observability and ports (5364f45, e7f8b77, 036aee6, ab56741)

- `frontdoor/src/metrics.rs:47-61`: the `operation` match is hard-coded and `_ => "Unknown"`. A new route added without updating it silently becomes `Unknown`. The `api.rs` test at `5364f45` covers the known routes; add a test that iterates the router's route list and fails when any returns `Unknown`. Cardinality is unchanged because `operation` is a function of (method, route), so it adds no series.
- The histogram mixes semantics under one `operation` label: `PutObject` duration is the whole upload (the handler consumes the body before replying), while `GetObject` is time to response headers (`metrics.rs` shortcut comment, "time to response headers"). The dashboard now groups latency by operation (`e7f8b77`), so a p99 comparison between `GetObject` and `PutObject` is not like for like. Note it in the panel description.
- `bytes_out` uses an exact `size_hint`, so streamed `GetObject` bodies count 0 (same shortcut comment). With the dashboard grouped by operation, `GetObject` shows zero egress. Either wrap the body in a counting stream now (the gRPC path in step 11 changes the body anyway) or hide the panel for that operation.
- `HealthCheck` requests from the compose healthcheck count in the request-rate and error-ratio panels and dominate them on an idle demo. Exclude `operation="HealthCheck"` in the panel queries.
- `docs/ports.md`: the table reserves `50061-50069` as "node 1 to 9" while node 1's default is 50051 (`main.cpp:16`), so node 1 has two ports. Say "nodes 2 to 10", or make 50051 node 1 and document `50051 + n`. The gRPC rows say "container only in Docker", yet compose does not run either C++ service yet, so the rows describe nothing that can be started from `make`. Also port 9000 appears in two rows with different meanings (host process vs container interior); it is correct, but a duplicate-port check script (a `grep` over the table) would mis-flag it.
- `ab56741`: the compose publish binds `127.0.0.1:9001:9000`, which is the safe default. When C++ services join compose, keep them unpublished (the plan does).

## Plan: ordering for steps 8 to 11

Order I would use:

8. One commit series: rename `NodeInfo*` messages, add the missing contract fields (chunk checksum storage rule, `ListChunkRefs`, `previous` on `PutManifestResponse`, replica ids), shared `ts2_proto` CMake file, root `vcpkg.json` with baseline, release triplet, `.dockerignore`, CI fixes, Rust proto crate (`ts2-proto`, `tonic-build`) that compiles both protos and nothing else.
9 and 10. Steps 9 and 10 do not depend on each other. Each touches only its own directory plus its own row in `docs/ports.md` and its own CI matrix entry. Change `proto/`, `ci.yml` and the shared CMake file only in step 8, or in a separate commit that updates both sides; two sets of edits to `proto/` at once is how contract drift starts.
11. After both: tonic clients in `ts2-core` behind the existing traits, placement stays in `ts2-core::place` (`object.rs:10-13`), the `ChunkStore` trait gains a `node_id` argument (it only takes `chunk_id` now, but a `ChunkRef` already records the node), compose gains both services, e2e runs against them.

Do not write step 11's client code against a half-built step 9 or step 10. Give step 11 an in-process fake gRPC server (tonic) in `ts2-core` tests so its tests do not depend on the C++ binaries, and run one e2e smoke through the real C++ services at the end.

The garbage collection (GC) placement (finding 3) must be decided before step 10, because it changes step 10's RPC list.

## What to test in each step

Step 9, ChunkStoreService (Catch2, in-process channel as the existing test does, plus a temp dir):
- Put then get returns the same bytes; get with a range returns the slice; range end past length gives `OUT_OF_RANGE`; empty range rules.
- Put with a wrong `expected_sha256` or `expected_length` stores nothing and leaves nothing in `tmp/`.
- First message is `data`, not `header`: `INVALID_ARGUMENT`. Second `header` mid-stream: `INVALID_ARGUMENT`. Client cancels mid-stream: no file in `data/` and tmp file removed.
- Put twice with the same id and bytes succeeds; same id different bytes fails.
- Flip a byte in the stored file: get returns `DATA_LOSS`, and a range get also fails (whole-chunk verification).
- Delete is idempotent; delete missing chunk succeeds; delete then get is `NOT_FOUND`.
- `ListChunks` paging: 0, 1, page_size boundary, ids sorted, token past the end, a bad token is `INVALID_ARGUMENT`; the tmp dir and sidecars are never listed.
- Startup clears `tmp/` and leaves `data/` alone; a data file with no sidecar is reported, not served.
- Id validation: `../x`, uppercase, 31 and 33 characters, NUL byte.
- Durability ordering cannot be tested by assertion; test it with a fault-injection seam (an `fsync` function pointer that records the call order: tmp file, rename, dir). Test the `F_FULLFSYNC` branch on macOS CI.
- A 4 MiB put in 64 KiB frames, and a frame bigger than the server limit.

Step 10, ObjectIndexService:
- Every Rust `SqliteObjectIndex` behaviour (`ts2-core/src/object_index.rs`) as a table of vectors shared with the Rust tests: version 1 then 2 on overwrite, `NOT_FOUND` with distinct messages for bucket and object, delete returns the manifest, delete bucket with objects is `FAILED_PRECONDITION`, bucket name rules.
- Listing: prefix containing `%`, `_` and `\` (guards against a `LIKE` based implementation), keys that differ only by case, non-ASCII keys sorted bytewise, `start_after` inside and outside the prefix, `max_keys` edges, truncated flag and `next_start_after`. Replace `substr(key, 1, length(?))` (`object_index.rs:184`) with a range scan `key >= prefix AND key < prefix_successor` so the primary key is used and the byte-vs-character length ambiguity goes away.
- Inline and chunked manifests round trip, including an empty inline object and a manifest with 1000 chunk refs; `has_inline_data` semantics.
- Both `inline_data` and `chunks` set, or neither: `INVALID_ARGUMENT`.
- Concurrency: two simultaneous overwrites of one key produce versions n+1 and n+2 with no duplicates (one connection per thread or a mutex; `busy_timeout`; `BEGIN IMMEDIATE` for writes).
- Pragmas: `journal_mode=WAL`, `synchronous=FULL`, `foreign_keys=ON` (per connection), and `fullfsync=ON` and `checkpoint_fullfsync=ON` on macOS. Kill-and-restart test: write, `kill -9`, reopen, read.
- Corrupt manifest blob in the DB returns `INTERNAL`, not a crash.

Step 11, FrontdoorService switch:
- The existing `frontdoor/tests/api.rs` and `make e2e` pass unchanged against the gRPC-backed traits; that is the acceptance bar for "only the client changes".
- Error mapping table test: each gRPC status to each `Error` variant to each HTTP status (404, 409, 503, 507, 500).
- Failure injection with a fake server: chunk node down mid-upload (written chunks are deleted, 503), index down after chunks written (chunks deleted; if the delete also fails, GC reclaims them), slow node hits the deadline.
- Range GET through gRPC for first, middle and last chunk, and a range spanning two chunks.
- A put whose checksum header is computed before send (finding in the proto section).
- GC: a chunk younger than the minimum age is kept; an unreferenced older chunk is deleted; a referenced chunk is never deleted even if its manifest committed between the two list calls.

## Smaller points

- `docs/plan.md` item 7 says FrontdoorService "changes only its client, not its logic". The `ChunkStore` trait change (node id, a delete-by-node, a list method for GC if GC is in frontdoor) and the new `Error` variants make that approximately true but not exactly; adjust the sentence.
- `chunk_store.proto:32` and `:19` list `PERMISSION_DENIED` for delete and put, but no auth exists until v0.2; same note as for `write_token`.
- `ChunkInfo.length` has no matching use in the diff; keep, since scrub and quota need it.
- The `NodeInfo` RPC doubles as the health check (both protos). It reads `statvfs` on every call (`service.cpp:10`); fine for a 5-second healthcheck interval.

## Not checked

Linux builds (no Linux host here; the Dockerfile and the Ubuntu CI job were read, not run), the generated docs site, the Grafana JSON beyond its commit stat, and whether `cargo test` passes on HEAD.
