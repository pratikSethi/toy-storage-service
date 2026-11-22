#include "service.h"

#include <filesystem>

namespace ts2 {

grpc::Status ChunkStoreServiceImpl::NodeInfo(grpc::ServerContext*, const v1::ChunkStoreNodeInfoRequest*,
                                             v1::ChunkStoreNodeInfoResponse* response) {
  std::error_code ec;
  auto space = std::filesystem::space(config_.data_dir, ec);
  response->set_node_id(config_.node_id);
  response->set_cell(config_.cell);
  response->set_failure_domain(config_.failure_domain);
  response->set_bytes_free(ec ? 0 : space.available);
  response->set_bytes_total(ec ? 0 : space.capacity);
  response->set_healthy(!ec);
  response->set_version("0.0.1");
  return grpc::Status::OK;
}

}  // namespace ts2
