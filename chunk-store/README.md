# ChunkStoreService

C++20 (gRPC, protobuf, spdlog, Catch2, built with CMake and vcpkg). The binary is `ts2-chunk-store`. It runs on each storage node.

An object is cut into fixed-size chunks, and each chunk is identified by a hash of its bytes. ChunkStoreService is the process that keeps those chunks on one node's disk. The gateway sends it chunks to store and asks for them back, in whole or by byte range. It knows nothing about objects, keys or buckets; that is the index's job.

## What it will own

- Writing a chunk safely: temp file, checksum check, fsync, rename into place.
- Reading a chunk or a range of it, with checksum verification.
- Deleting chunks and listing them so garbage collection can find orphans.
- Reporting node id, cell, failure domain and free space.

The contract is [`proto/ts2/v1/chunk_store.proto`](../proto/ts2/v1/chunk_store.proto). Today the skeleton answers `NodeInfo` only; every other RPC returns `UNIMPLEMENTED`.

## Build, test, run

You need CMake, Ninja, pkg-config, a C++20 compiler and [vcpkg](https://github.com/microsoft/vcpkg) with `VCPKG_ROOT` set. On macOS: `brew install cmake ninja pkgconf`. The first build compiles gRPC from source and takes a while; vcpkg caches it after that.

```sh
cd chunk-store
cmake --preset debug          # UBSan on (no ASan: vcpkg protobuf is built without it and crashes)
cmake --build --preset debug
ctest --preset debug
./build/debug/ts2-chunk-store # listens on 127.0.0.1:50051
```

Config is environment variables: `TS2_LISTEN`, `TS2_NODE_ID`, `TS2_CELL`, `TS2_FAILURE_DOMAIN`, `TS2_DATA_DIR`. The container image is built from the repo root: `docker build -f chunk-store/Dockerfile -t ts2-chunk-store .`

## Design

[System overview](../docs/design/000-system-overview.md), the chunk write path, range reads and garbage collection sections. A dedicated design doc is not written yet.
