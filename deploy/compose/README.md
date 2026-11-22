# deploy/compose

Docker compose setup for running TS2 in containers. Today it starts one service, FrontdoorService (Rust), with its SQLite index and chunk files on a named volume (`ts2-data`). There are no other services yet. ObjectIndexService, ChunkStoreService nodes and the observability profile are added by later steps in the [plan](../../docs/plan.md).

## What it owns

- `compose.yaml`: the service, its port (host `127.0.0.1:9001`, container 9000), the data volume and a healthcheck on `/healthz`.
- The images are linux/arm64. On a Mac that means Linux in a VM, so this setup does not exercise the macOS fsync paths. Use `make run` for those.

## Run

From the repo root:

```sh
make up       # build the image and start in the background
make down     # stop (the volume is kept)
```

Then use the API as in the [demo](../../docs/demo.md), with `127.0.0.1:9001` in place of `127.0.0.1:9000`. See the [port registry](../../docs/ports.md). The container reads `config/frontdoor.compose.yaml`.

## Design

[System overview](../../docs/design/000-system-overview.md), section 10 (deployment).
