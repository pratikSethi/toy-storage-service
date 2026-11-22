# Port registry

Every port TS2 uses, in one place. **Rule: a new port goes in this table in the same commit that introduces it.**

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
| 9428 | VictoriaLogs (image default, not set in the repo) | `make up-obs` | container only | `deploy/compose/otel/collector.yaml` |
| 50051 | ChunkStoreService gRPC | built binary, or its container | host `127.0.0.1` when run directly, container only in Docker | `chunk-store/src/main.cpp`, `chunk-store/Dockerfile` |
| 50052 | ObjectIndexService gRPC | built binary, or its container | host `127.0.0.1` when run directly, container only in Docker | `object-index/src/main.cpp`, `object-index/Dockerfile` |
| 50061-50069 | Reserved for more ChunkStoreService nodes (node 1 to 9) | not used yet | n/a | this page |

Compose publishes only FrontdoorService (9001) and Grafana (3000) to the host. The collector, VictoriaMetrics and VictoriaLogs talk to each other on the compose network.

The two C++ services read `TS2_LISTEN`, so their ports can be overridden. The values above are the defaults.

## Why separate ports

- `make run` and `make up` can run side by side. The host process keeps 9000 and the compose stack uses 9001, so neither fails with "address already in use".
- `make e2e` starts its own FrontdoorService on 9099 with a throwaway data dir. It never collides with a dev server on 9000 or the compose stack on 9001, and it never touches their data.
- The reserved 5006x range keeps future ChunkStoreService nodes away from the two fixed gRPC ports.

## Adding a port

1. Pick a free number that follows the grouping above.
2. Add a row to the table in the same commit that introduces the port.
3. Say whether it is published to the host or container only, and name the file that defines it.
