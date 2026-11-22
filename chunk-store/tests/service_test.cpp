#include <catch2/catch_test_macros.hpp>
#include <grpcpp/grpcpp.h>

#include "service.h"

TEST_CASE("NodeInfo reports the configured identity") {
  ts2::ChunkStoreServiceImpl service({"n1", "c1", "rack-a", "."});
  grpc::ServerBuilder builder;
  builder.RegisterService(&service);
  auto server = builder.BuildAndStart();
  auto stub = ts2::v1::ChunkStoreService::NewStub(server->InProcessChannel({}));

  grpc::ClientContext ctx;
  ts2::v1::ChunkStoreNodeInfoRequest req;
  ts2::v1::ChunkStoreNodeInfoResponse resp;
  auto status = stub->NodeInfo(&ctx, req, &resp);

  REQUIRE(status.ok());
  CHECK(resp.node_id() == "n1");
  CHECK(resp.cell() == "c1");
  CHECK(resp.failure_domain() == "rack-a");
  CHECK(resp.healthy());
}
