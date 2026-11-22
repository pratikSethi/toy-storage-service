# proto

The internal contract between TS2 services: Protobuf and gRPC, package `ts2.v1`. The external contract for clients is the OpenAPI spec in `docs/api/openapi.yaml`.

| File | Service | Implemented by |
| --- | --- | --- |
| `ts2/v1/chunk_store.proto` | `ChunkStoreService` | `chunk-store/` (C++) |
| `ts2/v1/object_index.proto` | `ObjectIndexService` | `object-index/` (C++) |

The RPCs mirror the `ChunkStore` and `ObjectIndex` traits in `ts2-core`, so the gateway can swap its in-process implementations for gRPC clients without changing its logic. Every RPC documents its error codes in the proto comments.

Rules: add fields with new numbers, never reuse or renumber, never change a type. Breaking changes need a new package version (`v2`).

## Using it

**C++ (grpc++).** Both services include `cmake/ts2_proto.cmake`, which runs `protoc` with the grpc plugin over every file here and builds one `ts2_proto` library.

**Rust (tonic).** The `ts2-proto` crate compiles both files with `tonic-prost-build` and exposes them as `ts2_proto::v1`. FrontdoorService will depend on it in round 4 step (c).

Only the step 0 owner edits files here during round 4. The two service workers ask for a contract change instead of making it, so the Rust and C++ sides cannot drift.
