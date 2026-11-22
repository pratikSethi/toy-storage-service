# proto

The internal contract between TS2 services: Protobuf and gRPC, package `ts2.v1`. The external contract for clients is the OpenAPI spec in `docs/api/openapi.yaml`.

| File | Service | Implemented by |
| --- | --- | --- |
| `ts2/v1/chunk_store.proto` | `ChunkStoreService` | `chunk-store/` (C++) |
| `ts2/v1/object_index.proto` | `ObjectIndexService` | `object-index/` (C++) |

The RPCs mirror the `ChunkStore` and `ObjectIndex` traits in `ts2-core`, so the gateway can swap its in-process implementations for gRPC clients without changing its logic. Every RPC documents its error codes in the proto comments.

Rules: add fields with new numbers, never reuse or renumber, never change a type. Breaking changes need a new package version (`v2`).

## Using it

**C++ (grpc++).** Each service's CMake project runs `protoc` with the grpc plugin over `../proto` and links the generated sources. See `chunk-store/CMakeLists.txt`.

**Rust (tonic, later).** The gateway will add a `build.rs` using `tonic-build` pointed at `../proto`, and call the services through the generated clients.
