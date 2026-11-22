#include <grpcpp/grpcpp.h>

#include <catch2/catch_test_macros.hpp>
#include <chrono>
#include <fstream>
#include <thread>

#include "service.h"
#include "test_util.h"

using namespace ts2;
using namespace ts2::testing;
namespace fs = std::filesystem;
namespace v1 = ts2::v1;

namespace {

constexpr size_t kMiB = 1024 * 1024;

// A real gRPC server over an in-process channel, backed by a temp directory.
struct Fixture {
  TempDir dir;
  DiskStore store{dir.path()};
  std::unique_ptr<ChunkStoreServiceImpl> service;
  std::unique_ptr<grpc::Server> server;
  std::unique_ptr<v1::ChunkStoreService::Stub> stub;

  explicit Fixture(bool require_token = false) {
    REQUIRE(store.open().ok());
    service = std::make_unique<ChunkStoreServiceImpl>(NodeConfig{"n1", "c1", "rack-a", require_token}, store);
    grpc::ServerBuilder builder;
    apply_server_limits(builder);
    builder.RegisterService(service.get());
    server = builder.BuildAndStart();
    stub = v1::ChunkStoreService::NewStub(server->InProcessChannel({}));
  }
  ~Fixture() { server->Shutdown(); }

  size_t tmp_files() const { return count_files(dir.path() / "tmp"); }

  static v1::PutChunkRequest header(const std::string& id, const std::string& bytes) {
    v1::PutChunkRequest req;
    auto* h = req.mutable_header();
    h->set_chunk_id(id);
    h->set_expected_length(bytes.size());
    h->set_expected_sha256(sha256_hex(bytes));
    return req;
  }
  static v1::PutChunkRequest data(std::string bytes) {
    v1::PutChunkRequest req;
    req.set_data(std::move(bytes));
    return req;
  }

  // Sends the given messages as one PutChunk stream. The server may answer early, so a failed
  // Write just ends the loop and Finish() returns the real status.
  grpc::Status put_messages(const std::vector<v1::PutChunkRequest>& messages, v1::PutChunkResponse* resp = nullptr) {
    grpc::ClientContext ctx;
    v1::PutChunkResponse local;
    auto writer = stub->PutChunk(&ctx, resp ? resp : &local);
    for (const auto& m : messages) {
      if (!writer->Write(m)) break;
    }
    writer->WritesDone();
    return writer->Finish();
  }

  // Header plus the bytes cut into frames of frame_size.
  grpc::Status put(const std::string& id, const std::string& bytes, size_t frame_size = 64 * 1024) {
    std::vector<v1::PutChunkRequest> messages = {header(id, bytes)};
    for (size_t pos = 0; pos < bytes.size(); pos += frame_size) messages.push_back(data(bytes.substr(pos, frame_size)));
    return put_messages(messages);
  }

  // Reads a chunk. Returns the status and the frames received.
  grpc::Status get(const std::string& id, std::vector<std::string>& frames, uint64_t start = 0, uint64_t end = 0) {
    grpc::ClientContext ctx;
    v1::GetChunkRequest req;
    req.set_chunk_id(id);
    req.set_range_start(start);
    req.set_range_end(end);
    auto reader = stub->GetChunk(&ctx, req);
    v1::GetChunkResponse resp;
    while (reader->Read(&resp)) frames.push_back(resp.data());
    return reader->Finish();
  }

  static std::string join(const std::vector<std::string>& frames) {
    std::string all;
    for (const auto& f : frames) all += f;
    return all;
  }
};

}  // namespace

TEST_CASE("NodeInfo reports the configured identity") {
  Fixture f;
  grpc::ClientContext ctx;
  v1::ChunkStoreNodeInfoRequest req;
  v1::ChunkStoreNodeInfoResponse resp;
  REQUIRE(f.stub->NodeInfo(&ctx, req, &resp).ok());
  CHECK(resp.node_id() == "n1");
  CHECK(resp.cell() == "c1");
  CHECK(resp.failure_domain() == "rack-a");
  CHECK(resp.bytes_total() > 0);
  CHECK(resp.healthy());
}

