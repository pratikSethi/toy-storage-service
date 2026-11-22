#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

#include <catch2/catch_test_macros.hpp>
#include <algorithm>
#include <cerrno>
#include <chrono>
#include <ctime>
#include <fstream>
#include <vector>

#include "disk_store.h"
#include "test_util.h"

using namespace ts2;
using namespace ts2::testing;
namespace fs = std::filesystem;

namespace {

struct Fixture {
  TempDir dir;
  DiskStore store{dir.path()};
  Fixture() { REQUIRE(store.open().ok()); }

  fs::path data_file(const std::string& id) const { return dir.path() / id.substr(0, 2) / id; }
  fs::path sidecar_file(const std::string& id) const { return dir.path() / id.substr(0, 2) / (id + ".sha256"); }
  size_t tmp_files() const { return count_files(dir.path() / "tmp"); }
};

std::string read_all(const fs::path& p) {
  std::ifstream in(p, std::ios::binary);
  return {std::istreambuf_iterator<char>(in), {}};
}

}  // namespace

TEST_CASE("chunk ids must be exactly 32 lowercase hex characters") {
  CHECK(DiskStore::valid_chunk_id(make_id(0xab, 1)));
  CHECK_FALSE(DiskStore::valid_chunk_id(""));
  CHECK_FALSE(DiskStore::valid_chunk_id("../../etc/passwd"));
  CHECK_FALSE(DiskStore::valid_chunk_id("../" + make_id(1, 1).substr(3)));  // right length, has a slash
  CHECK_FALSE(DiskStore::valid_chunk_id("ABCDEF0123456789abcdef0123456789"));  // uppercase
  CHECK_FALSE(DiskStore::valid_chunk_id(make_id(1, 1).substr(1)));             // 31 characters
  CHECK_FALSE(DiskStore::valid_chunk_id(make_id(1, 1) + "0"));                 // 33 characters
  CHECK_FALSE(DiskStore::valid_chunk_id(std::string("0123456789abcdef0123456789abcd\0f", 32)));  // NUL byte
  CHECK_FALSE(DiskStore::valid_chunk_id(make_id(1, 1) + ".sha256"));
}

TEST_CASE("every operation rejects a bad id") {
  Fixture f;
  std::unique_ptr<ChunkWriter> w;
  std::string out;
  ListPage page;
  CHECK(f.store.begin_put("../x", 0, sha256_hex(""), w).code == Code::kInvalidArgument);
  CHECK(f.store.get("../x", 0, 0, out).code == Code::kInvalidArgument);
  CHECK(f.store.remove("../x").code == Code::kInvalidArgument);
  CHECK(f.store.list(0, "../x", page).code == Code::kInvalidArgument);
}

TEST_CASE("put then get returns the same bytes") {
  Fixture f;
  auto id = make_id(0x12, 1);
  auto bytes = make_bytes(1000);
  REQUIRE(put_chunk(f.store, id, bytes).ok());

  std::string out;
  REQUIRE(f.store.get(id, 0, 0, out).ok());
  CHECK(out == bytes);
  CHECK(fs::exists(f.sidecar_file(id)));
  CHECK(read_all(f.sidecar_file(id)) == sha256_hex(bytes) + "\n");
  CHECK(f.tmp_files() == 0);
}

TEST_CASE("an empty chunk can be stored and read") {
  Fixture f;
  auto id = make_id(1, 1);
  REQUIRE(put_chunk(f.store, id, "").ok());
  std::string out = "x";
  REQUIRE(f.store.get(id, 0, 0, out).ok());
  CHECK(out.empty());
}

TEST_CASE("ranged reads return the half-open slice") {
  Fixture f;
  auto id = make_id(1, 1);
  std::string bytes = "0123456789";
  REQUIRE(put_chunk(f.store, id, bytes).ok());
  std::string out;

  REQUIRE(f.store.get(id, 2, 5, out).ok());
  CHECK(out == "234");
  REQUIRE(f.store.get(id, 7, 0, out).ok());  // end 0 means to the end
  CHECK(out == "789");
  REQUIRE(f.store.get(id, 0, 10, out).ok());  // end == length is allowed
  CHECK(out == bytes);
  REQUIRE(f.store.get(id, 10, 0, out).ok());  // start == length: nothing left, not an error
  CHECK(out.empty());
}

