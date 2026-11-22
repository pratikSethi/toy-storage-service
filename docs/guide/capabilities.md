# Capabilities

What you can do today. Each row shows the CLI and the curl form. Full detail is in the [CLI reference](../cli/index.md) and the [API reference](../api/index.md). The curl lines assume `B=$TS2_ENDPOINT/v1/buckets`.

| Capability | CLI | curl |
|---|---|---|
| Create a bucket | `ts2 mb photos` | `curl -X POST $B -H 'content-type: application/json' -d '{"name":"photos"}'` |
| List buckets | n/a | `curl $B` |
| Delete a bucket (must be empty) | `ts2 rb photos` | `curl -X DELETE $B/photos` |
| Upload an object | `ts2 cp cat.jpg ts2://photos/cats/cat.jpg` | `curl -X PUT $B/photos/objects/cats/cat.jpg -H 'content-type: image/jpeg' -T cat.jpg` |
| Download an object | `ts2 cp ts2://photos/cats/cat.jpg out.jpg` | `curl $B/photos/objects/cats/cat.jpg -o out.jpg` |
| Read object metadata | `ts2 head ts2://photos/cats/cat.jpg` | `curl -I $B/photos/objects/cats/cat.jpg` |
| Delete an object | `ts2 rm ts2://photos/cats/cat.jpg` | `curl -X DELETE $B/photos/objects/cats/cat.jpg` |
| List objects by prefix | `ts2 ls photos` | `curl "$B/photos/objects?prefix=cats/&max_keys=100"` |
| Read part of an object | n/a | `curl -H 'Range: bytes=0-1023' $B/photos/objects/cats/cat.jpg` |

A range read supports one range per request: `bytes=a-b`, `bytes=a-` or `bytes=-n` (the last n bytes). A satisfiable range returns `206`.

## Coming later

Planned, not available: authentication with API keys, multipart and resumable uploads for large files, presigned URLs, quotas, per-bucket region or location, and object versioning you can read back. The list and order live in the [backlog](../backlog.md).
