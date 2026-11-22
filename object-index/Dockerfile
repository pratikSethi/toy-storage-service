# Build from the repo root so ../proto is in the context:
#   docker build -f object-index/Dockerfile -t ts2-object-index .
FROM ubuntu:24.04 AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential cmake ninja-build git curl zip unzip tar pkg-config ca-certificates \
    && apt-get clean
# vcpkg has no prebuilt tools for arm64 Linux, so use the system ones.
ENV VCPKG_FORCE_SYSTEM_BINARIES=1 VCPKG_ROOT=/opt/vcpkg
RUN git clone --depth 1 https://github.com/microsoft/vcpkg ${VCPKG_ROOT} \
    && ${VCPKG_ROOT}/bootstrap-vcpkg.sh -disableMetrics
WORKDIR /src
# Dependencies first so source edits do not rebuild gRPC.
COPY object-index/vcpkg.json object-index/vcpkg.json
RUN cd object-index && ${VCPKG_ROOT}/vcpkg install
COPY proto proto
COPY object-index object-index
RUN cmake -S object-index -B /out -G Ninja -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_TOOLCHAIN_FILE=${VCPKG_ROOT}/scripts/buildsystems/vcpkg.cmake \
    && cmake --build /out --target ts2-object-index

FROM ubuntu:24.04
RUN useradd --system --create-home --home-dir /data ts2
COPY --from=build /out/ts2-object-index /usr/local/bin/ts2-object-index
USER ts2
ENV TS2_LISTEN=0.0.0.0:50052 TS2_DATA_DIR=/data
EXPOSE 50052
ENTRYPOINT ["ts2-object-index"]
