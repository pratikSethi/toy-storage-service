// Index-level tests: SqliteIndex directly, no gRPC.
#include <sqlite3.h>

#include <catch2/catch_test_macros.hpp>
#include <set>
#include <string>
#include <thread>
#include <vector>

#include "sqlite_index.h"
#include "test_util.h"

using ts2::IndexError;
using ts2::IndexErrorKind;
using ts2::SqliteIndex;

namespace {

// Checks that `fn` throws an IndexError of `kind`, and returns its message for further checks.
template <typename Fn>
std::string error_message(IndexErrorKind kind, Fn&& fn) {
  try {
    fn();
  } catch (const IndexError& e) {
    REQUIRE(e.kind() == kind);
    return e.what();
  }
  FAIL("expected an IndexError");
  return "";
}

// A temp dir, an index on it, and a bucket called "photos".
struct Fixture {
  TempDir dir;
  SqliteIndex index{dir.path() / "index.db"};
  Fixture() { index.create_bucket("photos"); }

  void put_inline(const std::string& key) { index.put_manifest(inline_manifest("photos", key)); }

  std::vector<std::string> keys(const std::string& prefix, const std::string& start_after = "",
                                uint32_t max_keys = 0) {
    std::vector<std::string> out;
    for (const auto& o : index.list_objects("photos", prefix, start_after, max_keys).objects) out.push_back(o.key());
    return out;
  }
};

// Runs raw SQL against the database file, like a corrupting bug (or an operator) would.
void raw_sql(const std::filesystem::path& db_path, const char* sql) {
  sqlite3* db = nullptr;
  REQUIRE(sqlite3_open(db_path.c_str(), &db) == SQLITE_OK);
  char* err = nullptr;
  int rc = sqlite3_exec(db, sql, nullptr, nullptr, &err);
  INFO(std::string(err ? err : ""));
  REQUIRE(rc == SQLITE_OK);
  sqlite3_free(err);
  sqlite3_close(db);
}

}  // namespace

// ---- Buckets ---------------------------------------------------------------------------------