TEST_CASE("invalid ranges") {
  Fixture f;
  auto id = make_id(1, 1);
  REQUIRE(put_chunk(f.store, id, "0123456789").ok());
  std::string out;

  CHECK(f.store.get(id, 0, 11, out).code == Code::kOutOfRange);
  CHECK(f.store.get(id, 11, 0, out).code == Code::kOutOfRange);
  CHECK(f.store.get(id, 5, 5, out).code == Code::kInvalidArgument);  // empty range
  CHECK(f.store.get(id, 6, 5, out).code == Code::kInvalidArgument);  // backwards
}

TEST_CASE("get of a missing chunk is NOT_FOUND") {
  Fixture f;
  std::string out;
  CHECK(f.store.get(make_id(1, 1), 0, 0, out).code == Code::kNotFound);
}

TEST_CASE("a wrong sha256 stores nothing") {
  Fixture f;
  auto id = make_id(1, 1);
  std::unique_ptr<ChunkWriter> w;
  REQUIRE(f.store.begin_put(id, 5, sha256_hex("other"), w).ok());
  REQUIRE(w->append("hello").ok());
  CHECK(w->commit().code == Code::kInvalidArgument);
  w.reset();

  CHECK(f.tmp_files() == 0);
  CHECK_FALSE(fs::exists(f.data_file(id)));
  CHECK_FALSE(fs::exists(f.sidecar_file(id)));
}

TEST_CASE("a short upload stores nothing") {
  Fixture f;
  auto id = make_id(1, 1);
  std::unique_ptr<ChunkWriter> w;
  REQUIRE(f.store.begin_put(id, 10, sha256_hex("hello"), w).ok());
  REQUIRE(w->append("hello").ok());
  CHECK(w->commit().code == Code::kInvalidArgument);
  w.reset();

  CHECK(f.tmp_files() == 0);
  CHECK_FALSE(fs::exists(f.data_file(id)));
}

TEST_CASE("a long upload is rejected as soon as it exceeds expected_length") {
  Fixture f;
  auto id = make_id(1, 1);
  std::unique_ptr<ChunkWriter> w;
  REQUIRE(f.store.begin_put(id, 3, sha256_hex("hel"), w).ok());
  CHECK(w->append("hello").code == Code::kInvalidArgument);
  w.reset();
  CHECK(f.tmp_files() == 0);
}

TEST_CASE("a writer destroyed without commit cleans up tmp") {
  Fixture f;
  std::unique_ptr<ChunkWriter> w;
  REQUIRE(f.store.begin_put(make_id(1, 1), 5, sha256_hex("hello"), w).ok());
  REQUIRE(w->append("hel").ok());
  CHECK(f.tmp_files() == 2);  // data and sidecar
  w.reset();                  // what a cancelled upload does
  CHECK(f.tmp_files() == 0);
  CHECK_FALSE(fs::exists(f.data_file(make_id(1, 1))));
}

TEST_CASE("begin_put validates the sha256 format") {
  Fixture f;
  std::unique_ptr<ChunkWriter> w;
  CHECK(f.store.begin_put(make_id(1, 1), 0, "abc", w).code == Code::kInvalidArgument);
  CHECK(f.store.begin_put(make_id(1, 1), 0, std::string(64, 'A'), w).code == Code::kInvalidArgument);
}

TEST_CASE("putting the same id and bytes twice succeeds") {
  Fixture f;
  auto id = make_id(1, 1);
  REQUIRE(put_chunk(f.store, id, "hello").ok());
  REQUIRE(put_chunk(f.store, id, "hello").ok());
  CHECK(f.tmp_files() == 0);
}

TEST_CASE("a repeated put still verifies the bytes it receives") {
  Fixture f;
  auto id = make_id(1, 1);
  REQUIRE(put_chunk(f.store, id, "hello").ok());
  std::unique_ptr<ChunkWriter> w;
  REQUIRE(f.store.begin_put(id, 5, sha256_hex("hello"), w).ok());
  REQUIRE(w->append("HELLO").ok());
  CHECK(w->commit().code == Code::kInvalidArgument);
}

TEST_CASE("the same id with different bytes is ALREADY_EXISTS and the original is kept") {
  Fixture f;
  auto id = make_id(1, 1);
  REQUIRE(put_chunk(f.store, id, "hello").ok());
  CHECK(put_chunk(f.store, id, "world").code == Code::kAlreadyExists);

  std::string out;
  REQUIRE(f.store.get(id, 0, 0, out).ok());
  CHECK(out == "hello");
  CHECK(f.tmp_files() == 0);
}

