# Components

One section per component. Terms used here, in short: a **bucket** and **key** name an object. A **chunk** is a 4 MiB piece of an object. A **manifest** is the small record per object: size, ETag, and the list of chunks and which node holds each. The full definitions are in the [system overview](../design/000-system-overview.md).

## FrontdoorService (Rust)

The only thing clients talk to. It checks limits, cuts an upload into chunks, writes the chunks to ChunkStoreService nodes, then writes the manifest to ObjectIndexService. A read looks up the manifest and streams the chunks back in order. It also emits the `ts2_http_*` metrics and one JSON log line per request.

- Status: v0.1 done, remote backends and orphan GC landed after it.
- Design: [system overview, section 5](../design/000-system-overview.md), [ADR 0006](../adr/0006-object-bytes-through-frontdoor.md).
- Directory: `frontdoor/`.

## ts2-core (Rust library)

The shared logic that FrontdoorService uses: the data model, the `ObjectIndex` and `ChunkStore` traits, the in-process SQLite index and local-disk chunk store, and the gRPC clients for the C++ services. Until the C++ services are on, FrontdoorService runs these in-process.

- Status: v0.1 done, tonic clients added for v0.2.
- Design: [system overview, sections 4 and 6](../design/000-system-overview.md).
- Directory: `ts2-core/`.

## ObjectIndexService (C++)

Keeps the manifest for every bucket/key in a cell. It is the one place that knows which chunks make up an object, so losing it loses every object in the cell until backups or a replicated index exist.

- Status: v0.2, in progress.
- Design: [system overview, sections 3 and 6](../design/000-system-overview.md), [ADR 0004](../adr/0004-cells-for-the-data-plane-thin-global-layer.md).
- Directory: `object-index/`.

## ChunkStoreService (C++)

Stores chunks on a node's disk by chunk id and returns them on request, verifying the checksum. A cell has several nodes, and FrontdoorService picks the node for each chunk.

- Status: v0.2, in progress.
- Design: [system overview, sections 5 and 7](../design/000-system-overview.md), [ADR 0003](../adr/0003-random-chunk-ids-not-content-addressed.md).
- Directory: `chunk-store/`.

## ts2 CLI (Python)

The command line client (`mb`, `rb`, `ls`, `cp`, `rm`, `head`). It speaks only the public REST API, so it knows about buckets and keys and nothing about chunks.

- Status: v0.1 done.
- Docs: [CLI reference](../cli/index.md).
- Directory: `cli/`.

## Observability stack

FrontdoorService exports OTLP to an OpenTelemetry Collector, which feeds VictoriaMetrics (metrics) and VictoriaLogs (logs). Grafana shows the provisioned `frontdoor` dashboard. It runs with `make up-obs`. Thresholds and dashboards are described in [operations](operations.md).

- Status: v0.1 done. Dashboards for the C++ services come in v0.2.
- Design: [system overview, section 9](../design/000-system-overview.md).
- Directory: `deploy/compose/`.
