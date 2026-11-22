# Limits

These are the defaults in `config/frontdoor.yaml` and the API. Limits are checked when a request arrives and fail with a specific error.

| Limit | Value | Error | Configurable |
|---|---|---|---|
| Object size | 256 MiB | 413 | Yes (`max_object_bytes`). Raising is safe. |
| Key length | 1024 bytes | 400 | Yes (`max_key_bytes`). Lowering is unsafe, because existing keys could already be longer, so we do not do it. |
| Bucket name | 3 to 63 characters: lowercase letters, digits, hyphens | 400 | No |
| Keys | Not empty, no `..` segments, slashes allowed | 400 | No |
| List page size | 1000 keys (the default and the maximum) | n/a, larger values are capped | No |
| Ranges per read | 1 | 416 | No |

Not limited yet: number of buckets, number of objects, request rate, and total storage per team. Quotas and rate limits are on the [backlog](../backlog.md). If you plan to store more than a few hundred GiB, or need objects above 256 MiB, tell us first.