TEST_CASE("a flipped byte makes get and ranged get fail with DATA_LOSS") {
  Fixture f;
  auto id = make_id(1, 1);
  REQUIRE(put_chunk(f.store, id, make_bytes(100)).ok());
  {
    std::fstream file(f.data_file(id), std::ios::in | std::ios::out | std::ios::binary);
    file.seekp(90);  // outside the range read below, so only whole-chunk verification can catch it
    file.put('!');
  }
  std::string out;
  CHECK(f.store.get(id, 0, 0, out).code == Code::kDataLoss);
  CHECK(f.store.get(id, 0, 10, out).code == Code::kDataLoss);
}

TEST_CASE("a data file without a sidecar is corrupt and not served") {
  Fixture f;
  auto id = make_id(1, 1);
  REQUIRE(put_chunk(f.store, id, "hello").ok());
  fs::remove(f.sidecar_file(id));

  std::string out;
  CHECK(f.store.get(id, 0, 0, out).code == Code::kDataLoss);

  ListPage page;
  REQUIRE(f.store.list(0, "", page).ok());
  REQUIRE(page.chunks.size() == 1);
  CHECK(page.chunks[0].corrupt);

  // A new put may replace the corrupt chunk.
  REQUIRE(put_chunk(f.store, id, "hello").ok());
  REQUIRE(f.store.get(id, 0, 0, out).ok());
}

TEST_CASE("delete is idempotent") {
  Fixture f;
  auto id = make_id(1, 1);
  CHECK(f.store.remove(id).ok());  // never existed
  REQUIRE(put_chunk(f.store, id, "hello").ok());
  CHECK(f.store.remove(id).ok());
  CHECK(f.store.remove(id).ok());
  CHECK_FALSE(fs::exists(f.data_file(id)));
  CHECK_FALSE(fs::exists(f.sidecar_file(id)));

  std::string out;
  CHECK(f.store.get(id, 0, 0, out).code == Code::kNotFound);
}

TEST_CASE("list is sorted and pages by last id") {
  Fixture f;
  // Ids in several fan-out directories, stored out of order.
  std::vector<std::string> ids = {make_id(0x02, 2), make_id(0x01, 5), make_id(0x01, 3), make_id(0xff, 1),
                                  make_id(0x02, 1)};
  for (const auto& id : ids) REQUIRE(put_chunk(f.store, id, "x").ok());
  std::sort(ids.begin(), ids.end());

  ListPage page;
  REQUIRE(f.store.list(2, "", page).ok());
  REQUIRE(page.chunks.size() == 2);
  CHECK(page.chunks[0].id == ids[0]);
  CHECK(page.chunks[1].id == ids[1]);
  CHECK(page.next_page_token == ids[1]);

  REQUIRE(f.store.list(2, page.next_page_token, page).ok());
  REQUIRE(page.chunks.size() == 2);
  CHECK(page.chunks[0].id == ids[2]);
  CHECK(page.chunks[1].id == ids[3]);
  CHECK(page.next_page_token == ids[3]);

  REQUIRE(f.store.list(2, page.next_page_token, page).ok());
  REQUIRE(page.chunks.size() == 1);
  CHECK(page.chunks[0].id == ids[4]);
  CHECK(page.next_page_token.empty());
}

TEST_CASE("list page boundaries") {
  Fixture f;
  ListPage page;

  SECTION("empty store") {
    REQUIRE(f.store.list(0, "", page).ok());
    CHECK(page.chunks.empty());
    CHECK(page.next_page_token.empty());
  }
  SECTION("exactly one full page has no next token") {
    for (unsigned i = 0; i < 3; ++i) REQUIRE(put_chunk(f.store, make_id(1, i), "x").ok());
    REQUIRE(f.store.list(3, "", page).ok());
    CHECK(page.chunks.size() == 3);
    CHECK(page.next_page_token.empty());
  }
  SECTION("page_size 1") {
    for (unsigned i = 0; i < 2; ++i) REQUIRE(put_chunk(f.store, make_id(1, i), "x").ok());
    REQUIRE(f.store.list(1, "", page).ok());
    CHECK(page.chunks.size() == 1);
    CHECK(page.next_page_token == make_id(1, 0));
  }
  SECTION("a token past the end gives an empty page") {
    REQUIRE(put_chunk(f.store, make_id(1, 1), "x").ok());
    REQUIRE(f.store.list(0, make_id(0xff, 0), page).ok());
    CHECK(page.chunks.empty());
    CHECK(page.next_page_token.empty());
  }
  SECTION("a token that is not an id is INVALID_ARGUMENT") {
    CHECK(f.store.list(0, "nope", page).code == Code::kInvalidArgument);
  }
  SECTION("page_size above the maximum is INVALID_ARGUMENT") {
    CHECK(f.store.list(10001, "", page).code == Code::kInvalidArgument);
    CHECK(f.store.list(10000, "", page).ok());
  }
}

