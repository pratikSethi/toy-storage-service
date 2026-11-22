# ADR 0001: JSON REST API instead of S3 wire compatibility

Status: accepted

## Context

TS2 needs an external API. S3 compatibility would let existing SDKs and tools work, but it means XML bodies, SigV4 request signing, and many quirks (virtual-host addressing, error codes, listing formats). That is a large amount of work that teaches little about storage internals.

## Decision

The external API is JSON over HTTP, versioned in the path (`/v1/...`), described by OpenAPI 3.1, generated from code: FrontdoorService uses utoipa 5.x (utoipa 4 emits OpenAPI 3.0) on annotated axum handlers, serves the spec at `/openapi.json` and a Scalar UI at `/docs`, and a test writes it to `docs/api/openapi.yaml`. Errors use RFC 9457 Problem Details with a `request_id` extension. List uses GET with query parameters. The IETF HTTP QUERY method is parked until it is an RFC. An S3 adapter may be added later as a separate component.

## Consequences

- The generated Python client buffers request bodies in memory, so the CLI uses plain `httpx` for object bytes.
- Faster to build and easy to call with curl and httpx.
- No existing S3 SDK or tool works against TS2 in v0.
- The spec is generated from the handlers and reviewed as a file in diffs, so it drifts less than a hand-written one. It is still only as good as the handler annotations, and a test must regenerate it.
- The API is ours to design, so we own its compatibility rules from `/v1` onward.

## Alternatives considered

- **S3 XML plus SigV4 now.** Rejected: cost is high, learning value is low.
- **gRPC externally.** Rejected: harder to use from curl and browsers. gRPC is used internally.
- **S3 adapter later.** Kept open as an optional add-on.
