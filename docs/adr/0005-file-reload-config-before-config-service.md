# ADR 0005: File-reload config before a config service

Status: accepted for v0, not built

## Context

Limits (such as `max_object_bytes`) and feature flags need to change without redeploying, so a limit can be bumped live. A config service or a flag service (OpenFeature with flagd) would do this, but adds another system to build and run before the core exists.

## Decision

Each service loads a config file at start and reloads it on change, through SIGHUP or a file watch. The file has a `limits` block and a `flags` block. A bad file is rejected and the previous config stays active. A config service and OpenFeature/flagd are deferred.

**Today:** FrontdoorService reads `config/frontdoor.yaml` once at start. There is no reload, no `flags` block and no bad-file rollback. The C++ services read environment variables. A config service is also not built.

## Consequences

- No extra service in v0. Changing a limit is an edit to a file, visible in a compose volume or a Kubernetes ConfigMap.
- Services read the same file shape, so a later config service can serve the same content.
- No per-tenant targeting or gradual rollout of flags.
- Every service must handle reload safely, and limits must be read from the current config on each request.

## Alternatives considered

- **Config service now.** Rejected: a large detour from storage work.
- **OpenFeature with flagd now.** Rejected for the same reason. It stays an option once flags need targeting.
- **Environment variables only.** Rejected: needs a restart to change.
