#pragma once

#include <string>
#include <utility>

#include "ts2/v1/chunk_store.grpc.pb.h"

namespace ts2 {

struct NodeConfig {
  std::string node_id;
  std::string cell;
  std::string failure_domain;
  std::string data_dir;
};

// Skeleton: only NodeInfo is implemented, every other RPC returns UNIMPLEMENTED.
class ChunkStoreServiceImpl final : public v1::ChunkStoreService::Service {
 public:
  explicit ChunkStoreServiceImpl(NodeConfig config) : config_(std::move(config)) {}

  grpc::Status NodeInfo(grpc::ServerContext*, const v1::ChunkStoreNodeInfoRequest*,
                        v1::ChunkStoreNodeInfoResponse* response) override;

 private:
  NodeConfig config_;
};

}  // namespace ts2
