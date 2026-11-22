"""Smoke test: drives the ts2 CLI against a running FrontdoorService (TS2_ENDPOINT)."""

import os
import subprocess
import sys
import uuid
from pathlib import Path

import httpx
import pytest

ENDPOINT = os.environ.get("TS2_ENDPOINT", "http://127.0.0.1:9000")
BUCKET = f"e2e-{uuid.uuid4().hex[:8]}"
URI = f"ts2://{BUCKET}"
MIB = 1024 * 1024


@pytest.fixture(scope="module", autouse=True)
def require_frontdoor():
    try:
        httpx.get(f"{ENDPOINT}/healthz", timeout=2).raise_for_status()
    except httpx.HTTPError:
        pytest.skip(
            f"no healthy frontdoor at {ENDPOINT}/healthz (start it with `make run`)"
        )


def ts2(*args: str | Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, "-m", "ts2", "--endpoint", ENDPOINT, *map(str, args)],
        check=False,
        capture_output=True,
        text=True,
    )


def test_mb():
    r = ts2("mb", BUCKET)
    assert r.returncode == 0, r.stderr
    assert BUCKET in ts2("ls").stdout


@pytest.fixture(scope="module")
def data(tmp_path_factory) -> Path:
    d = tmp_path_factory.mktemp("data")
    (d / "hello.txt").write_text("hello ts2\n")
    (d / "big.bin").write_bytes(os.urandom(10 * MIB))  # 3 chunks of 4 MiB
    return d


def test_cp_up_small(data):
    r = ts2("cp", data / "hello.txt", f"{URI}/docs/hello.txt")
    assert r.returncode == 0, r.stderr
    assert "etag=" in r.stdout


def test_cp_up_big(data):
    r = ts2("cp", data / "big.bin", f"{URI}/big.bin")
    assert r.returncode == 0, r.stderr
    assert "etag=" in r.stdout


def test_ls_prefix():
    r = ts2("ls", BUCKET, "--prefix", "docs/")
    assert r.returncode == 0, r.stderr
    assert "docs/hello.txt" in r.stdout
    assert "big.bin" not in r.stdout


def test_cp_down_small(data, tmp_path):
    out = tmp_path / "hello.txt"
    assert ts2("cp", f"{URI}/docs/hello.txt", out).returncode == 0
    assert out.read_bytes() == (data / "hello.txt").read_bytes()


def test_cp_down_big(data, tmp_path):
    out = tmp_path / "big.bin"
    assert ts2("cp", f"{URI}/big.bin", out).returncode == 0
    assert out.read_bytes() == (data / "big.bin").read_bytes()


def test_head():
    r = ts2("head", f"{URI}/big.bin")
    assert r.returncode == 0, r.stderr
    assert f"content-length: {10 * MIB}" in r.stdout


def test_error_prints_problem_title(tmp_path):
    r = ts2("cp", f"{URI}/nope", tmp_path / "nope")
    assert r.returncode == 1
    assert (
        r.stderr.strip() and "HTTP " not in r.stderr
    )  # parsed problem+json, not raw status


def test_head_missing_exits_1():
    # HEAD responses have no body, so there is no problem title to print
    assert ts2("head", f"{URI}/nope").returncode == 1


def test_rm():
    assert ts2("rm", f"{URI}/docs/hello.txt").returncode == 0
    assert ts2("rm", f"{URI}/big.bin").returncode == 0


def test_rb():
    r = ts2("rb", BUCKET)
    assert r.returncode == 0, r.stderr
    assert BUCKET not in ts2("ls").stdout