TEST_CASE("NodeInfo is unhealthy when tmp is not writable") {
  Fixture f;
  fs::permissions(f.dir.path() / "tmp", fs::perms::owner_read | fs::perms::owner_exec);
  if (::access((f.dir.path() / "tmp").c_str(), W_OK) == 0) return;  // running as root: permissions not enforced

  grpc::ClientContext ctx;
  v1::ChunkStoreNodeInfoRequest req;
  v1::ChunkStoreNodeInfoResponse resp;
  REQUIRE(f.stub->NodeInfo(&ctx, req, &resp).ok());
  CHECK_FALSE(resp.healthy());
}

TEST_CASE("a 4 MiB chunk uploaded in 64 KiB frames is read back in 1 MiB frames") {
  Fixture f;
  auto id = make_id(0xab, 1);
  auto bytes = make_bytes(4 * kMiB);

  grpc::ClientContext ctx;
  v1::PutChunkResponse resp;
  std::vector<v1::PutChunkRequest> messages = {Fixture::header(id, bytes)};
  for (size_t pos = 0; pos < bytes.size(); pos += 64 * 1024) messages.push_back(Fixture::data(bytes.substr(pos, 64 * 1024)));
  REQUIRE(f.put_messages(messages, &resp).ok());
  CHECK(resp.stored_length() == bytes.size());
  CHECK(resp.sha256() == sha256_hex(bytes));

  std::vector<std::string> frames;
  REQUIRE(f.get(id, frames).ok());
  CHECK(frames.size() == 4);
  for (const auto& frame : frames) CHECK(frame.size() == kMiB);
  CHECK(Fixture::join(frames) == bytes);
}

TEST_CASE("a 1 MiB frame is accepted") {
  Fixture f;
  auto bytes = make_bytes(kMiB);
  CHECK(f.put(make_id(1, 1), bytes, kMiB).ok());
}

