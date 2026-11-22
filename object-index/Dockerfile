# Build from the repo root so ../proto is in the context:
#   docker build -f object-index/Dockerfile -t ts2-object-index .
FROM ubuntu:24.04 AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential cmake ninja-build git curl zip unzip tar pkg-config ca-certificates perl python3 \
    && apt-get clean
# vcpkg has no prebuilt tools for arm64 Linux, so use the system ones. Full clone: the manifest
# builtin-baseline commit must exist locally.
ENV VCPKG_FORCE_SYSTEM_BINARIES=1 VCPKG_ROOT=/opt/vcpkg
RUN git clone https://github.com/microsoft/vcpkg ${VCPKG_ROOT} \
    && ${VCPKG_ROOT}/bootstrap-vcpkg.sh -disableMetrics
WORKDIR /src
# Dependencies first so source edits do not rebuild gRPC. One root vcpkg.json serves both services, and both
# Dockerfiles have the same instructions up to here, so the second image reuses the first one's layer.
# The cache mount keeps vcpkg's binary cache between builds, so changing vcpkg.json rebuilds only the ports
# that changed instead of all of gRPC. It is not part of the image.
COPY vcpkg.json vcpkg.json
RUN --mount=type=cache,target=/root/.cache/vcpkg \
    ${VCPKG_ROOT}/vcpkg install --x-install-root=/src/vcpkg_installed
COPY cmake cmake
COPY proto proto
COPY object-index object-index
RUN cmake -S object-index -B /out -G Ninja -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_TOOLCHAIN_FILE=${VCPKG_ROOT}/scripts/buildsystems/vcpkg.cmake \
      -DVCPKG_MANIFEST_MODE=OFF -DVCPKG_INSTALLED_DIR=/src/vcpkg_installed \
    && cmake --build /out --target ts2-object-index

FROM ubuntu:24.04
RUN useradd --system --create-home --home-dir /data ts2
COPY --from=build /out/ts2-object-index /usr/local/bin/ts2-object-index
USER ts2
ENV TS2_LISTEN=0.0.0.0:50052 TS2_DATA_DIR=/data
EXPOSE 50052
ENTRYPOINT ["ts2-object-index"]
