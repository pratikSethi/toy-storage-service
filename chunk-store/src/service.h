#pragma once

#include <grpcpp/grpcpp.h>

#include <string>
#include <utility>

#include "disk_store.h"
#include "ts2/v1/chunk_store.grpc.pb.h"

namespace ts2 {

// No message carries more than 1 MiB of chunk data. The server accepts 1 KiB more for framing.
constexpr size_t kMaxFrameBytes = 1024 * 1024;
constexpr int kMaxMessageBytes = static_cast<int>(kMaxFrameBytes) + 1024;

// Sets the message size limits from the contract. Used by main and by the tests.
void apply_server_limits(grpc::ServerBuilder& builder);

struct NodeConfig {
  std::string node_id;
  std::string cell;
  std::string failure_domain;
  // TS2_REQUIRE_TOKEN. Off in v0.1. When on, a PutChunk without write_token is PERMISSION_DENIED.
  // A non-empty token is not validated yet; that arrives with tokens in v0.2.
  bool require_token = false;
};

class ChunkStoreServiceImpl final : public v1::ChunkStoreService::Service {
 public:
  // The store must outlive the service and must already be open().
  ChunkStoreServiceImpl(NodeConfig config, DiskStore& store) : config_(std::move(config)), store_(store) {}

  grpc::Status PutChunk(grpc::ServerContext* context, grpc::ServerReader<v1::PutChunkRequest>* reader,
                        v1::PutChunkResponse* response) override;
  grpc::Status GetChunk(grpc::ServerContext* context, const v1::GetChunkRequest* request,
                        grpc::ServerWriter<v1::GetChunkResponse>* writer) override;
  grpc::Status DeleteChunk(grpc::ServerContext*, const v1::DeleteChunkRequest* request,
                           v1::DeleteChunkResponse*) override;
  grpc::Status ListChunks(grpc::ServerContext*, const v1::ListChunksRequest* request,
                          v1::ListChunksResponse* response) override;
  grpc::Status NodeInfo(grpc::ServerContext*, const v1::ChunkStoreNodeInfoRequest*,
                        v1::ChunkStoreNodeInfoResponse* response) override;

 private:
  NodeConfig config_;
  DiskStore& store_;
};

}  // namespace ts2
