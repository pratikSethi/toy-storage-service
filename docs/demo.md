# Demo: curl walkthrough

Start FrontdoorService in one terminal. It listens on `127.0.0.1:9000` and keeps data under `./data`.

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **manifest**: the small record kept for each object. It holds the size and the ordered list of its chunks, with the node that holds each one.
> - **ETag**: a fingerprint of an object's bytes (the SHA-256 hash in hex). Two objects with the same ETag have the same content.
> - **inline object**: an object under 64 KiB. It is stored inside its manifest instead of as chunks.
> - **gRPC**: a way for services to call each other over the network (a remote procedure call, RPC), sending Protobuf messages over HTTP/2. The TS2 services talk to each other this way.

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

## With the CLI

The `ts2` CLI (`cli/`) does the same without curl. Install it once with `uv tool install --editable ./cli`, or prefix each command with `uv run --project cli`.

```sh
ts2 mb demo
ts2 cp ./big.bin ts2://demo/big.bin        # prints the ETag
ts2 ls demo --prefix docs/
ts2 head ts2://demo/big.bin
ts2 cp ts2://demo/big.bin ./copy.bin
ts2 rm ts2://demo/big.bin
ts2 rb demo
```

`--endpoint` or `TS2_ENDPOINT` points it elsewhere. Errors print as `title: detail (request_id)`. `make e2e` runs the same flow as a test.

## API docs

- Interactive reference (Scalar): <http://127.0.0.1:9000/docs>
- Spec as JSON: <http://127.0.0.1:9000/openapi.json>
- Spec as a checked-in file: [`api/openapi.yaml`](api/openapi.yaml), written by a frontdoor test so API changes show up in review diffs.

The OpenAPI file imports into Bruno: Collection, Import, OpenAPI, then pick `docs/api/openapi.yaml` (or the `/openapi.json` URL).

## With compose

`make up` builds the images (linux/arm64) and serves the same API on `127.0.0.1:9001`. Frontdoor runs in remote mode against the C++ ObjectIndexService and ChunkStoreService containers, with data in named volumes. The first build compiles gRPC and takes a long time. `make e2e-compose` runs the smoke test against it. It does not collide with `make run` on 9000. `make down` stops it.
