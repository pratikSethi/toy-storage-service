# Development setup

What to install to build, test and run TS2, and why. macOS first, Ubuntu/Debian second. Versions are what the author's Mac (Apple Silicon) had when this page was written; nothing here pins them.

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **gRPC**: a way for services to call each other over the network (a remote procedure call, RPC), sending Protobuf messages over HTTP/2. The TS2 services talk to each other this way.
> - **SQLite**: a small database that lives in one file.

## What needs what

| You want to | You need |
|---|---|
| Run FrontdoorService, `make test`, `make run` | Rust toolchain |
| Use the `ts2` CLI, run e2e tests, serve these docs | uv |
| Run the compose demo (`make up`, `make up-obs`) | Docker Desktop or Podman |
| Build ChunkStoreService or ObjectIndexService | Xcode CLT, CMake, Ninja, vcpkg, and the Homebrew packages below |

You can work on the Rust side, the CLI and the docs without any of the C++ tools.

Ports used by `make run`, `make up`, `make e2e`, `make docs` and the C++ services are listed in the [port registry](ports.md).

## Tools

| Tool | Why | Needed by | macOS | Ubuntu/Debian |
|---|---|---|---|---|
| Rust (rustup, cargo, clippy, rustfmt) | Compiles and tests FrontdoorService and `ts2-core`. | `frontdoor/`, `ts2-core/` | `brew install rustup` then `rustup-init`, or the installer at rustup.rs | `curl https://sh.rustup.rs -sSf \| sh` |
| uv | Runs the Python parts without managing venvs by hand: `uv sync --project cli`, `uv run pytest`, `uv run mkdocs`. Fetches Python 3.12+ if none is present. | `cli/`, `tests/e2e/`, `make docs` | `brew install uv` | `curl -LsSf https://astral.sh/uv/install.sh \| sh` |
| Docker Desktop or Podman | Builds the service images and runs the compose demo and the observability profile (Grafana, collector). Podman works with the same Dockerfiles and `compose.yaml`. | `deploy/compose/`, `make up`, `make up-obs` | Docker Desktop, or `brew install podman` | `sudo apt install docker.io docker-compose-v2` or `podman` |
| Xcode Command Line Tools | Provides `clang++` (C++20) and `git`. | `chunk-store/`, `object-index/` | `xcode-select --install` | `sudo apt install build-essential` |
| CMake | Configures the C++ builds through presets in `CMakePresets.json`. | `chunk-store/`, `object-index/` | `brew install cmake` | `sudo apt install cmake` |
| Ninja | The build backend the presets select. Faster than Make. | `chunk-store/`, `object-index/` | `brew install ninja` | `sudo apt install ninja-build` |
| vcpkg | C++ package manager. Builds gRPC, Protobuf, Catch2, spdlog and SQLite from source at pinned versions, the same way on the Mac, in CI and in Docker. | `chunk-store/`, `object-index/` | see [vcpkg](#vcpkg) | same |
| pkgconf | Provides `pkg-config`, which vcpkg's builds use to find libraries. The first vcpkg build failed without it. | vcpkg builds | `brew install pkgconf` | `sudo apt install pkg-config` |
| autoconf, automake, libtool | Autotools. Some gRPC dependencies build with them. | vcpkg builds | `brew install autoconf automake libtool` | `sudo apt install autoconf automake libtool` |

Install the Homebrew packages in one go:

```sh
brew install cmake ninja uv pkgconf autoconf automake libtool
```

Homebrew cannot run inside a sandboxed agent chat, because its nested macOS sandbox is refused. Run it in your own terminal.

## vcpkg

C++ has no built-in package manager. gRPC is too large, and needs `protoc` code generation, to vendor into the repo the way DuckDB vendors its small dependencies. vcpkg is free and MIT licensed. Both C++ services share one root `vcpkg.json` with a pinned `builtin-baseline`, so every machine, CI and Docker build the same versions and share one binary cache. The CMake presets load vcpkg through `$VCPKG_ROOT` and point `VCPKG_MANIFEST_DIR` at the repo root. Use a full clone (not `--depth 1`): vcpkg needs the baseline commit locally. To bump versions, change the baseline in `vcpkg.json` (a cold rebuild follows).

### vcpkg in the Docker images

The two C++ Dockerfiles install vcpkg dependencies from the root `vcpkg.json` before copying any source, so a source edit does not rebuild gRPC. Two things keep a rebuild cheap:

- Both Dockerfiles have identical instructions up to that step, so the second image reuses the first image's layer.
- The install step runs with `--mount=type=cache,target=/root/.cache/vcpkg`, which keeps vcpkg's binary cache across builds. When `vcpkg.json` changes, only the changed ports rebuild.

Chosen over a shared base image because it needs no extra image to build first and no ordering between the two `docker build` calls. A cold build (empty Docker cache) still compiles gRPC once, which takes tens of minutes. Build the images one after another (`make up` does), because two cold builds in parallel compile gRPC twice at the same time. `docker builder prune` clears the cache.

Clone and bootstrap it. `-disableMetrics` turns off telemetry.

```sh
git clone https://github.com/microsoft/vcpkg ~/vcpkg
~/vcpkg/bootstrap-vcpkg.sh -disableMetrics
```

Set `VCPKG_ROOT`. On the author's Mac this lives in its own file, `~/.config/shell/vcpkg.sh`, sourced from `~/.zshrc` by a labelled line:

```sh
# ~/.config/shell/vcpkg.sh
export VCPKG_ROOT="$HOME/vcpkg"
export PATH="$VCPKG_ROOT:$PATH"
```

```sh
# ~/.zshrc
# vcpkg (C++ package manager): edit ~/.config/shell/vcpkg.sh
[ -f ~/.config/shell/vcpkg.sh ] && source ~/.config/shell/vcpkg.sh
```

The first C++ build compiles gRPC from source and takes roughly 20 to 40 minutes. vcpkg caches the result, so later builds are fast.

## Build the C++ services

```sh
cd chunk-store            # or object-index
cmake --preset debug      # UBSan on
cmake --build --preset debug
ctest --preset debug
```

There is also an `asan` preset (`-fsanitize=address,undefined`). On the author's Mac (vcpkg gRPC 1.84, protobuf 36) the `asan` build links but the NodeInfo test segfaults inside protobuf serialization, with and without UBSan. The cause is not found, so `asan` is not in CI yet. Try it on Linux first.

Both services generate code from `proto/` through the shared `cmake/ts2_proto.cmake`.

## protoc for Rust

The `ts2-proto` crate compiles both protos with `tonic-prost-build`. It gets `protoc` from the `protoc-bin-vendored` build dependency, so no system `protoc` is needed (not on the Mac, in CI or in the frontdoor Docker build). Set `PROTOC` to use your own binary instead, for example the one vcpkg builds under `chunk-store/build/debug/vcpkg_installed/<triplet>/tools/protobuf/`.

```sh
cargo build -p ts2-proto
```

## Check your setup

```sh
rustc --version && cargo --version
cargo clippy --version && rustfmt --version
uv --version
docker --version            # or: podman --version
clang++ --version
cmake --version
ninja --version
pkg-config --version
brew list --versions pkgconf autoconf automake libtool   # macOS
echo $VCPKG_ROOT && ~/vcpkg/vcpkg version
```

Seen on the author's Mac: cargo and rustc 1.98.0, rustup 1.29.0, uv 0.12.13, Docker 29.7.2, Apple clang 21.0.0, CMake 4.4.5, Ninja 1.13.2, pkgconf 3.0.11, autoconf 2.73, automake 1.20, libtool 2.6.2, vcpkg 2026-10-29.
