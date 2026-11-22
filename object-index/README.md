# ObjectIndexService

C++20 (gRPC, protobuf, SQLite, spdlog, Catch2, built with CMake and vcpkg). The binary is `ts2-object-index`.

When you upload an object, its bytes end up as chunks on storage nodes. The manifest is the small record that says which chunks, in what order, make up the object, along with its size, ETag and content type. ObjectIndexService stores one manifest per `(bucket, key)` and answers "what is under this prefix?". It never touches chunk data, except tiny objects that are stored inline in the manifest itself.

## What it will own

- Buckets: create, get, delete (only when empty), list.
- Manifests: put (the commit point of an upload, assigns the version), get, delete, and listing keys by prefix in order.
- `ListChunkRefs`: every chunk reference in every manifest, so FrontdoorService can find orphan chunks. Orphan GC itself is not done here.
- Durability of that metadata, in SQLite first.

The contract is [`proto/ts2/v1/object_index.proto`](../proto/ts2/v1/object_index.proto), which mirrors the `ObjectIndex` trait in [`ts2-core`](../ts2-core/README.md). Today the skeleton answers `NodeInfo` only; every other RPC returns `UNIMPLEMENTED`.

## Build, test, run

You need CMake, Ninja, pkg-config, a C++20 compiler and [vcpkg](https://github.com/microsoft/vcpkg) with `VCPKG_ROOT` set. On macOS: `brew install cmake ninja pkgconf`. The first build compiles gRPC from source and takes a while; vcpkg caches it after that.

```sh
cd object-index
cmake --preset debug          # UBSan on (no ASan: vcpkg protobuf is built without it and crashes)
cmake --build --preset debug
ctest --preset debug
./build/debug/ts2-object-index # listens on 127.0.0.1:50052
```

Config is environment variables: `TS2_LISTEN`, `TS2_NODE_ID`, `TS2_CELL`, `TS2_DATA_DIR`. The container image is built from the repo root: `docker build -f object-index/Dockerfile -t ts2-object-index .`

## Design

[System overview](../docs/design/000-system-overview.md), the manifest schema and request path sections. A dedicated design doc is not written yet.