TEST_CASE("create, get and list buckets", "[bucket]") {
  TempDir dir;
  SqliteIndex index(dir.path() / "index.db");

  auto created = index.create_bucket("zeta");
  index.create_bucket("alpha");
  CHECK(created.name() == "zeta");
  CHECK(created.created_at().seconds() > 1'700'000'000);

  CHECK(index.get_bucket("zeta").created_at().seconds() == created.created_at().seconds());
  CHECK(index.get_bucket("zeta").created_at().nanos() == created.created_at().nanos());

  auto all = index.list_buckets();
  REQUIRE(all.size() == 2);
  CHECK(all[0].name() == "alpha");  // ordered by name
  CHECK(all[1].name() == "zeta");
}

TEST_CASE("duplicate bucket is rejected, missing bucket is not found", "[bucket]") {
  TempDir dir;
  SqliteIndex index(dir.path() / "index.db");
  index.create_bucket("photos");
  error_message(IndexErrorKind::BucketExists, [&] { index.create_bucket("photos"); });
  error_message(IndexErrorKind::BucketNotFound, [&] { index.get_bucket("nope"); });
  error_message(IndexErrorKind::BucketNotFound, [&] { index.delete_bucket("nope"); });
}

TEST_CASE("bucket name rules", "[bucket][validation]") {
  TempDir dir;
  SqliteIndex index(dir.path() / "index.db");
  const std::vector<std::string> good = {"abc", "my-bucket", "a1b2c3", std::string(63, 'a'), "---", "123"};
  const std::vector<std::string> bad = {"",    "ab",  std::string(64, 'a'), "Upper",  "with_underscore",
                                        "a.b", "a b", "a/b",                "bücket", std::string("ab\0c", 4)};
  for (const auto& name : good) {
    INFO("good: " << name);
    CHECK_NOTHROW(index.create_bucket(name));
  }
  for (const auto& name : bad) {
    INFO("bad: " << name);
    error_message(IndexErrorKind::InvalidArgument, [&] { index.create_bucket(name); });
  }
}

TEST_CASE("delete bucket: only when empty", "[bucket]") {
  Fixture f;
  f.put_inline("a.txt");
  error_message(IndexErrorKind::BucketNotEmpty, [&] { f.index.delete_bucket("photos"); });
  CHECK_NOTHROW(f.index.get_bucket("photos"));  // still there

  f.index.delete_manifest("photos", "a.txt");
  CHECK_NOTHROW(f.index.delete_bucket("photos"));
  error_message(IndexErrorKind::BucketNotFound, [&] { f.index.get_bucket("photos"); });
}

// ---- Manifests -------------------------------------------------------------------------------

TEST_CASE("put assigns version 1, then n+1 on overwrite, and returns previous", "[manifest]") {
  Fixture f;
  auto first = f.index.put_manifest(inline_manifest("photos", "k", "one"));
  CHECK(first.stored.version() == 1);
  CHECK_FALSE(first.previous.has_value());

  auto second = f.index.put_manifest(inline_manifest("photos", "k", "two!"));
  CHECK(second.stored.version() == 2);
  REQUIRE(second.previous.has_value());
  CHECK(second.previous->version() == 1);
  CHECK(second.previous->inline_data() == "one");

  auto third = f.index.put_manifest(inline_manifest("photos", "k", "three"));
  CHECK(third.stored.version() == 3);
  CHECK(f.index.get_manifest("photos", "k").inline_data() == "three");

  // The caller's version is ignored: the index owns it.
  auto m = inline_manifest("photos", "other");
  m.set_version(99);
  CHECK(f.index.put_manifest(m).stored.version() == 1);
}

TEST_CASE("manifest fields round trip, created_at is not rewritten", "[manifest]") {
  Fixture f;
  auto m = inline_manifest("photos", "dir/file.txt", "payload");
  (*m.mutable_metadata())["owner"] = "pratik";
  (*m.mutable_metadata())["empty"] = "";
  f.index.put_manifest(m);

  auto got = f.index.get_manifest("photos", "dir/file.txt");
  CHECK(got.bucket() == "photos");
  CHECK(got.key() == "dir/file.txt");
  CHECK(got.size() == 7);
  CHECK(got.etag() == "etag-dir/file.txt");
  CHECK(got.content_type() == "text/plain");
  CHECK(got.created_at().seconds() == 1'700'000'000);
  CHECK(got.created_at().nanos() == 123);
  CHECK(got.metadata().at("owner") == "pratik");
  CHECK(got.metadata().at("empty") == "");
  CHECK(got.inline_data() == "payload");
}

TEST_CASE("NOT_FOUND messages say whether the bucket or the object is missing", "[manifest]") {
  Fixture f;
  auto no_bucket = error_message(IndexErrorKind::BucketNotFound, [&] { f.index.get_manifest("nope", "k"); });
  auto no_object = error_message(IndexErrorKind::ObjectNotFound, [&] { f.index.get_manifest("photos", "k"); });
  CHECK(no_bucket.find("bucket") != std::string::npos);
  CHECK(no_object.find("object") != std::string::npos);
  CHECK(no_bucket != no_object);

  error_message(IndexErrorKind::BucketNotFound, [&] { f.index.delete_manifest("nope", "k"); });
  error_message(IndexErrorKind::ObjectNotFound, [&] { f.index.delete_manifest("photos", "k"); });
  error_message(IndexErrorKind::BucketNotFound, [&] { f.index.put_manifest(inline_manifest("nope", "k")); });
  error_message(IndexErrorKind::BucketNotFound, [&] { f.index.list_objects("nope", "", "", 0); });
}

TEST_CASE("delete returns the manifest and removes it", "[manifest]") {
  Fixture f;
  f.index.put_manifest(chunked_manifest("photos", "big", 1, 3));
  auto deleted = f.index.delete_manifest("photos", "big");
  CHECK(deleted.version() == 1);
  CHECK(deleted.chunks_size() == 3);
  error_message(IndexErrorKind::ObjectNotFound, [&] { f.index.get_manifest("photos", "big"); });
  CHECK(f.index.list_chunk_refs(0, "").refs.empty());  // refs go with the manifest

  // A re-created key starts again at version 1.
  CHECK(f.index.put_manifest(inline_manifest("photos", "big")).stored.version() == 1);
}

TEST_CASE("key validation", "[manifest][validation]") {
  Fixture f;
  const std::vector<std::string> good = {"a",         "dir/file.txt", "a..b",   "...",   "a/.../b", "trailing/",
                                         "/leading",  "é/ü",          "with space", "%_\\", "a/./b"};
  const std::vector<std::string> bad = {"",   std::string("a\0b", 3), "a\nb",   "tab\there", std::string("a\x7f" "b"),
                                        "..", "../x",                 "x/..",   "a/../b",    "\xc2\x85",  // U+0085, a C1 control
                                        "x/../"};
  for (const auto& key : good) {
    INFO("good: " << key);
    CHECK_NOTHROW(f.index.put_manifest(inline_manifest("photos", key)));
  }
  for (const auto& key : bad) {
    INFO("bad: " << key);
    error_message(IndexErrorKind::InvalidArgument, [&] { f.index.put_manifest(inline_manifest("photos", key)); });
  }
}

TEST_CASE("exactly one of inline_data and chunks", "[manifest][validation]") {
  Fixture f;
  auto both = chunked_manifest("photos", "k", 1, 1);
  both.set_inline_data("x");
  error_message(IndexErrorKind::InvalidArgument, [&] { f.index.put_manifest(both); });

  auto neither = inline_manifest("photos", "k");
  neither.clear_inline_data();
  error_message(IndexErrorKind::InvalidArgument, [&] { f.index.put_manifest(neither); });

  auto short_id = chunked_manifest("photos", "k", 1, 1);
  short_id.mutable_chunks(0)->set_chunk_id("abcdef");
  error_message(IndexErrorKind::InvalidArgument, [&] { f.index.put_manifest(short_id); });
  auto upper = chunked_manifest("photos", "k", 1, 1);
  upper.mutable_chunks(0)->set_chunk_id(std::string(32, 'A'));
  error_message(IndexErrorKind::InvalidArgument, [&] { f.index.put_manifest(upper); });

  // Nothing was stored by the failed puts.
  error_message(IndexErrorKind::ObjectNotFound, [&] { f.index.get_manifest("photos", "k"); });
}

TEST_CASE("an empty inline object keeps has_inline_data", "[manifest]") {
  Fixture f;
  f.index.put_manifest(inline_manifest("photos", "empty", ""));
  auto got = f.index.get_manifest("photos", "empty");
  CHECK(got.has_inline_data());
  CHECK(got.inline_data().empty());
  CHECK(got.chunks_size() == 0);
  CHECK(got.size() == 0);
}

TEST_CASE("a manifest with 1000 chunk refs round trips", "[manifest]") {
  Fixture f;
  f.index.put_manifest(chunked_manifest("photos", "huge", 1, 1000));
  auto got = f.index.get_manifest("photos", "huge");
  REQUIRE(got.chunks_size() == 1000);
  CHECK_FALSE(got.has_inline_data());
  CHECK(got.chunks(0).chunk_id() == chunk_id(1));
  CHECK(got.chunks(999).chunk_id() == chunk_id(1000));
  CHECK(got.chunks(500).node_ids(0) == "node-1");
  CHECK(got.chunks(500).length() == 10);
  CHECK(got.chunks(500).sha256() == std::string(64, 'a'));
}

// ---- ListObjects -----------------------------------------------------------------------------

TEST_CASE("list: prefix match is literal, not a LIKE pattern", "[list]") {
  Fixture f;
  for (const char* k : {"a%b", "a%c", "axb", "a_b", "aXb", "a\\b", "a\\c", "a", "b%"}) f.put_inline(k);

  CHECK(f.keys("a%") == std::vector<std::string>{"a%b", "a%c"});
  CHECK(f.keys("a_") == std::vector<std::string>{"a_b"});
  CHECK(f.keys("a\\") == std::vector<std::string>{"a\\b", "a\\c"});
  CHECK(f.keys("%").empty());
  CHECK(f.keys("a").size() == 8);  // everything starting with "a", including "a" itself
}

TEST_CASE("list: ordering is bytewise, so case and non-ASCII sort by byte value", "[list]") {
  Fixture f;
  // Bytewise: uppercase (0x41..) < lowercase (0x61..) < multi-byte UTF-8 (0xC3.., 0xE2..).
  for (const char* k : {"b", "B", "a", "A", "é", "z", "Z", "€", "e"}) f.put_inline(k);
  CHECK(f.keys("") == std::vector<std::string>{"A", "B", "Z", "a", "b", "e", "z", "é", "€"});
  CHECK(f.keys("é") == std::vector<std::string>{"é"});
  CHECK(f.keys("a").size() == 1);  // case-sensitive: "A" is not under "a"
}

TEST_CASE("list: prefix whose successor is not valid UTF-8 still bounds correctly", "[list]") {
  Fixture f;
  // ÿ is C3 BF. Its successor (C3 C0) is not valid UTF-8, but the range scan compares bytes.
  for (const char* k : {"ÿ", "ÿa", "ÿ/x", "Ā", "z"}) f.put_inline(k);  // Ā is C4 80, just past the ÿ range
  CHECK(f.keys("ÿ") == std::vector<std::string>{"ÿ", "ÿ/x", "ÿa"});

  // A prefix of only 0xFF bytes has no successor: it means "everything >= prefix".
  const std::string ff("\xff\xff", 2);
  f.put_inline(ff + "tail");
  CHECK(f.keys(ff).size() == 1);
}

TEST_CASE("list: start_after inside and outside the prefix", "[list]") {
  Fixture f;
  for (const char* k : {"a/1", "a/2", "a/3", "b/1", "b/2"}) f.put_inline(k);

  CHECK(f.keys("a/", "a/1") == std::vector<std::string>{"a/2", "a/3"});  // inside: exclusive
  CHECK(f.keys("a/", "a/3").empty());                                     // at the last key
  CHECK(f.keys("a/", "a/").size() == 3);                                  // before the first key
  CHECK(f.keys("b/", "a/2") == std::vector<std::string>{"b/1", "b/2"});  // before the prefix: prefix wins
  CHECK(f.keys("a/", "b/1").empty());                                     // past the prefix
  CHECK(f.keys("", "a/3") == std::vector<std::string>{"b/1", "b/2"});
  CHECK(f.keys("", "zzz").empty());                                       // after everything
  CHECK(f.keys("a/", "a/1x") == std::vector<std::string>{"a/2", "a/3"}); // start_after need not be a key
}

TEST_CASE("list: pagination, truncation and next_start_after", "[list]") {
  Fixture f;
  for (int i = 0; i < 5; ++i) f.put_inline("k" + std::to_string(i));

  auto page = f.index.list_objects("photos", "", "", 2);
  REQUIRE(page.objects.size() == 2);
  CHECK(page.is_truncated);
  CHECK(page.next_start_after == "k1");

  page = f.index.list_objects("photos", "", page.next_start_after, 2);
  CHECK(page.objects[0].key() == "k2");
  CHECK(page.is_truncated);
  CHECK(page.next_start_after == "k3");

  page = f.index.list_objects("photos", "", page.next_start_after, 2);
  REQUIRE(page.objects.size() == 1);
  CHECK(page.objects[0].key() == "k4");
  CHECK_FALSE(page.is_truncated);
  CHECK(page.next_start_after.empty());
}

TEST_CASE("list: max_keys edges", "[list]") {
  Fixture f;
  for (int i = 0; i < 3; ++i) f.put_inline("k" + std::to_string(i));

  // Exactly as many keys as max_keys: full page, not truncated.
  auto exact = f.index.list_objects("photos", "", "", 3);
  CHECK(exact.objects.size() == 3);
  CHECK_FALSE(exact.is_truncated);
  CHECK(exact.next_start_after.empty());

  auto one = f.index.list_objects("photos", "", "", 1);
  CHECK(one.objects.size() == 1);
  CHECK(one.is_truncated);

  CHECK(f.index.list_objects("photos", "", "", 0).objects.size() == 3);  // 0 = default (1000)
  CHECK_NOTHROW(f.index.list_objects("photos", "", "", ts2::kMaxMaxKeys));
  error_message(IndexErrorKind::InvalidArgument,
                [&] { f.index.list_objects("photos", "", "", ts2::kMaxMaxKeys + 1); });

  CHECK(f.keys("nothing-here").empty());  // empty result, not an error
}

TEST_CASE("list: default page is 1000 keys", "[list]") {
  Fixture f;
  for (int i = 0; i < 1001; ++i) {
    char key[16];
    std::snprintf(key, sizeof key, "k%05d", i);
    f.put_inline(key);
  }
  auto page = f.index.list_objects("photos", "", "", 0);
  CHECK(page.objects.size() == 1000);
  CHECK(page.is_truncated);
  CHECK(page.next_start_after == "k00999");
}

TEST_CASE("list: summaries carry size, etag and last_modified; buckets are separate", "[list]") {
  Fixture f;
  f.index.create_bucket("docs");
  f.put_inline("k");
  f.index.put_manifest(inline_manifest("docs", "other"));

  auto page = f.index.list_objects("photos", "", "", 0);
  REQUIRE(page.objects.size() == 1);
  CHECK(page.objects[0].size() == 5);
  CHECK(page.objects[0].etag() == "etag-k");
  CHECK(page.objects[0].last_modified().seconds() == 1'700'000'000);
  CHECK(page.objects[0].last_modified().nanos() == 123);
}

// ---- ListChunkRefs ---------------------------------------------------------------------------

TEST_CASE("list_chunk_refs pages through every ref in id order, across buckets", "[chunk_refs]") {
  Fixture f;
  f.index.create_bucket("docs");
  // Ids 1..25 spread over three objects in two buckets, inserted out of id order.
  f.index.put_manifest(chunked_manifest("photos", "b", 11, 10));
  f.index.put_manifest(chunked_manifest("docs", "c", 21, 5));
  f.index.put_manifest(chunked_manifest("photos", "a", 1, 10));

  std::vector<std::string> seen;
  std::string token;
  int pages = 0;
  do {
    auto page = f.index.list_chunk_refs(10, token);
    for (const auto& r : page.refs) seen.push_back(r.chunk_id());
    token = page.next_page_token;
    ++pages;
  } while (!token.empty());

  REQUIRE(seen.size() == 25);
  CHECK(pages == 3);
  for (int i = 0; i < 25; ++i) CHECK(seen[static_cast<size_t>(i)] == chunk_id(i + 1));
}

TEST_CASE("list_chunk_refs page edges and token validation", "[chunk_refs]") {
  Fixture f;
  CHECK(f.index.list_chunk_refs(0, "").refs.empty());  // empty index

  f.index.put_manifest(chunked_manifest("photos", "a", 1, 4));
  auto exact = f.index.list_chunk_refs(4, "");
  CHECK(exact.refs.size() == 4);
  CHECK(exact.next_page_token.empty());  // exactly one full page: no extra empty page

  auto first = f.index.list_chunk_refs(3, "");
  CHECK(first.refs.size() == 3);
  CHECK(first.next_page_token == chunk_id(3));  // the token is the last id returned
  auto second = f.index.list_chunk_refs(3, first.next_page_token);
  REQUIRE(second.refs.size() == 1);
  CHECK(second.next_page_token.empty());

  CHECK(f.index.list_chunk_refs(3, chunk_id(4)).refs.empty());  // token past the end

  CHECK_NOTHROW(f.index.list_chunk_refs(ts2::kMaxChunkRefsPage, ""));
  error_message(IndexErrorKind::InvalidArgument,
                [&] { f.index.list_chunk_refs(ts2::kMaxChunkRefsPage + 1, ""); });
  error_message(IndexErrorKind::InvalidArgument, [&] { f.index.list_chunk_refs(0, "not-a-token"); });
  error_message(IndexErrorKind::InvalidArgument, [&] { f.index.list_chunk_refs(0, std::string(32, 'g')); });
}

TEST_CASE("list_chunk_refs follows overwrite, and returns full refs", "[chunk_refs]") {
  Fixture f;
  f.index.put_manifest(chunked_manifest("photos", "k", 1, 3));
  f.index.put_manifest(chunked_manifest("photos", "k", 10, 2));  // overwrite: old refs are gone
  auto refs = f.index.list_chunk_refs(0, "").refs;
  REQUIRE(refs.size() == 2);
  CHECK(refs[0].chunk_id() == chunk_id(10));
  CHECK(refs[0].node_ids(0) == "node-1");
  CHECK(refs[0].length() == 10);
  CHECK(refs[0].sha256() == std::string(64, 'a'));

  f.index.put_manifest(inline_manifest("photos", "k"));  // overwrite with an inline object: no refs
  CHECK(f.index.list_chunk_refs(0, "").refs.empty());
}

TEST_CASE("list_chunk_refs is distinct when two objects share a chunk id", "[chunk_refs]") {
  Fixture f;
  f.index.put_manifest(chunked_manifest("photos", "a", 1, 2));
  f.index.put_manifest(chunked_manifest("photos", "b", 2, 2));  // shares chunk 2
  CHECK(f.index.list_chunk_refs(0, "").refs.size() == 3);

  f.index.delete_manifest("photos", "a");
  CHECK(f.index.list_chunk_refs(0, "").refs.size() == 2);  // chunk 2 is still referenced by "b"
}

// ---- Durability, concurrency, corruption -----------------------------------------------------

TEST_CASE("data survives close and reopen", "[persistence]") {
  TempDir dir;
  auto db = dir.path() / "index.db";
  {
    SqliteIndex index(db);
    index.create_bucket("photos");
    index.put_manifest(inline_manifest("photos", "small", ""));
    index.put_manifest(chunked_manifest("photos", "big", 1, 5));
    index.put_manifest(chunked_manifest("photos", "big", 1, 5));  // version 2
  }
  SqliteIndex index(db);
  CHECK(index.get_bucket("photos").name() == "photos");
  CHECK(index.get_manifest("photos", "small").has_inline_data());
  CHECK(index.get_manifest("photos", "big").version() == 2);
  CHECK(index.list_chunk_refs(0, "").refs.size() == 5);
  // Version numbering continues after the reopen.
  CHECK(index.put_manifest(chunked_manifest("photos", "big", 1, 5)).stored.version() == 3);
}

TEST_CASE("pragmas are set on the connection", "[persistence]") {
  TempDir dir;
  SqliteIndex index(dir.path() / "index.db");
  CHECK(index.pragma("journal_mode") == "wal");
  CHECK(index.pragma("synchronous") == "2");  // FULL
  CHECK(index.pragma("foreign_keys") == "1");
  CHECK(index.pragma("busy_timeout") == "5000");
#ifdef __APPLE__
  CHECK(index.pragma("fullfsync") == "1");
  CHECK(index.pragma("checkpoint_fullfsync") == "1");
#endif
}

TEST_CASE("concurrent overwrites of one key get distinct consecutive versions", "[concurrency]") {
  Fixture f;
  constexpr int kThreads = 8;
  constexpr int kPutsPerThread = 25;
  std::vector<std::vector<uint64_t>> versions(kThreads);
  std::vector<std::thread> threads;
  for (int t = 0; t < kThreads; ++t) {
    threads.emplace_back([&, t] {
      for (int i = 0; i < kPutsPerThread; ++i) {
        // No Catch2 assertions off the main thread; record here, check afterwards.
        auto stored = f.index.put_manifest(inline_manifest("photos", "hot")).stored;
        versions[static_cast<size_t>(t)].push_back(stored.version());
      }
    });
  }
  for (auto& th : threads) th.join();

  std::set<uint64_t> all;
  for (const auto& v : versions) all.insert(v.begin(), v.end());
  CHECK(all.size() == kThreads * kPutsPerThread);  // no duplicates
  CHECK(*all.begin() == 1);
  CHECK(*all.rbegin() == kThreads * kPutsPerThread);  // no gaps
  CHECK(f.index.get_manifest("photos", "hot").version() == kThreads * kPutsPerThread);
}

TEST_CASE("concurrent puts and delete_bucket never leave objects in a deleted bucket", "[concurrency]") {
  Fixture f;
  std::thread putter([&] {
    for (int i = 0; i < 200; ++i) {
      try {
        f.index.put_manifest(inline_manifest("photos", "k" + std::to_string(i)));
      } catch (const IndexError&) {
        // BucketNotFound once the delete won; that is the allowed outcome.
      }
    }
  });
  bool deleted = false;
  for (int i = 0; i < 200 && !deleted; ++i) {
    try {
      f.index.delete_bucket("photos");
      deleted = true;
    } catch (const IndexError&) {
      // BucketNotEmpty while the putter is ahead.
    }
  }
  putter.join();
  if (deleted) {
    // The foreign key (manifests.bucket -> buckets.name) is enforced, so a manifest could not
    // have landed after the delete. Check from the outside as well.
    error_message(IndexErrorKind::BucketNotFound, [&] { f.index.list_objects("photos", "", "", 0); });
    CHECK(f.index.list_chunk_refs(0, "").refs.empty());
  }
}

TEST_CASE("a corrupt manifest blob is INTERNAL, not a crash", "[corruption]") {
  Fixture f;
  f.put_inline("good");
  f.put_inline("bad");
  raw_sql(f.dir.path() / "index.db", "UPDATE manifests SET manifest = x'ffffff' WHERE key = 'bad'");

  CHECK(f.index.get_manifest("photos", "good").inline_data() == "hello");
  error_message(IndexErrorKind::Internal, [&] { f.index.get_manifest("photos", "bad"); });

  // Delete reports INTERNAL and leaves the row in place for inspection.
  error_message(IndexErrorKind::Internal, [&] { f.index.delete_manifest("photos", "bad"); });
  CHECK(f.keys("") == std::vector<std::string>{"bad", "good"});  // listing reads columns, not the blob

  // Overwriting repairs the key; there is no readable previous manifest to return.
  auto repaired = f.index.put_manifest(inline_manifest("photos", "bad", "fixed"));
  CHECK(repaired.stored.version() == 2);
  CHECK_FALSE(repaired.previous.has_value());
  CHECK(f.index.get_manifest("photos", "bad").inline_data() == "fixed");
}

TEST_CASE("a corrupt chunk ref blob is INTERNAL", "[corruption]") {
  Fixture f;
  f.index.put_manifest(chunked_manifest("photos", "k", 1, 2));
  raw_sql(f.dir.path() / "index.db", "UPDATE chunk_refs SET ref = x'ffffff'");
  error_message(IndexErrorKind::Internal, [&] { f.index.list_chunk_refs(0, ""); });
}

TEST_CASE("an unopenable database path is INTERNAL", "[corruption]") {
  TempDir dir;
  error_message(IndexErrorKind::Internal, [&] { SqliteIndex index(dir.path() / "missing-dir" / "index.db"); });
}
