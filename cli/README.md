# ts2 CLI

Command line client for [FrontdoorService](../frontdoor/README.md). Python, typer and httpx.

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **ETag**: a fingerprint of an object's bytes (the SHA-256 hash in hex). Two objects with the same ETag have the same content.

## Install

```sh
uv tool install --editable ./cli     # puts `ts2` on your PATH
uv run --project cli ts2 --help      # or run it without installing
```

## Usage

The endpoint comes from `--endpoint` or `TS2_ENDPOINT` and defaults to `http://127.0.0.1:9000`.

```sh
ts2 mb demo
ts2 cp ./big.bin ts2://demo/big.bin      # prints the ETag
ts2 ls                                   # buckets
ts2 ls demo --prefix docs/               # objects
ts2 head ts2://demo/big.bin
ts2 cp ts2://demo/big.bin ./copy.bin
ts2 rm ts2://demo/big.bin
ts2 rb demo
```

Errors are printed as `title: detail (request_id)` on stderr with exit code 1. `head` has no response body, so it only reports the HTTP status.

## Develop

```sh
cd cli && uv sync && uv run ruff check . && uv run ruff format .
```

End-to-end tests live in [`tests/e2e`](../tests/e2e) and run with `make e2e`.
