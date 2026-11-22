#include <grpcpp/grpcpp.h>
#include <spdlog/spdlog.h>

#include <chrono>
#include <csignal>
#include <cstdlib>
#include <string>
#include <thread>

#include "service.h"

static std::string env_or(const char* name, const char* fallback) {
  const char* v = std::getenv(name);
  return v && *v ? v : fallback;
}

int main() {
  // Block SIGTERM/SIGINT before any thread exists, so every thread inherits the mask and only the
  // dedicated waiter below receives them.
  sigset_t signals;
  sigemptyset(&signals);
  sigaddset(&signals, SIGTERM);
  sigaddset(&signals, SIGINT);
  pthread_sigmask(SIG_BLOCK, &signals, nullptr);

  // Loopback by default. The container image sets TS2_LISTEN to 0.0.0.0.
  auto listen = env_or("TS2_LISTEN", "127.0.0.1:50052");
  auto data_dir = env_or("TS2_DATA_DIR", "./data");

  try {
    ts2::ObjectIndexServiceImpl service({
        .node_id = env_or("TS2_NODE_ID", "node-1"),
        .cell = env_or("TS2_CELL", "cell-1"),
        .data_dir = data_dir,
    });

    grpc::ServerBuilder builder;
    builder.AddListeningPort(listen, grpc::InsecureServerCredentials());
    builder.RegisterService(&service);
    auto server = builder.BuildAndStart();
    if (!server) {
      spdlog::error("failed to listen on {}", listen);
      return 1;
    }
    spdlog::info("ts2-object-index listening on {}, data in {}", listen, data_dir);

    // Docker stop sends SIGTERM. Shutdown(deadline) stops accepting calls and lets in-flight ones
    // finish until the deadline, then cancels them. SQLite commits are already durable.
    std::thread waiter([&] {
      int sig = 0;
      sigwait(&signals, &sig);
      spdlog::info("signal {}, shutting down", sig);
      server->Shutdown(std::chrono::system_clock::now() + std::chrono::seconds(10));
    });
    server->Wait();
    waiter.join();
    spdlog::info("stopped");
  } catch (const std::exception& e) {
    spdlog::error("fatal: {}", e.what());
    return 1;
  }
}
