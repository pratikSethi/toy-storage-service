# Getting started

The shortest path from nothing to a stored file.

1. **Get the endpoint.** TS2 is a learning project, so the endpoint is whatever you run: `http://127.0.0.1:9000` for `make run` and `http://127.0.0.1:9001` for `make up`. See [setup](../setup.md) to start it.
2. **Install the CLI** and point it at the endpoint. Steps are in the [CLI reference](../cli/index.md#install).

    ```sh
    export TS2_ENDPOINT=http://127.0.0.1:9000
    ```

3. **Make a bucket, upload, list, download, delete.**

    ```sh
    ts2 mb demo
    ts2 cp hello.txt ts2://demo/docs/hello.txt
    ts2 ls demo
    ts2 cp ts2://demo/docs/hello.txt copy.txt
    ts2 rm ts2://demo/docs/hello.txt
    ts2 rb demo
    ```

Expected output and every option are in the [CLI reference](../cli/index.md). Prefer HTTP? Every command has a curl form in [capabilities](capabilities.md) and a full description in the [API reference](../api/index.md).

Before you rely on it, read the [limits](limits.md) and the [service levels](service-levels.md).
