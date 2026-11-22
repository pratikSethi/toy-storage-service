#include <catch2/catch_test_macros.hpp>
#include <grpcpp/grpcpp.h>

#include "service.h"

TEST_CASE("NodeInfo reports the configured identity") {
  ts2::ObjectIndexServiceImpl service({"n1", "c1", "."});
  grpc::ServerBuilder builder;
  builder.RegisterService(&service);
  auto server = builder.BuildAndStart();
  auto stub = ts2::v1::ObjectIndexService::NewStub(server->InProcessChannel({}));

  grpc::ClientContext ctx;
  ts2::v1::ObjectIndexNodeInfoRequest req;
  ts2::v1::ObjectIndexNodeInfoResponse resp;
  auto status = stub->NodeInfo(&ctx, req, &resp);

  REQUIRE(status.ok());
  CHECK(resp.node_id() == "n1");
  CHECK(resp.cell() == "c1");
  CHECK(resp.healthy());
}
