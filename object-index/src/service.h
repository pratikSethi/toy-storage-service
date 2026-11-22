#pragma once

#include <string>
#include <utility>

#include "ts2/v1/object_index.grpc.pb.h"

namespace ts2 {

struct NodeConfig {
  std::string node_id;
  std::string cell;
  std::string data_dir;  // SQLite file lives here later
};

// Skeleton: only NodeInfo is implemented, every other RPC returns UNIMPLEMENTED.
class ObjectIndexServiceImpl final : public v1::ObjectIndexService::Service {
 public:
  explicit ObjectIndexServiceImpl(NodeConfig config) : config_(std::move(config)) {}

  grpc::Status NodeInfo(grpc::ServerContext*, const v1::NodeInfoRequest*,
                        v1::NodeInfoResponse* response) override;

 private:
  NodeConfig config_;
};

}  // namespace ts2
