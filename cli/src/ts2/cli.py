import mimetypes
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Annotated, Any
from urllib.parse import quote

import httpx
import typer

app = typer.Typer(no_args_is_help=True, add_completion=False, help="TS2 command line client.")

SCHEME = "ts2://"
CHUNK = 1 << 20


def fail(message: str) -> typer.Exit:
    typer.echo(message, err=True)
    return typer.Exit(1)


def check(resp: httpx.Response) -> httpx.Response:
    """Return the response, or print the problem+json and exit 1."""
    if resp.is_success:
        return resp
    resp.read()
    try:
        p: dict[str, Any] = resp.json()
        msg = f"{p['title']}: {p['detail']} ({p['request_id']})"
    except (ValueError, KeyError):
        msg = f"HTTP {resp.status_code}: {resp.text[:200]}"
    raise fail(msg)


@app.callback()
def main(
    ctx: typer.Context,
    endpoint: Annotated[
        str, typer.Option(envvar="TS2_ENDPOINT", help="FrontdoorService base URL.")
    ] = "http://127.0.0.1:9000",
) -> None:
    client = httpx.Client(base_url=endpoint.rstrip("/"), timeout=httpx.Timeout(30, read=None))
    ctx.call_on_close(client.close)
    ctx.obj = client


def parse_uri(uri: str) -> tuple[str, str]:
    """Split ts2://bucket/key into (bucket, key)."""
    bucket, _, key = uri.removeprefix(SCHEME).partition("/")
    if not bucket or not key:
        raise fail(f"expected ts2://bucket/key, got {uri}")
    return bucket, key


def obj_path(uri: str) -> str:
    bucket, key = parse_uri(uri)
    return f"/v1/buckets/{quote(bucket)}/objects/{quote(key)}"


def request(ctx: typer.Context, method: str, path: str, **kw: Any) -> httpx.Response:
    try:
        return check(ctx.obj.request(method, path, **kw))
    except httpx.HTTPError as e:
        raise fail(f"request failed: {e}") from e


@app.command()
def mb(ctx: typer.Context, bucket: str) -> None:
    """Make a bucket."""
    request(ctx, "POST", "/v1/buckets", json={"name": bucket})
    typer.echo(f"made bucket {bucket}")


@app.command()
def rb(ctx: typer.Context, bucket: str) -> None:
    """Remove an empty bucket."""
    request(ctx, "DELETE", f"/v1/buckets/{quote(bucket)}")
    typer.echo(f"removed bucket {bucket}")


@app.command()
def ls(
    ctx: typer.Context,
    bucket: Annotated[str | None, typer.Argument()] = None,
    prefix: str = "",
) -> None:
    """List buckets, or the objects in a bucket."""
    if bucket is None:
        for b in request(ctx, "GET", "/v1/buckets").json()["buckets"]:
            typer.echo(f"{b['created_at']}  {b['name']}")
        return
    params: dict[str, Any] = {"prefix": prefix}
    while True:
        page = request(ctx, "GET", f"/v1/buckets/{quote(bucket)}/objects", params=params).json()
        for o in page["objects"]:
            typer.echo(f"{o['last_modified']}  {o['size']:>12}  {o['key']}")
        if not page["is_truncated"]:
            return
        params["start_after"] = page["next_start_after"]


def read_file(path: Path) -> Iterator[bytes]:
    with path.open("rb") as f:
        while block := f.read(CHUNK):
            yield block


@contextmanager
def stream(ctx: typer.Context, method: str, path: str) -> Iterator[httpx.Response]:
    try:
        with ctx.obj.stream(method, path) as resp:
            yield check(resp)
    except httpx.HTTPError as e:
        raise fail(f"request failed: {e}") from e


@app.command()
def cp(ctx: typer.Context, src: str, dst: str) -> None:
    """Copy a local file to ts2://bucket/key, or the reverse."""
    up, down = dst.startswith(SCHEME), src.startswith(SCHEME)
    if up == down:
        raise fail("exactly one of src and dst must be a ts2:// uri")
    if up:
        path = Path(src)
        if not path.is_file():
            raise fail(f"not a file: {src}")
        ctype = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
        resp = request(
            ctx, "PUT", obj_path(dst), content=read_file(path), headers={"content-type": ctype}
        )
        typer.echo(f"uploaded {dst} etag={resp.headers.get('etag')}")
        return
    target = Path(dst)
    with stream(ctx, "GET", obj_path(src)) as resp, target.open("wb") as f:
        for block in resp.iter_bytes(CHUNK):
            f.write(block)
    typer.echo(f"downloaded {src} to {dst}")


@app.command()
def rm(ctx: typer.Context, uri: str) -> None:
    """Delete an object."""
    request(ctx, "DELETE", obj_path(uri))
    typer.echo(f"deleted {uri}")


@app.command()
def head(ctx: typer.Context, uri: str) -> None:
    """Show an object's headers."""
    resp = request(ctx, "HEAD", obj_path(uri))
    for name in ("content-length", "content-type", "etag", "last-modified"):
        if name in resp.headers:
            typer.echo(f"{name}: {resp.headers[name]}")
