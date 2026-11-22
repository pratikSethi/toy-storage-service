#!/bin/sh
# Restarts chunk node-1, the first chunk-store that `make e2e-native` started, on the same port and data dir.
# Usage: restart_chunk_store_native.sh <CPP_DIR>   (CPP_DIR holds the pids file, logs and data)
# Returns once the new process is started; the e2e test waits a moment for it to listen.
set -e
dir=$1
old=$(sed -n 1p "$dir/pids")
kill "$old"
while kill -0 "$old" 2>/dev/null; do sleep 0.1; done
TS2_LISTEN=127.0.0.1:50151 TS2_NODE_ID=node-1 TS2_DATA_DIR="$dir/chunks-1" \
  chunk-store/build/debug/ts2-chunk-store >> "$dir/chunk-store-1.log" 2>&1 &
# make's cleanup kills whatever is in pids, so record the new pid
{ echo "$!"; sed 1d "$dir/pids"; } > "$dir/pids.new" && mv "$dir/pids.new" "$dir/pids"
