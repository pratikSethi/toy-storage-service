# API reference

The TS2 API is the JSON REST interface that FrontdoorService exposes for buckets and objects. This page renders the checked-in spec, `docs/api/openapi.yaml`, so you can read it without running anything. The live interactive version is served at `/docs` by a running FrontdoorService (port 9000 for `make run`, 9001 for Docker, see [Port registry](../ports.md)). The source of truth is the Rust code: the spec is generated from the utoipa annotations by a frontdoor test, so change the handlers, not the YAML.

<swagger-ui src="openapi.yaml"/>
