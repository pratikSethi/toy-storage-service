# CLI reference

`ts2` is the command line client for the [API](../api/index.md). It is a small Python program (typer and httpx) in `cli/`. It has six commands: `mb`, `rb`, `ls`, `cp`, `rm` and `head`.

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **ETag**: a fingerprint of an object's bytes (the SHA-256 hash in hex). Two objects with the same ETag have the same content.

## Install

From the repo root:

```sh
uv tool install --editable ./cli     # puts `ts2` on your PATH
ts2 --help
```

Or run it without installing anything:

```sh
uv run --project cli ts2 --help
```

Use `uv run --project cli ts2 ...` in place of `ts2` in every example below.

## Point it at a server

The CLI talks to FrontdoorService at `http://127.0.0.1:9000` unless told otherwise. Set the address with `--endpoint` or the `TS2_ENDPOINT` environment variable. The flag wins.

| Server started with | Address | What to do |
|---|---|---|
| `make run` | `http://127.0.0.1:9000` | Nothing, this is the default. |
| `make up` (Docker) | `http://127.0.0.1:9001` | `export TS2_ENDPOINT=http://127.0.0.1:9001` |

Every port is listed in the [port registry](../ports.md).

## Try it

Start the server in one terminal with `make run`. In another:

```sh
echo hello > hello.txt
head -c 10485760 /dev/urandom > big.bin       # 10 MiB, so it is stored as 3 chunks

ts2 mb demo
ts2 cp hello.txt ts2://demo/docs/hello.txt
ts2 cp big.bin ts2://demo/big.bin
ts2 ls demo
ts2 head ts2://demo/big.bin
ts2 cp ts2://demo/big.bin copy.bin
cmp big.bin copy.bin && echo same
ts2 rm ts2://demo/docs/hello.txt
ts2 rm ts2://demo/big.bin
ts2 rb demo
```

Expected output. Timestamps and ETags differ on your machine, and the ETag of `big.bin` depends on the random bytes.

```console
$ ts2 mb demo
made bucket demo
$ ts2 cp hello.txt ts2://demo/docs/hello.txt
uploaded ts2://demo/docs/hello.txt etag="5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
$ ts2 cp big.bin ts2://demo/big.bin
uploaded ts2://demo/big.bin etag="beb253d7ec0efd5fa8943f654c8c700fe1477e2ac2ffd090ca330d46fbdda58e"
$ ts2 ls demo
2026-11-22T20:11:03.491283Z      10485760  big.bin
2026-11-22T20:11:03.091444Z             6  docs/hello.txt
$ ts2 head ts2://demo/big.bin
content-length: 10485760
content-type: application/octet-stream
etag: "beb253d7ec0efd5fa8943f654c8c700fe1477e2ac2ffd090ca330d46fbdda58e"
$ ts2 cp ts2://demo/big.bin copy.bin
downloaded ts2://demo/big.bin to copy.bin
$ cmp big.bin copy.bin && echo same
same
$ ts2 rm ts2://demo/docs/hello.txt
deleted ts2://demo/docs/hello.txt
$ ts2 rm ts2://demo/big.bin
deleted ts2://demo/big.bin
$ ts2 rb demo
removed bucket demo
```

Errors go to stderr as `title: detail (request_id)` and the exit code is 1. For example, `ts2 rb demo` while the bucket still holds objects prints `Bucket not empty: bucket is not empty (<request id>)`. `ts2 head` has no response body to read, so on a missing key it only prints `HTTP 404`.

## Examples

```sh
ts2 mb photos                              # make a bucket
ts2 ls                                     # list buckets
ts2 ls photos --prefix cats/               # list keys that start with cats/
ts2 cp ./cat.jpg ts2://photos/cats/cat.jpg # upload, prints the ETag
ts2 cp ts2://photos/cats/cat.jpg ./cat.jpg # download
ts2 head ts2://photos/cats/cat.jpg         # size, content type, ETag
ts2 rm ts2://photos/cats/cat.jpg           # delete an object
ts2 rb photos                              # remove an empty bucket
ts2 --endpoint http://127.0.0.1:9001 ls    # talk to the Docker server for one command
```

## Command reference

This section is generated from the code by `typer` and checked by `cli/tests/test_docs.py`, so it cannot drift. To refresh it after changing a help string, run `TS2_UPDATE_DOCS=1 uv run --project cli pytest cli/tests/test_docs.py`.

<!-- cli-reference:start -->

### `ts2`

TS2 command line client.

**Usage**:

```console
$ ts2 [OPTIONS] COMMAND [ARGS]...
```

**Options**:

* `--endpoint <str>`: FrontdoorService base URL.  [env var: TS2_ENDPOINT; default: http://127.0.0.1:9000]
* `--help`: Show this message and exit.

**Commands**:

* `mb`: Make a bucket.
* `rb`: Remove an empty bucket.
* `ls`: List buckets, or the objects in a bucket.
* `cp`: Copy a file to or from ts2://bucket/key.
* `rm`: Delete an object.
* `head`: Show an object's size, content type and ETag.

#### `ts2 mb`

Make a bucket.

**Usage**:

```console
$ ts2 mb [OPTIONS] {bucket}
```

**Arguments**:

* `bucket <str>`: Name of the bucket to create.  [required]

**Options**:

* `--help`: Show this message and exit.

#### `ts2 rb`

Remove an empty bucket. Fails if the bucket still holds objects.

**Usage**:

```console
$ ts2 rb [OPTIONS] {bucket}
```

**Arguments**:

* `bucket <str>`: Name of the bucket to remove.  [required]

**Options**:

* `--help`: Show this message and exit.

#### `ts2 ls`

List buckets, or the objects in a bucket. Pages through large listings.

**Usage**:

```console
$ ts2 ls [OPTIONS] [bucket]
```

**Arguments**:

* `bucket <str>`: Bucket to list. Omit to list all buckets.

**Options**:

* `--prefix <str>`: Only list keys that start with this prefix.
* `--help`: Show this message and exit.

#### `ts2 cp`

Copy a file to or from ts2://bucket/key. Exactly one side is a ts2:// uri.

Uploads print the ETag. Both directions stream, so large files are fine.

**Usage**:

```console
$ ts2 cp [OPTIONS] {src} {dst}
```

**Arguments**:

* `src <str>`: Local file path, or ts2://bucket/key to download.  [required]
* `dst <str>`: ts2://bucket/key to upload to, or local file path.  [required]

**Options**:

* `--help`: Show this message and exit.

#### `ts2 rm`

Delete an object.

**Usage**:

```console
$ ts2 rm [OPTIONS] {uri}
```

**Arguments**:

* `uri <str>`: Object to delete, ts2://bucket/key.  [required]

**Options**:

* `--help`: Show this message and exit.

#### `ts2 head`

Show an object's size, content type and ETag.

**Usage**:

```console
$ ts2 head [OPTIONS] {uri}
```

**Arguments**:

* `uri <str>`: Object to inspect, ts2://bucket/key.  [required]

**Options**:

* `--help`: Show this message and exit.

<!-- cli-reference:end -->
