# Debugging one upload

This page sets up VS Code so you can start the whole stack under debuggers and step one `ts2 cp` upload from the CLI to the disk.

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **manifest**: the small record kept for each object. It holds the size and the ordered list of its chunks, with the node that holds each one.
> - **ETag**: a fingerprint of an object's bytes (the SHA-256 hash in hex). Two objects with the same ETag have the same content.
> - **gRPC**: a way for services to call each other over the network (a remote procedure call, RPC), sending Protobuf messages over HTTP/2. The TS2 services talk to each other this way.
> - **fsync**: a system call that forces written data onto the disk before it returns, so a crash cannot lose it.
> - **SQLite**: a small database that lives in one file.

## What you need

| Extension | Id | Used for |
|---|---|---|
| CodeLLDB | `vadimcn.vscode-lldb` | the `lldb` launches: FrontdoorService (Rust), ChunkStoreService and ObjectIndexService (C++) |
| rust-analyzer | `rust-lang.rust-analyzer` | Rust navigation and symbols |
| Python and Python Debugger | `ms-python.python`, `ms-python.debugpy` | the `ts2` CLI launch |
| C/C++, CMake Tools | `ms-vscode.cpptools`, `ms-vscode.cmake-tools` | optional, for C++ navigation and IntelliSense. Debugging does not need them. |

The C++ debug presets must be configured once, so that `chunk-store/build/debug` and `object-index/build/debug` exist (see [setup](../setup.md)). The launches build into those folders.

## Run it

1. Open the repo root in VS Code.
2. In Run and Debug, pick **Debug TS2** and start it. The pre-launch task builds both C++ services (`cmake --build --preset debug`) and FrontdoorService (`cargo build -p ts2-frontdoor`), then starts all three under CodeLLDB.
3. Set breakpoints (next section).
4. Pick **Debug ts2 CLI** and start it. Its pre-launch task runs `uv sync --project cli`, writes a 10 MiB file to `data/debug/ten.bin` and makes the `demo` bucket. The launch then runs `ts2 cp data/debug/ten.bin ts2://demo/ten.bin` under debugpy with `cli/.venv/bin/python`.

Everything it writes is under `data/debug/`, which is gitignored. Delete that folder to start clean.

| Launch | Process | Port | Config |
|---|---|---|---|
| ChunkStoreService | `chunk-store/build/debug/ts2-chunk-store` | 50051 | `TS2_LISTEN`, `TS2_DATA_DIR=data/debug/chunks` |
| ObjectIndexService | `object-index/build/debug/ts2-object-index` | 50052 | `TS2_LISTEN`, `TS2_DATA_DIR=data/debug/index` |
| FrontdoorService (remote) | `target/debug/ts2-frontdoor` | 9000 | `.vscode/frontdoor.debug.yaml`, `TS2_INDEX_ADDR`, `TS2_CHUNK_NODES` |
| Debug ts2 CLI | `python -m ts2` | n/a | `TS2_ENDPOINT=http://127.0.0.1:9000` |
| Debug frontdoor (in-process) | `target/debug/ts2-frontdoor` | 9000 | same yaml, no `TS2_*` addresses |

**Debug frontdoor (in-process)** is the single-process variant. ts2-core runs the SQLite index and the on-disk chunk store inside FrontdoorService, so you can step from the HTTP handler to the disk without gRPC. It uses port 9000 too, so do not run it together with Debug TS2.

## Pausing at a breakpoint

FrontdoorService gives each gRPC call a deadline (30 s for PutChunk, 10 s for GetChunk, 5 s for the rest). A paused debugger would turn into a 504. The remote launch sets `TS2_DEADLINE_PUT_CHUNK_SECS`, `TS2_DEADLINE_GET_CHUNK_SECS` and `TS2_DEADLINE_OTHER_SECS` to 3600. Unset, the defaults stay as they were. The variables are read in `frontdoor/src/main.rs: deadlines_from_env`.

Limit: the CLI's HTTP client has a 30 s write timeout (`cli/src/ts2/cli.py: main`). If you stay paused in FrontdoorService for more than 30 s while the CLI is still sending the body, the CLI fails with a write timeout. Pause in the CLI or the C++ services freely. In FrontdoorService, resume within 30 s or use a small file.

## Follow one PUT

Set these breakpoints in order, then start Debug ts2 CLI. Each hit is the next hop.

| # | Where | What you see |
|---|---|---|
| 1 | `cli/src/ts2/cli.py: cp` | the `ts2://demo/ten.bin` uri and the streaming `PUT` request |
| 2 | `frontdoor/src/handlers.rs: put_object` | admission checks (key, bucket, Content-Length), then the body becomes a stream |
| 3 | `ts2-core/src/object.rs: put_object`, then `read_and_commit` | the body is cut into 4 MiB chunks; a 10 MiB object makes three |
| 4 | `ts2-core/src/object.rs: write_chunk` | a random chunk id, the node picked by `place`, then `chunks.put` |
| 5 | `ts2-core/src/remote.rs: RemoteChunkStore::put` (the `ChunkStore` impl) | the sha256 header and the 1 MiB frames of the `PutChunk` stream |
| 6 | `chunk-store/src/service.cpp: ChunkStoreServiceImpl::PutChunk` | header read, `begin_put`, then the frame loop calling `append` |
| 7 | `chunk-store/src/disk_store.cpp: ChunkWriter::commit` | the durable write: fsync both temp files, rename the sidecar then the data file, fsync the directory |
| 8 | `ts2-core/src/remote.rs: RemoteObjectIndex::put_manifest` (the `ObjectIndex` impl) | the manifest listing the three chunks, sent after every chunk is durable |
| 9 | `object-index/src/service.cpp: ObjectIndexServiceImpl::PutManifest` | the gRPC entry, which calls the SQLite index |
| 10 | `object-index/src/sqlite_index.cpp: SqliteIndex::put_manifest` | manifest validation, then the transaction that stores it and returns the previous version |
| 11 | back in `frontdoor/src/handlers.rs: put_object` | `StatusCode::CREATED` with the ETag header, which the CLI prints as `uploaded` |

Steps 4 to 7 repeat once per chunk. Step 8 runs once, after the last chunk. In the in-process launch, steps 5, 6, 8 and 9 are replaced by the local implementations in `ts2-core/src/chunk_store.rs` and the SQLite index.

## Troubleshooting

- **Address already in use**: another stack is on 9000, 50051 or 50052. Check [ports](../ports.md), then stop the other run.
- **Binary not found**: the pre-launch build failed or the C++ preset was never configured. Run the preset once from `chunk-store/` and `object-index/`.
- **The prepare task prints a bucket error on the second run**: expected, the bucket already exists. The task ignores it.
