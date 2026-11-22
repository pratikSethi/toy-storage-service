// Service-level tests: the real gRPC service over an in-process channel. Behaviour is covered in
// sqlite_index_test.cpp; these check the wire mapping (status codes, field plumbing).
#include <sqlite3.h>

#include <catch2/catch_test_macros.hpp>
#include <grpcpp/grpcpp.h>
#include <memory>

#include "service.h"
#include "test_util.h"

namespace v1 = ts2::v1;

namespace {

struct ServiceFixture {
  TempDir dir;
  ts2::ObjectIndexServiceImpl service{{"n1", "c1", (dir.path() / "data").string()}};
  std::unique_ptr<grpc::Server> server;
  std::unique_ptr<v1::ObjectIndexService::Stub> stub;

  ServiceFixture() {
    grpc::ServerBuilder builder;
    builder.RegisterService(&service);
    server = builder.BuildAndStart();
    stub = v1::ObjectIndexService::NewStub(server->InProcessChannel({}));
  }

  void create_bucket(const std::string& name) {
    grpc::ClientContext ctx;
    v1::CreateBucketRequest req;
    req.set_name(name);
    v1::Bucket resp;
    REQUIRE(stub->CreateBucket(&ctx, req, &resp).ok());
  }

  grpc::Status put(const v1::Manifest& m, v1::PutManifestResponse* resp) {
    grpc::ClientContext ctx;
    v1::PutManifestRequest req;
    *req.mutable_manifest() = m;
    return stub->PutManifest(&ctx, req, resp);
  }

  grpc::Status get(const std::string& bucket, const std::string& key, v1::Manifest* resp) {
    grpc::ClientContext ctx;
    v1::GetManifestRequest req;
    req.set_bucket(bucket);
    req.set_key(key);
    return stub->GetManifest(&ctx, req, resp);
  }
};

}  // namespace

TEST_CASE("NodeInfo reports the configured identity", "[service]") {
  ServiceFixture f;
  grpc::ClientContext ctx;
  v1::ObjectIndexNodeInfoRequest req;
  v1::ObjectIndexNodeInfoResponse resp;
  REQUIRE(f.stub->NodeInfo(&ctx, req, &resp).ok());
  CHECK(resp.node_id() == "n1");
  CHECK(resp.cell() == "c1");
  CHECK(resp.healthy());
}