TEST_CASE("a frame over 1 MiB is rejected and nothing is stored") {
  Fixture f;
  auto id = make_id(1, 1);

  SECTION("just over the limit, inside the message slack: the handler rejects it") {
    auto bytes = make_bytes(kMiB + 512);
    CHECK(f.put(id, bytes, bytes.size()).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
  }
  SECTION("beyond the message size limit: gRPC rejects it") {
    auto bytes = make_bytes(kMiB + 4096);
    CHECK(f.put(id, bytes, bytes.size()).error_code() == grpc::StatusCode::RESOURCE_EXHAUSTED);
  }
  CHECK(f.tmp_files() == 0);
  std::vector<std::string> frames;
  CHECK(f.get(id, frames).error_code() == grpc::StatusCode::NOT_FOUND);
}

TEST_CASE("ranged get returns only the slice") {
  Fixture f;
  auto id = make_id(1, 1);
  auto bytes = make_bytes(3 * kMiB);
  REQUIRE(f.put(id, bytes).ok());

  std::vector<std::string> frames;
  REQUIRE(f.get(id, frames, 100, 100 + 2 * kMiB).ok());
  CHECK(Fixture::join(frames) == bytes.substr(100, 2 * kMiB));
  CHECK(frames.size() == 2);

  frames.clear();
  REQUIRE(f.get(id, frames, 3 * kMiB - 10).ok());  // to the end
  CHECK(Fixture::join(frames) == bytes.substr(3 * kMiB - 10));
}

TEST_CASE("get range errors") {
  Fixture f;
  auto id = make_id(1, 1);
  REQUIRE(f.put(id, "0123456789").ok());
  std::vector<std::string> frames;
  CHECK(f.get(id, frames, 0, 11).error_code() == grpc::StatusCode::OUT_OF_RANGE);
  CHECK(f.get(id, frames, 5, 5).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
  CHECK(f.get(make_id(9, 9), frames).error_code() == grpc::StatusCode::NOT_FOUND);
  CHECK(frames.empty());
}

TEST_CASE("the first message must be a header") {
  Fixture f;
  CHECK(f.put_messages({Fixture::data("hello")}).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
  CHECK(f.put_messages({}).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
  CHECK(f.tmp_files() == 0);
}

TEST_CASE("a second header mid-stream is rejected and tmp is cleaned") {
  Fixture f;
  auto id = make_id(1, 1);
  auto status = f.put_messages({Fixture::header(id, "hello"), Fixture::data("hel"), Fixture::header(id, "hello")});
  CHECK(status.error_code() == grpc::StatusCode::INVALID_ARGUMENT);
  CHECK(f.tmp_files() == 0);
  CHECK_FALSE(fs::exists(f.dir.path() / "00" / id));
}

TEST_CASE("sha256 and length mismatches are INVALID_ARGUMENT and store nothing") {
  Fixture f;
  auto id = make_id(1, 1);

  SECTION("wrong sha256") {
    auto header = Fixture::header(id, "hello");
    header.mutable_header()->set_expected_sha256(sha256_hex("other"));
    CHECK(f.put_messages({header, Fixture::data("hello")}).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
  }
  SECTION("short") {
    CHECK(f.put_messages({Fixture::header(id, "hello"), Fixture::data("hel")}).error_code() ==
          grpc::StatusCode::INVALID_ARGUMENT);
  }
  SECTION("long") {
    CHECK(f.put_messages({Fixture::header(id, "hello"), Fixture::data("hello!")}).error_code() ==
          grpc::StatusCode::INVALID_ARGUMENT);
  }
  CHECK(f.tmp_files() == 0);
  CHECK(count_files(f.dir.path()) == 0);
}

TEST_CASE("bad chunk ids are INVALID_ARGUMENT on every RPC") {
  Fixture f;
  for (std::string id : {std::string("../../x"), std::string("ABCDEF0123456789abcdef0123456789"),
                         make_id(1, 1).substr(1), make_id(1, 1) + "0", std::string("0123456789abcdef0123456789abcd\0f", 32)}) {
    CAPTURE(id);
    CHECK(f.put_messages({Fixture::header(id, "x"), Fixture::data("x")}).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
    std::vector<std::string> frames;
    CHECK(f.get(id, frames).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
    grpc::ClientContext ctx;
    v1::DeleteChunkRequest req;
    req.set_chunk_id(id);
    v1::DeleteChunkResponse resp;
    CHECK(f.stub->DeleteChunk(&ctx, req, &resp).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
  }
  CHECK(count_files(f.dir.path()) == 0);
}

TEST_CASE("put is idempotent for the same bytes and ALREADY_EXISTS for different bytes") {
  Fixture f;
  auto id = make_id(1, 1);
  REQUIRE(f.put(id, "hello").ok());
  CHECK(f.put(id, "hello").ok());
  CHECK(f.put(id, "world").error_code() == grpc::StatusCode::ALREADY_EXISTS);

  std::vector<std::string> frames;
  REQUIRE(f.get(id, frames).ok());
  CHECK(Fixture::join(frames) == "hello");
  CHECK(f.tmp_files() == 0);
}

TEST_CASE("a flipped byte on disk gives DATA_LOSS and no frames, ranged or not") {
  Fixture f;
  auto id = make_id(0xab, 1);
  REQUIRE(f.put(id, make_bytes(3 * kMiB)).ok());
  {
    std::fstream file(f.dir.path() / "ab" / id, std::ios::in | std::ios::out | std::ios::binary);
    file.seekp(3 * kMiB - 1);
    file.put('!');
  }
  std::vector<std::string> frames;
  CHECK(f.get(id, frames).error_code() == grpc::StatusCode::DATA_LOSS);
  CHECK(f.get(id, frames, 0, 10).error_code() == grpc::StatusCode::DATA_LOSS);
  CHECK(frames.empty());
}

TEST_CASE("a missing sidecar is DATA_LOSS") {
  Fixture f;
  auto id = make_id(0xab, 1);
  REQUIRE(f.put(id, "hello").ok());
  fs::remove(f.dir.path() / "ab" / (id + ".sha256"));
  std::vector<std::string> frames;
  CHECK(f.get(id, frames).error_code() == grpc::StatusCode::DATA_LOSS);
}

TEST_CASE("delete is idempotent and a deleted chunk is NOT_FOUND") {
  Fixture f;
  auto id = make_id(1, 1);
  REQUIRE(f.put(id, "hello").ok());

  v1::DeleteChunkRequest req;
  req.set_chunk_id(id);
  for (int i = 0; i < 2; ++i) {
    grpc::ClientContext ctx;
    v1::DeleteChunkResponse resp;
    CHECK(f.stub->DeleteChunk(&ctx, req, &resp).ok());
  }
  std::vector<std::string> frames;
  CHECK(f.get(id, frames).error_code() == grpc::StatusCode::NOT_FOUND);
}

TEST_CASE("ListChunks pages through the chunks in order") {
  Fixture f;
  std::vector<std::string> ids;
  for (unsigned i = 0; i < 5; ++i) ids.push_back(make_id(i * 40, i));  // spread over fan-out directories
  for (const auto& id : ids) REQUIRE(f.put(id, "x").ok());

  std::vector<std::string> seen;
  std::string token;
  int pages = 0;
  do {
    grpc::ClientContext ctx;
    v1::ListChunksRequest req;
    req.set_page_size(2);
    req.set_page_token(token);
    v1::ListChunksResponse resp;
    REQUIRE(f.stub->ListChunks(&ctx, req, &resp).ok());
    for (const auto& c : resp.chunks()) {
      seen.push_back(c.chunk_id());
      CHECK(c.length() == 1);
      CHECK_FALSE(c.corrupt());
    }
    token = resp.next_page_token();
    ++pages;
  } while (!token.empty());

  CHECK(pages == 3);
  CHECK(seen == ids);  // already ascending
}

TEST_CASE("ListChunks rejects a bad token and an oversize page") {
  Fixture f;
  v1::ListChunksResponse resp;
  {
    grpc::ClientContext ctx;
    v1::ListChunksRequest req;
    req.set_page_token("nope");
    CHECK(f.stub->ListChunks(&ctx, req, &resp).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
  }
  {
    grpc::ClientContext ctx;
    v1::ListChunksRequest req;
    req.set_page_size(10001);
    CHECK(f.stub->ListChunks(&ctx, req, &resp).error_code() == grpc::StatusCode::INVALID_ARGUMENT);
  }
}

TEST_CASE("a client that cancels mid-upload leaves nothing in tmp or data") {
  Fixture f;
  auto id = make_id(1, 1);
  auto bytes = make_bytes(256 * 1024);

  grpc::ClientContext ctx;
  v1::PutChunkResponse resp;
  auto writer = f.stub->PutChunk(&ctx, &resp);
  REQUIRE(writer->Write(Fixture::header(id, bytes)));
  REQUIRE(writer->Write(Fixture::data(bytes.substr(0, 64 * 1024))));
  ctx.TryCancel();
  CHECK_FALSE(writer->Finish().ok());

  // The server notices the cancel asynchronously, so give it a moment.
  for (int i = 0; i < 100 && count_files(f.dir.path()) != 0; ++i) {
    std::this_thread::sleep_for(std::chrono::milliseconds(20));
  }
  CHECK(count_files(f.dir.path()) == 0);
}

TEST_CASE("write_token is ignored unless TS2_REQUIRE_TOKEN is on") {
  SECTION("off: empty and non-empty tokens are accepted") {
    Fixture f;
    auto header = Fixture::header(make_id(1, 1), "hello");
    header.mutable_header()->set_write_token("whatever");
    CHECK(f.put_messages({header, Fixture::data("hello")}).ok());
    CHECK(f.put(make_id(1, 2), "hello").ok());
  }
  SECTION("on: a missing token is PERMISSION_DENIED") {
    Fixture f(/*require_token=*/true);
    CHECK(f.put(make_id(1, 1), "hello").error_code() == grpc::StatusCode::PERMISSION_DENIED);
    auto header = Fixture::header(make_id(1, 1), "hello");
    header.mutable_header()->set_write_token("t");
    CHECK(f.put_messages({header, Fixture::data("hello")}).ok());
  }
}