TEST_CASE("list never shows tmp files or sidecars, and reports length and mtime") {
  Fixture f;
  auto id = make_id(1, 1);
  REQUIRE(put_chunk(f.store, id, "hello").ok());
  std::unique_ptr<ChunkWriter> in_flight;
  REQUIRE(f.store.begin_put(make_id(2, 2), 5, sha256_hex("hello"), in_flight).ok());

  ListPage page;
  REQUIRE(f.store.list(0, "", page).ok());
  REQUIRE(page.chunks.size() == 1);
  CHECK(page.chunks[0].id == id);
  CHECK(page.chunks[0].length == 5);
  CHECK_FALSE(page.chunks[0].corrupt);
  auto now = std::time(nullptr);
  CHECK(page.chunks[0].created_unix <= now);
  CHECK(page.chunks[0].created_unix >= now - 60);

  // created_unix comes from the data file mtime.
  timespec times[2] = {{1000, 0}, {1000, 0}};
  REQUIRE(::utimensat(AT_FDCWD, f.data_file(id).c_str(), times, 0) == 0);
  REQUIRE(f.store.list(0, "", page).ok());
  CHECK(page.chunks[0].created_unix == 1000);
}

TEST_CASE("open clears tmp and leaves stored chunks alone") {
  Fixture f;
  auto id = make_id(1, 1);
  REQUIRE(put_chunk(f.store, id, "hello").ok());
  std::ofstream(f.dir.path() / "tmp" / "leftover") << "partial";
  std::ofstream(f.dir.path() / "tmp" / "leftover.sha256") << "partial";
  REQUIRE(f.tmp_files() == 2);

  DiskStore restarted(f.dir.path());
  REQUIRE(restarted.open().ok());
  CHECK(f.tmp_files() == 0);
  std::string out;
  REQUIRE(restarted.get(id, 0, 0, out).ok());
  CHECK(out == "hello");
}

TEST_CASE("writable is false when tmp cannot be written") {
  Fixture f;
  CHECK(f.store.writable());
  fs::permissions(f.dir.path() / "tmp", fs::perms::owner_read | fs::perms::owner_exec);
  // Running as root ignores permissions, so only check when the OS actually enforces them.
  if (::access((f.dir.path() / "tmp").c_str(), W_OK) != 0) CHECK_FALSE(f.store.writable());
}

TEST_CASE("durability order: fsync data, fsync sidecar, rename sidecar, rename data, fsync dir") {
  TempDir dir;
  std::vector<std::string> events;
  FsOps ops = default_fs_ops();
  auto real = ops;
  // Record each call by file name, then do the real work.
  ops.sync_file = [&](int fd, const fs::path& path) {
    events.push_back("sync_file " + path.filename().string());
    return real.sync_file(fd, path);
  };
  ops.sync_dir = [&](const fs::path& d) {
    events.push_back("sync_dir " + d.filename().string());
    return real.sync_dir(d);
  };
  ops.rename = [&](const fs::path& from, const fs::path& to) {
    events.push_back("rename " + to.filename().string());
    return real.rename(from, to);
  };
  DiskStore store(dir.path(), ops);
  REQUIRE(store.open().ok());

  // The first put in a fan-out directory also syncs the root, so look at the second one.
  REQUIRE(put_chunk(store, make_id(0xab, 1), "first").ok());
  events.clear();
  auto id = make_id(0xab, 2);
  REQUIRE(put_chunk(store, id, "second").ok());

  REQUIRE(events.size() == 5);
  CHECK(events[0].starts_with("sync_file " + id + "."));
  CHECK_FALSE(events[0].ends_with(".sha256"));
  CHECK(events[1].starts_with("sync_file " + id + "."));
  CHECK(events[1].ends_with(".sha256"));
  CHECK(events[2] == "rename " + id + ".sha256");
  CHECK(events[3] == "rename " + id);
  CHECK(events[4] == "sync_dir ab");
}

TEST_CASE("a failing fsync fails the put and leaves nothing behind") {
  TempDir dir;
  FsOps ops = default_fs_ops();
  ops.sync_file = [](int, const fs::path&) {
    errno = EIO;
    return false;
  };
  DiskStore store(dir.path(), ops);
  REQUIRE(store.open().ok());

  auto id = make_id(1, 1);
  CHECK(put_chunk(store, id, "hello").code == Code::kInternal);
  CHECK(count_files(dir.path()) == 0);
}
