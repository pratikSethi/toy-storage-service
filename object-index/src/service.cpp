#include "service.h"

namespace ts2 {

grpc::Status ObjectIndexServiceImpl::NodeInfo(grpc::ServerContext*, const v1::ObjectIndexNodeInfoRequest*,
                                              v1::ObjectIndexNodeInfoResponse* response) {
  response->set_node_id(config_.node_id);
  response->set_cell(config_.cell);
  response->set_healthy(true);
  response->set_version("0.0.1");
  return grpc::Status::OK;
}

}  // namespace ts2
