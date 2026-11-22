#include <grpcpp/grpcpp.h>
#include <spdlog/spdlog.h>

#include <cstdlib>
#include <string>

#include "service.h"

static std::string env_or(const char* name, const char* fallback) {
  const char* v = std::getenv(name);
  return v && *v ? v : fallback;
}

int main() {
  // Loopback by default. The container image sets TS2_LISTEN to 0.0.0.0.
  auto listen = env_or("TS2_LISTEN", "127.0.0.1:50051");
  ts2::ChunkStoreServiceImpl service({
      .node_id = env_or("TS2_NODE_ID", "node-1"),
      .cell = env_or("TS2_CELL", "cell-1"),
      .failure_domain = env_or("TS2_FAILURE_DOMAIN", "local"),
      .data_dir = env_or("TS2_DATA_DIR", "."),
  });

  grpc::ServerBuilder builder;
  builder.AddListeningPort(listen, grpc::InsecureServerCredentials());
  builder.RegisterService(&service);
  auto server = builder.BuildAndStart();
  if (!server) {
    spdlog::error("failed to listen on {}", listen);
    return 1;
  }
  spdlog::info("ts2-chunk-store listening on {}", listen);
  server->Wait();
}
