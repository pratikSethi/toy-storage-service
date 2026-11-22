# API reference

The TS2 API is the JSON REST interface that FrontdoorService exposes for buckets and objects. This page renders the checked-in spec, `docs/api/openapi.yaml`, so you can read it without running anything. The live interactive version is served at `/docs` by a running FrontdoorService (port 9000 for `make run`, 9001 for Docker, see [Port registry](../ports.md)). The source of truth is the Rust code: the spec is generated from the utoipa annotations by a frontdoor test, so change the handlers, not the YAML.

## Endpoints

| Method | Path | What it does |
|---|---|---|
| GET | `/healthz` | Check that the service is up |
| GET | `/v1/buckets` | List buckets |
| POST | `/v1/buckets` | Create a bucket |
| GET | `/v1/buckets/{bucket}` | Get a bucket |
| DELETE | `/v1/buckets/{bucket}` | Delete a bucket (must be empty) |
| GET | `/v1/buckets/{bucket}/objects` | List objects, with prefix and paging |
| GET | `/v1/buckets/{bucket}/objects/{key}` | Download an object |
| PUT | `/v1/buckets/{bucket}/objects/{key}` | Upload an object |
| DELETE | `/v1/buckets/{bucket}/objects/{key}` | Delete an object |
| HEAD | `/v1/buckets/{bucket}/objects/{key}` | Get object metadata |

Each of these has a [CLI command](../cli/index.md): `mb`, `rb`, `ls`, `cp`, `rm` and `head`. Request and response shapes follow.

## Full spec

<swagger-ui src="openapi.yaml"/>
