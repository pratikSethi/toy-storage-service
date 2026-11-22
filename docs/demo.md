# Demo: curl walkthrough

Start FrontdoorService in one terminal. It listens on `127.0.0.1:9000` and keeps data under `./data`.

```sh
make run
```

In another terminal:

```sh
B=http://127.0.0.1:9000/v1/buckets

# Create a bucket (201)
curl -i -X POST $B -H 'content-type: application/json' -d '{"name":"demo"}'

# Put a small file (inline in the manifest) and a 10 MiB file (3 chunks of 4 MiB)
echo 'hello ts2' > hello.txt
curl -i -X PUT $B/demo/objects/docs/hello.txt -H 'content-type: text/plain' --data-binary @hello.txt
head -c 10485760 /dev/urandom > big.bin
curl -i -X PUT $B/demo/objects/big.bin --data-binary @big.bin

# Get it back, compare, and read only the headers
curl -s $B/demo/objects/docs/hello.txt
curl -s $B/demo/objects/big.bin | cmp - big.bin && echo identical
curl -I $B/demo/objects/big.bin

# Range GET across the first chunk boundary (4 MiB = 4194304): returns 206 with 10 bytes
curl -s -D - -H 'Range: bytes=4194299-4194308' $B/demo/objects/big.bin -o /dev/null
curl -s -H 'Range: bytes=-5' $B/demo/objects/docs/hello.txt     # last 5 bytes
curl -s -H 'Range: bytes=6-' $B/demo/objects/docs/hello.txt     # from byte 6 to the end

# List with a prefix and paging
curl -s "$B/demo/objects?prefix=docs/&max_keys=1"
curl -s "$B/demo/objects?max_keys=1&start_after=big.bin"

# A missing object is a problem+json 404
curl -i $B/demo/objects/nope

# The 413 demo: the limit is 256 MiB (max_object_bytes in config/frontdoor.yaml)
head -c 300000000 /dev/zero > huge.bin
curl -i -X PUT $B/demo/objects/huge.bin --data-binary @huge.bin
rm huge.bin

# Delete the objects, then the bucket (409 while it still has objects)
curl -i -X DELETE $B/demo
curl -i -X DELETE $B/demo/objects/docs/hello.txt
curl -i -X DELETE $B/demo/objects/big.bin
curl -i -X DELETE $B/demo
```

Every response carries `Server: ts2-frontdoor/<version>` and `x-ts2-request-id`. Send your own `x-ts2-request-id` and it is echoed back. Errors are `application/problem+json` and repeat the request id in `request_id`.

Only a single range is supported. `bytes=a-b`, `bytes=a-` and `bytes=-n` work. An unsatisfiable range, or several ranges in one header, returns 416.

## API docs

- Interactive reference (Scalar): <http://127.0.0.1:9000/docs>
- Spec as JSON: <http://127.0.0.1:9000/openapi.json>
- Spec as a checked-in file: [`api/openapi.yaml`](api/openapi.yaml), written by a frontdoor test so API changes show up in review diffs.

The OpenAPI file imports into Bruno: Collection, Import, OpenAPI, then pick `docs/api/openapi.yaml` (or the `/openapi.json` URL).

## With compose

`make up` builds the image (linux/arm64) and serves the same API on `127.0.0.1:9000`, with data in a named volume. `make down` stops it.
