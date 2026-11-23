# Port registry

Every port TS2 uses, in one place. **Rule: a new port goes in this table in the same commit that introduces it.**

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **gRPC**: a way for services to call each other over the network (a remote procedure call, RPC), sending Protobuf messages over HTTP/2. The TS2 services talk to each other this way.
> - **OTel** (OpenTelemetry): a standard way to emit metrics, logs and traces.
> - **OTLP** (OpenTelemetry protocol): the wire format OTel data is sent in.

| Port | What | How started | Reachable from | Defined in |
|---|---|---|---|---|
| 9000 | FrontdoorService (the default for the CLI, curl examples and Scalar docs) | `make run` | host, `127.0.0.1` | `config/frontdoor.yaml` |
| 9001 | FrontdoorService in compose | `make up`, `make up-obs` | host, `127.0.0.1` (maps to container 9000) | `deploy/compose/compose.yaml` |
| 9000 | FrontdoorService inside its container | compose | container only | `config/frontdoor.compose.yaml`, `frontdoor/Dockerfile` |
| 9099 | FrontdoorService for the smoke test, with its own data dir | `make e2e` | host, `127.0.0.1`, while the test runs | `Makefile` |
| 8000 | MkDocs dev server (the `mkdocs serve` default, not set in the repo) | `make docs` | host, `127.0.0.1` | `Makefile` |
| 3000 | Grafana | `make up-obs` | host, `127.0.0.1` | `deploy/compose/compose.yaml` |
| 4318 | OpenTelemetry Collector, OTLP over HTTP | `make up-obs` | container only | `deploy/compose/otel/collector.yaml`, `Makefile` |
| 8428 | VictoriaMetrics (image default, not set in the repo) | `make up-obs` | container only | `deploy/compose/otel/collector.yaml` |
| 9428 | VictoriaLogs, web UI at `/select/vmui` (image default port) | `make up-obs` | host, `127.0.0.1` | `deploy/compose/compose.yaml` |
| 50051 | ChunkStoreService gRPC (default of the binary; compose overrides it per node, see the rows below) | built binary | host `127.0.0.1` | `chunk-store/src/main.cpp` |
| 50052 | ObjectIndexService gRPC | built binary, or its container | host `127.0.0.1` when run directly, container only in Docker | `object-index/src/main.cpp`, `object-index/Dockerfile` |
| 50151 | ChunkStoreService for `make e2e-native`, temp data dir | `make e2e-native` | host, `127.0.0.1`, while the test runs | `Makefile` |
| 50152 | ObjectIndexService for `make e2e-native`, temp data dir | `make e2e-native` | host, `127.0.0.1`, while the test runs | `Makefile` |
| 50153-50155 | ChunkStoreService nodes 2 to 4 for `make e2e-native` (node 1 is on 50151), temp data dirs | `make e2e-native` | host, `127.0.0.1`, while the test runs | `Makefile` |
| 50061 | ChunkStoreService `node-1` in compose (`chunk-store-1`) | `make up` | container only | `deploy/compose/compose.yaml` |
| 50062 | ChunkStoreService `node-2` in compose (`chunk-store-2`) | `make up` | container only | `deploy/compose/compose.yaml` |
| 50063 | ChunkStoreService `node-3` in compose (`chunk-store-3`) | `make up` | container only | `deploy/compose/compose.yaml` |
| 50064 | ChunkStoreService `node-4` in compose (`chunk-store-4`) | `make up` | container only | `deploy/compose/compose.yaml` |
| 50065-50069 | Reserved for ChunkStoreService nodes 5 to 9 | not used yet | n/a | this page |

Compose publishes only FrontdoorService (9001), Grafana (3000) and the VictoriaLogs UI (9428) to the host. The collector and VictoriaMetrics talk to each other on the compose network.

The two C++ services read `TS2_LISTEN`, so their ports can be overridden. The values above are the defaults.

## Why separate ports

- `make run` and `make up` can run side by side. The host process keeps 9000 and the compose stack uses 9001, so neither fails with "address already in use".
- `make e2e` starts its own FrontdoorService on 9099 with a throwaway data dir. It never collides with a dev server on 9000 or the compose stack on 9001, and it never touches their data.
- The 5006x range gives each compose ChunkStoreService node its own port (`TS2_LISTEN`), away from the two fixed gRPC ports.

## Adding a port

1. Pick a free number that follows the grouping above.
2. Add a row to the table in the same commit that introduces the port.
3. Say whether it is published to the host or container only, and name the file that defines it.
