#!/bin/sh
# Prints how many chunk files node <n> holds. Usage: count_chunks.sh <CPP_DIR|compose> <n>
# CPP_DIR is the `make e2e-native` scratch dir; `compose` asks the chunk-store-<n> container.
set -e
if [ "$1" = compose ]; then
  docker compose -f deploy/compose/compose.yaml exec -T "chunk-store-$2" sh -c 'find /data -type f ! -name "*.sha256" | wc -l'
else
  find "$1/chunks-$2" -type f ! -name '*.sha256' | wc -l
fi | tr -d ' '
