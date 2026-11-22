# Build plan

Commit by commit. Each commit leaves a runnable system.

1. **Docs.** PRD, system overview, ADRs, backlog, this plan.
2. **Scaffold.** `Makefile`, Cargo workspace (`frontdoor`, `ts2-core`), multi-stage `Dockerfile` that works with docker and podman, compose with frontdoor only, `mkdocs.yml` with the Material theme.
3. **Frontdoor v0.1.** Health, bucket CRUD, object PUT/GET/HEAD/DELETE/list. Single-binary happy path with an in-process SQLite index module and a local-disk chunkstore module, using the same traits the real services will implement. utoipa and Scalar, RFC 9457 errors, request ids, JSON logs. Try it with curl and Bruno.
4. **Observability profile.** OTel Collector, VictoriaMetrics, VictoriaLogs, Grafana with one frontdoor dashboard.
5. **Chunkstore.** C++ service with gRPC. Frontdoor switches from the local module to it.
6. **Index.** C++ service, same switch.
7. **Python CLI and e2e tests.**
8. **Second cell.** Placement and replication.

Docs are served locally with `mkdocs serve` and published to GitHub Pages once the repo is public.