TEST_CASE("bucket RPCs map errors to status codes", "[service]") {
  ServiceFixture f;
  f.create_bucket("photos");

  {
    grpc::ClientContext ctx;
    v1::CreateBucketRequest req;
    req.set_name("photos");
    v1::Bucket resp;
    CHECK(f.stub->CreateBucket(&ctx, req, &resp).error_code() == grpc::StatusCode::ALREADY_EXISTS);
    req.set_name("Bad_Name");
    grpc::ClientContext ctx2;
    CHECK(f.stub->CreateBucket(&ctx2, req, &resp).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
  }
  {
    grpc::ClientContext ctx;
    v1::GetBucketRequest req;
    req.set_name("nope");
    v1::Bucket resp;
    CHECK(f.stub->GetBucket(&ctx, req, &resp).error_code() == grpc::StatusCode::NOT_FOUND);
  }
  {
    grpc::ClientContext ctx;
    v1::ListBucketsRequest req;
    v1::ListBucketsResponse resp;
    REQUIRE(f.stub->ListBuckets(&ctx, req, &resp).ok());
    REQUIRE(resp.buckets_size() == 1);
    CHECK(resp.buckets(0).name() == "photos");
  }
}

TEST_CASE("non-empty bucket delete is FAILED_PRECONDITION, then succeeds once empty", "[service]") {
  ServiceFixture f;
  f.create_bucket("photos");
  v1::PutManifestResponse put;
  REQUIRE(f.put(inline_manifest("photos", "k"), &put).ok());

  v1::DeleteBucketRequest del_bucket;
  del_bucket.set_name("photos");
  v1::DeleteBucketResponse del_resp;
  {
    grpc::ClientContext ctx;
    CHECK(f.stub->DeleteBucket(&ctx, del_bucket, &del_resp).error_code() == grpc::StatusCode::FAILED_PRECONDITION);
  }
  {
    grpc::ClientContext ctx;
    v1::DeleteManifestRequest req;
    req.set_bucket("photos");
    req.set_key("k");
    v1::Manifest deleted;
    REQUIRE(f.stub->DeleteManifest(&ctx, req, &deleted).ok());
    CHECK(deleted.key() == "k");
  }
  grpc::ClientContext ctx;
  CHECK(f.stub->DeleteBucket(&ctx, del_bucket, &del_resp).ok());
}

TEST_CASE("put returns the stored manifest and previous; get round trips", "[service]") {
  ServiceFixture f;
  f.create_bucket("photos");

  v1::PutManifestResponse first;
  REQUIRE(f.put(inline_manifest("photos", "k", "one"), &first).ok());
  CHECK(first.manifest().version() == 1);
  CHECK_FALSE(first.has_previous());

  v1::PutManifestResponse second;
  REQUIRE(f.put(chunked_manifest("photos", "k", 1, 2), &second).ok());
  CHECK(second.manifest().version() == 2);
  REQUIRE(second.has_previous());
  CHECK(second.previous().inline_data() == "one");

  v1::Manifest got;
  REQUIRE(f.get("photos", "k", &got).ok());
  CHECK(got.chunks_size() == 2);
  CHECK_FALSE(got.has_inline_data());
}

TEST_CASE("an empty inline object keeps its presence over the wire", "[service]") {
  ServiceFixture f;
  f.create_bucket("photos");
  v1::PutManifestResponse put;
  REQUIRE(f.put(inline_manifest("photos", "empty", ""), &put).ok());
  v1::Manifest got;
  REQUIRE(f.get("photos", "empty", &got).ok());
  CHECK(got.has_inline_data());
}

TEST_CASE("manifest errors: NOT_FOUND messages differ, bad input is INVALID_ARGUMENT", "[service]") {
  ServiceFixture f;
  f.create_bucket("photos");
  v1::Manifest got;

  auto no_bucket = f.get("nope", "k", &got);
  auto no_object = f.get("photos", "k", &got);
  CHECK(no_bucket.error_code() == grpc::StatusCode::NOT_FOUND);
  CHECK(no_object.error_code() == grpc::StatusCode::NOT_FOUND);
  CHECK(no_bucket.error_message() != no_object.error_message());

  v1::PutManifestResponse put;
  CHECK(f.put(inline_manifest("nope", "k"), &put).error_code() == grpc::StatusCode::NOT_FOUND);
  CHECK(f.put(inline_manifest("photos", "../k"), &put).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
  auto both = chunked_manifest("photos", "k", 1, 1);
  both.set_inline_data("x");
  CHECK(f.put(both, &put).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
}

TEST_CASE("ListObjects pages and rejects max_keys above the maximum", "[service]") {
  ServiceFixture f;
  f.create_bucket("photos");
  v1::PutManifestResponse put;
  for (const char* k : {"a/1", "a/2", "a/3", "b/1"}) REQUIRE(f.put(inline_manifest("photos", k), &put).ok());

  grpc::ClientContext ctx;
  v1::ListObjectsRequest req;
  req.set_bucket("photos");
  req.set_prefix("a/");
  req.set_max_keys(2);
  v1::ListObjectsResponse resp;
  REQUIRE(f.stub->ListObjects(&ctx, req, &resp).ok());
  REQUIRE(resp.objects_size() == 2);
  CHECK(resp.objects(0).key() == "a/1");
  CHECK(resp.is_truncated());
  CHECK(resp.next_start_after() == "a/2");

  grpc::ClientContext ctx2;
  req.set_start_after(resp.next_start_after());
  v1::ListObjectsResponse resp2;
  REQUIRE(f.stub->ListObjects(&ctx2, req, &resp2).ok());
  REQUIRE(resp2.objects_size() == 1);
  CHECK_FALSE(resp2.is_truncated());

  grpc::ClientContext ctx3;
  req.set_max_keys(1001);
  CHECK(f.stub->ListObjects(&ctx3, req, &resp2).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
}

TEST_CASE("ListChunkRefs pages and rejects a bad token", "[service]") {
  ServiceFixture f;
  f.create_bucket("photos");
  v1::PutManifestResponse put;
  REQUIRE(f.put(chunked_manifest("photos", "k", 1, 5), &put).ok());

  grpc::ClientContext ctx;
  v1::ListChunkRefsRequest req;
  req.set_page_size(3);
  v1::ListChunkRefsResponse resp;
  REQUIRE(f.stub->ListChunkRefs(&ctx, req, &resp).ok());
  CHECK(resp.refs_size() == 3);
  CHECK(resp.next_page_token() == chunk_id(3));

  grpc::ClientContext ctx2;
  req.set_page_token(resp.next_page_token());
  REQUIRE(f.stub->ListChunkRefs(&ctx2, req, &resp).ok());
  CHECK(resp.refs_size() == 2);
  CHECK(resp.next_page_token().empty());

  grpc::ClientContext ctx3;
  req.set_page_token("bad");
  CHECK(f.stub->ListChunkRefs(&ctx3, req, &resp).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
}

TEST_CASE("a corrupt manifest blob is INTERNAL over the wire", "[service]") {
  ServiceFixture f;
  f.create_bucket("photos");
  v1::PutManifestResponse put;
  REQUIRE(f.put(inline_manifest("photos", "bad"), &put).ok());

  sqlite3* db = nullptr;
  REQUIRE(sqlite3_open((f.dir.path() / "data" / "index.db").c_str(), &db) == SQLITE_OK);
  REQUIRE(sqlite3_exec(db, "UPDATE manifests SET manifest = x'ffffff'", nullptr, nullptr, nullptr) == SQLITE_OK);
  sqlite3_close(db);

  v1::Manifest got;
  CHECK(f.get("photos", "bad", &got).error_code() == grpc::StatusCode::INTERNAL);
}

TEST_CASE("the service creates a missing data dir", "[service]") {
  TempDir dir;
  auto data = dir.path() / "a" / "b";
  ts2::ObjectIndexServiceImpl service({"n", "c", data.string()});
  CHECK(std::filesystem::exists(data / "index.db"));
}
