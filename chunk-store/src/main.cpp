#include <grpcpp/grpcpp.h>
#include <spdlog/spdlog.h>

#include <chrono>
#include <csignal>
#include <cstdlib>
#include <string>
#include <thread>

#include "disk_store.h"
#include "service.h"

static std::string env_or(const char* name, const char* fallback) {
  const char* v = std::getenv(name);
  return v && *v ? v : fallback;
}

int main() {
  // Loopback by default. The container image sets TS2_LISTEN to 0.0.0.0.
  auto listen = env_or("TS2_LISTEN", "127.0.0.1:50051");

  ts2::DiskStore store(env_or("TS2_DATA_DIR", "./data"));
  if (auto st = store.open(); !st.ok()) {
    spdlog::error("cannot open data dir {}: {}", store.root().string(), st.message);
    return 1;
  }

  ts2::ChunkStoreServiceImpl service(
      {
          .node_id = env_or("TS2_NODE_ID", "node-1"),
          .cell = env_or("TS2_CELL", "cell-1"),
          .failure_domain = env_or("TS2_FAILURE_DOMAIN", "local"),
          .require_token = std::getenv("TS2_REQUIRE_TOKEN") != nullptr,
      },
      store);

  // Block SIGINT/SIGTERM in every thread (gRPC's threads inherit this mask), and let one thread
  // wait for them with sigwait. That avoids doing real work inside a signal handler.
  sigset_t signals;
  sigemptyset(&signals);
  sigaddset(&signals, SIGINT);
  sigaddset(&signals, SIGTERM);
  pthread_sigmask(SIG_BLOCK, &signals, nullptr);

  grpc::ServerBuilder builder;
  ts2::apply_server_limits(builder);
  builder.AddListeningPort(listen, grpc::InsecureServerCredentials());
  builder.RegisterService(&service);
  auto server = builder.BuildAndStart();
  if (!server) {
    spdlog::error("failed to listen on {}", listen);
    return 1;
  }
  spdlog::info("ts2-chunk-store listening on {}, data in {}", listen, store.root().string());

  std::thread shutdown_thread([&] {
    int sig = 0;
    sigwait(&signals, &sig);
    spdlog::info("signal {}, draining for up to 10 s", sig);
    // In-flight RPCs get 10 s to finish, then they are cancelled (their tmp files are cleaned up).
    server->Shutdown(std::chrono::system_clock::now() + std::chrono::seconds(10));
  });
  server->Wait();
  // Wait() can only return after Shutdown(), so the thread is done or about to be.
  shutdown_thread.join();
}
