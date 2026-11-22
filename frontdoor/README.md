# FrontdoorService

Rust (axum, tonic, tower, utoipa 5.x). The binary is `ts2-frontdoor`. It is the only part of TS2 that clients talk to.

When you upload an object, FrontdoorService checks the limits, cuts the bytes into chunks, writes them to chunk storage, and then writes the object's manifest (its size, ETag and ordered list of chunks) to the index. When you read, it looks up the manifest and streams the chunks back. In v0.1 the index is a SQLite file and chunk storage is a local directory, both provided by the [`ts2-core`](../ts2-core/README.md) library inside the same process.

## What it owns

- The `/v1` REST API: buckets, objects (PUT, GET with ranges, HEAD, DELETE) and listing.
- Admission: size and key limits, errors as RFC 9457 problem details, request ids.
- The OpenAPI 3.1 spec (served at `/openapi.json`, with a Scalar UI at `/docs`) and its checked-in copy at `docs/api/openapi.yaml`, written by a test.
- Until v0.4, the in-process BucketRegistry module.

## Build, test, run

From the repo root:

```sh
cargo build -p ts2-frontdoor
cargo test -p ts2-frontdoor      # also regenerates docs/api/openapi.yaml
make run                         # listens on 127.0.0.1:9000, data in ./data
```

Config is `config/frontdoor.yaml` (limits, listen address, data paths). Try the API with the [demo](../docs/demo.md).

## Design

[System overview](../docs/design/000-system-overview.md), section 2 (layers) and section 5 (request paths). A dedicated FrontdoorService design doc (001) is not written yet.
