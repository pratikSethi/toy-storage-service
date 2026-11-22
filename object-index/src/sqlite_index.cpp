#include "sqlite_index.h"

#include <chrono>
#include <utility>

namespace ts2 {
namespace {

// ---- Time ------------------------------------------------------------------------------------
// Timestamps are stored as int64 nanoseconds since the Unix epoch (good until the year 2262).
constexpr int64_t kNanosPerSecond = 1'000'000'000;
constexpr int64_t kMaxSeconds = 9'000'000'000;  // keeps seconds * 1e9 inside int64

int64_t to_nanos(const google::protobuf::Timestamp& t) {
  if (t.seconds() > kMaxSeconds || t.seconds() < -kMaxSeconds) {
    throw IndexError(IndexErrorKind::InvalidArgument, "timestamp out of range");
  }
  return t.seconds() * kNanosPerSecond + t.nanos();
}

google::protobuf::Timestamp from_nanos(int64_t ns) {
  int64_t seconds = ns / kNanosPerSecond;
  int64_t nanos = ns % kNanosPerSecond;
  if (nanos < 0) {  // floor division for pre-1970 times
    nanos += kNanosPerSecond;
    --seconds;
  }
  google::protobuf::Timestamp t;
  t.set_seconds(seconds);
  t.set_nanos(static_cast<int32_t>(nanos));
  return t;
}

int64_t now_nanos() {
  return std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::system_clock::now().time_since_epoch())
      .count();
}

// ---- SQLite RAII wrappers ----------------------------------------------------------------------

[[noreturn]] void throw_sqlite(sqlite3* db, int rc, const std::string& what) {
  // BUSY/LOCKED means another connection held the lock past busy_timeout: the caller may retry.
  const bool busy = (rc & 0xff) == SQLITE_BUSY || (rc & 0xff) == SQLITE_LOCKED;
  throw IndexError(busy ? IndexErrorKind::Unavailable : IndexErrorKind::Internal,
                   what + ": " + sqlite3_errmsg(db));
}

// One prepared statement. Columns are 0-based, parameters are 1-based (SQLite's convention).
class Stmt {
 public:
  Stmt(sqlite3* db, const char* sql) : db_(db) {
    int rc = sqlite3_prepare_v2(db, sql, -1, &stmt_, nullptr);
    if (rc != SQLITE_OK) throw_sqlite(db, rc, std::string("prepare failed for: ") + sql);
  }
  ~Stmt() { sqlite3_finalize(stmt_); }
  Stmt(const Stmt&) = delete;
  Stmt& operator=(const Stmt&) = delete;

  // Text is bound with its explicit length, so no NUL-termination or UTF-8 assumptions are made.
  // That matters for the prefix successor in list_objects, which can be invalid UTF-8.
  Stmt& bind_text(int i, std::string_view s) {
    check(sqlite3_bind_text(stmt_, i, s.data(), static_cast<int>(s.size()), SQLITE_TRANSIENT));
    return *this;
  }
  Stmt& bind_blob(int i, std::string_view s) {
    check(sqlite3_bind_blob(stmt_, i, s.data(), static_cast<int>(s.size()), SQLITE_TRANSIENT));
    return *this;
  }
  Stmt& bind_int(int i, int64_t v) {
    check(sqlite3_bind_int64(stmt_, i, v));
    return *this;
  }

  // Returns true if a row is available, false when the statement is done.
  bool step() {
    int rc = sqlite3_step(stmt_);
    if (rc == SQLITE_ROW) return true;
    if (rc == SQLITE_DONE) return false;
    throw_sqlite(db_, rc, "step failed");
  }

  std::string text(int col) const {
    auto p = reinterpret_cast<const char*>(sqlite3_column_text(stmt_, col));
    return std::string(p ? p : "", static_cast<size_t>(sqlite3_column_bytes(stmt_, col)));
  }
  std::string blob(int col) const {
    auto p = static_cast<const char*>(sqlite3_column_blob(stmt_, col));
    return std::string(p ? p : "", static_cast<size_t>(sqlite3_column_bytes(stmt_, col)));
  }
  int64_t integer(int col) const { return sqlite3_column_int64(stmt_, col); }

 private:
  void check(int rc) {
    if (rc != SQLITE_OK) throw_sqlite(db_, rc, "bind failed");
  }
  sqlite3* db_;
  sqlite3_stmt* stmt_ = nullptr;
};

void exec(sqlite3* db, const char* sql) {
  char* err = nullptr;
  int rc = sqlite3_exec(db, sql, nullptr, nullptr, &err);
  if (rc != SQLITE_OK) {
    std::string msg = err ? err : "unknown error";
    sqlite3_free(err);
    const bool busy = (rc & 0xff) == SQLITE_BUSY || (rc & 0xff) == SQLITE_LOCKED;
    throw IndexError(busy ? IndexErrorKind::Unavailable : IndexErrorKind::Internal,
                     std::string("exec failed for '") + sql + "': " + msg);
  }
}

// BEGIN IMMEDIATE takes the write lock up front, so a read-then-write sequence (check the bucket,
// then insert) cannot be interleaved with another writer. Rolls back unless commit() was called.
class WriteTransaction {
 public:
  explicit WriteTransaction(sqlite3* db) : db_(db) { exec(db_, "BEGIN IMMEDIATE"); }
  ~WriteTransaction() {
    if (!done_) sqlite3_exec(db_, "ROLLBACK", nullptr, nullptr, nullptr);
  }
  WriteTransaction(const WriteTransaction&) = delete;
  WriteTransaction& operator=(const WriteTransaction&) = delete;

  void commit() {
    exec(db_, "COMMIT");
    done_ = true;
  }

 private:
  sqlite3* db_;
  bool done_ = false;
};

// ---- Schema ----------------------------------------------------------------------------------

// Written separately from the Rust SqliteObjectIndex: the manifest is a serialized protobuf, not
// JSON. chunk_refs duplicates the chunk list so ListChunkRefs is an index scan instead of decoding
// every manifest. It is kept in step with manifests inside the same transaction.
constexpr const char* kSchema = R"sql(
CREATE TABLE IF NOT EXISTS buckets (
  name TEXT PRIMARY KEY,
  created_ns INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS manifests (
  bucket TEXT NOT NULL REFERENCES buckets(name),
  key TEXT NOT NULL,
  version INTEGER NOT NULL,
  size INTEGER NOT NULL,
  etag TEXT NOT NULL,
  created_ns INTEGER NOT NULL,
  manifest BLOB NOT NULL,
  PRIMARY KEY (bucket, key)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS chunk_refs (
  chunk_id TEXT NOT NULL,
  bucket TEXT NOT NULL,
  key TEXT NOT NULL,
  ref BLOB NOT NULL,
  PRIMARY KEY (chunk_id, bucket, key)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS chunk_refs_by_object ON chunk_refs (bucket, key);
)sql";

// ---- Helpers ---------------------------------------------------------------------------------

// `name` is always a literal from our own code or a test, never user input.
std::string read_pragma(sqlite3* db, const std::string& name) {
  Stmt s(db, ("PRAGMA " + name).c_str());
  return s.step() ? s.text(0) : "";
}

bool bucket_exists(sqlite3* db, std::string_view name) {
  Stmt s(db, "SELECT 1 FROM buckets WHERE name = ?1");
  s.bind_text(1, name);
  return s.step();
}

std::string bucket_missing_message(std::string_view name) { return "bucket not found: " + std::string(name); }
std::string object_missing_message(std::string_view bucket, std::string_view key) {
  return "object not found: " + std::string(bucket) + "/" + std::string(key);
}

v1::Manifest parse_manifest(const std::string& blob, std::string_view bucket, std::string_view key) {
  v1::Manifest m;
  if (!m.ParseFromString(blob)) {
    throw IndexError(IndexErrorKind::Internal,
                     "stored manifest cannot be decoded: " + std::string(bucket) + "/" + std::string(key));
  }
  return m;
}

bool is_lower_hex(std::string_view s) {
  for (char c : s) {
    if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'))) return false;
  }
  return true;
}

constexpr size_t kChunkIdLength = 32;

// The smallest string greater than every string that starts with `prefix`: bump the last byte that
// is not 0xFF and drop everything after it. nullopt means no upper bound (empty or all-0xFF prefix).
// This turns "starts with" into a range, key >= prefix AND key < successor, which uses the primary key.
std::optional<std::string> prefix_successor(std::string_view prefix) {
  std::string s(prefix);
  while (!s.empty()) {
    auto last = static_cast<unsigned char>(s.back());
    if (last != 0xFF) {
      s.back() = static_cast<char>(last + 1);
      return s;
    }
    s.pop_back();
  }
  return std::nullopt;
}

}  // namespace

// ---- Validation ------------------------------------------------------------------------------

void validate_bucket_name(std::string_view name) {
  bool ok = name.size() >= 3 && name.size() <= 63;
  for (char c : name) {
    ok = ok && ((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-');
  }
  if (!ok) {
    throw IndexError(IndexErrorKind::InvalidArgument, "bucket name must be 3 to 63 characters of a-z, 0-9 and '-'");
  }
}

void validate_key(std::string_view key) {
  auto bad = [](const char* why) { throw IndexError(IndexErrorKind::InvalidArgument, why); };
  if (key.empty()) bad("key must not be empty");
  for (size_t i = 0; i < key.size(); ++i) {
    auto c = static_cast<unsigned char>(key[i]);
    // Control characters are C0 (0x00-0x1F), DEL (0x7F), and C1 (U+0080-U+009F, UTF-8 C2 80..C2 9F),
    // the same set as Rust's char::is_control.
    if (c < 0x20 || c == 0x7F) bad("key must not contain control characters");
    if (c == 0xC2 && i + 1 < key.size()) {
      auto next = static_cast<unsigned char>(key[i + 1]);
      if (next >= 0x80 && next <= 0x9F) bad("key must not contain control characters");
    }
  }
  // A '..' segment is a whole path component equal to "..", so "a..b" and "..." are fine.
  size_t start = 0;
  while (start <= key.size()) {
    size_t end = key.find('/', start);
    if (end == std::string_view::npos) end = key.size();
    if (key.substr(start, end - start) == "..") bad("key must not contain a '..' segment");
    start = end + 1;
  }
}

// ---- SqliteIndex -----------------------------------------------------------------------------

SqliteIndex::SqliteIndex(const std::filesystem::path& db_path) {
  sqlite3* raw = nullptr;
  int rc = sqlite3_open_v2(db_path.c_str(), &raw, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX,
                           nullptr);
  db_.reset(raw);  // owns the handle even when open failed, so it is closed
  if (rc != SQLITE_OK) {
    throw IndexError(IndexErrorKind::Internal,
                     "cannot open " + db_path.string() + ": " + (raw ? sqlite3_errmsg(raw) : "out of memory"));
  }
  sqlite3_busy_timeout(db_.get(), 5000);
  exec(db_.get(), "PRAGMA journal_mode = WAL");
  if (read_pragma(db_.get(), "journal_mode") != "wal") {
    throw IndexError(IndexErrorKind::Internal, "could not enable WAL mode on " + db_path.string());
  }
  exec(db_.get(), "PRAGMA synchronous = FULL");  // fsync on every commit: a committed put survives power loss
  exec(db_.get(), "PRAGMA foreign_keys = ON");   // per connection, off by default
#ifdef __APPLE__
  // On macOS plain fsync does not flush the drive cache; F_FULLFSYNC does.
  exec(db_.get(), "PRAGMA fullfsync = ON");
  exec(db_.get(), "PRAGMA checkpoint_fullfsync = ON");
#endif
  exec(db_.get(), kSchema);
}

SqliteIndex::~SqliteIndex() = default;

std::string SqliteIndex::pragma(const std::string& name) {
  std::lock_guard lock(mutex_);
  return read_pragma(db_.get(), name);
}

v1::Bucket SqliteIndex::create_bucket(std::string_view name) {
  validate_bucket_name(name);
  std::lock_guard lock(mutex_);
  WriteTransaction tx(db_.get());
  if (bucket_exists(db_.get(), name)) {
    throw IndexError(IndexErrorKind::BucketExists, "bucket already exists: " + std::string(name));
  }
  int64_t created = now_nanos();
  Stmt(db_.get(), "INSERT INTO buckets (name, created_ns) VALUES (?1, ?2)").bind_text(1, name).bind_int(2, created).step();
  tx.commit();
  v1::Bucket b;
  b.set_name(std::string(name));
  *b.mutable_created_at() = from_nanos(created);
  return b;
}

v1::Bucket SqliteIndex::get_bucket(std::string_view name) {
  std::lock_guard lock(mutex_);
  Stmt s(db_.get(), "SELECT created_ns FROM buckets WHERE name = ?1");
  s.bind_text(1, name);
  if (!s.step()) throw IndexError(IndexErrorKind::BucketNotFound, bucket_missing_message(name));
  v1::Bucket b;
  b.set_name(std::string(name));
  *b.mutable_created_at() = from_nanos(s.integer(0));
  return b;
}

void SqliteIndex::delete_bucket(std::string_view name) {
  std::lock_guard lock(mutex_);
  WriteTransaction tx(db_.get());
  if (!bucket_exists(db_.get(), name)) throw IndexError(IndexErrorKind::BucketNotFound, bucket_missing_message(name));
  // The emptiness check and the delete share one write transaction, so a concurrent put cannot slip in between.
  Stmt has_objects(db_.get(), "SELECT 1 FROM manifests WHERE bucket = ?1 LIMIT 1");
  has_objects.bind_text(1, name);
  if (has_objects.step()) {
    throw IndexError(IndexErrorKind::BucketNotEmpty, "bucket is not empty: " + std::string(name));
  }
  Stmt(db_.get(), "DELETE FROM buckets WHERE name = ?1").bind_text(1, name).step();
  tx.commit();
}

std::vector<v1::Bucket> SqliteIndex::list_buckets() {
  std::lock_guard lock(mutex_);
  Stmt s(db_.get(), "SELECT name, created_ns FROM buckets ORDER BY name");
  std::vector<v1::Bucket> out;
  while (s.step()) {
    v1::Bucket b;
    b.set_name(s.text(0));
    *b.mutable_created_at() = from_nanos(s.integer(1));
    out.push_back(std::move(b));
  }
  return out;
}

PutResult SqliteIndex::put_manifest(const v1::Manifest& manifest) {
  // Validate before taking the lock or touching the database.
  validate_key(manifest.key());
  // Test presence, not emptiness: an empty object has an inline_data that is set but zero bytes long.
  if (manifest.has_inline_data() == (manifest.chunks_size() > 0)) {
    throw IndexError(IndexErrorKind::InvalidArgument, "exactly one of inline_data and chunks must be set");
  }
  for (const auto& chunk : manifest.chunks()) {
    if (chunk.chunk_id().size() != kChunkIdLength || !is_lower_hex(chunk.chunk_id())) {
      throw IndexError(IndexErrorKind::InvalidArgument, "chunk_id must be 32 lowercase hex characters");
    }
  }
  if (manifest.size() > static_cast<uint64_t>(INT64_MAX)) {
    throw IndexError(IndexErrorKind::InvalidArgument, "size too large");
  }
  const int64_t created_ns = manifest.has_created_at() ? to_nanos(manifest.created_at()) : 0;

  std::lock_guard lock(mutex_);
  WriteTransaction tx(db_.get());
  if (!bucket_exists(db_.get(), manifest.bucket())) {
    throw IndexError(IndexErrorKind::BucketNotFound, bucket_missing_message(manifest.bucket()));
  }

  PutResult result;
  int64_t version = 1;
  {
    Stmt old(db_.get(), "SELECT version, manifest FROM manifests WHERE bucket = ?1 AND key = ?2");
    old.bind_text(1, manifest.bucket()).bind_text(2, manifest.key());
    if (old.step()) {
      version = old.integer(0) + 1;
      // An undecodable old blob must not block an overwrite: the new manifest repairs the key.
      // The replaced chunks are then not reported in `previous`; orphan GC reclaims them.
      v1::Manifest prev;
      if (prev.ParseFromString(old.blob(1))) result.previous = std::move(prev);
    }
  }

  result.stored = manifest;
  result.stored.set_version(static_cast<uint64_t>(version));
  std::string blob;
  if (!result.stored.SerializeToString(&blob)) {
    throw IndexError(IndexErrorKind::Internal, "cannot serialize manifest");
  }

  Stmt(db_.get(), "INSERT OR REPLACE INTO manifests (bucket, key, version, size, etag, created_ns, manifest) "
                  "VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")
      .bind_text(1, manifest.bucket())
      .bind_text(2, manifest.key())
      .bind_int(3, version)
      .bind_int(4, static_cast<int64_t>(manifest.size()))
      .bind_text(5, manifest.etag())
      .bind_int(6, created_ns)
      .bind_blob(7, blob)
      .step();

  // Replace this object's chunk refs.
  Stmt(db_.get(), "DELETE FROM chunk_refs WHERE bucket = ?1 AND key = ?2")
      .bind_text(1, manifest.bucket())
      .bind_text(2, manifest.key())
      .step();
  for (const auto& chunk : manifest.chunks()) {
    // OR IGNORE: if one manifest lists the same chunk id twice, one row is enough.
    Stmt(db_.get(), "INSERT OR IGNORE INTO chunk_refs (chunk_id, bucket, key, ref) VALUES (?1, ?2, ?3, ?4)")
        .bind_text(1, chunk.chunk_id())
        .bind_text(2, manifest.bucket())
        .bind_text(3, manifest.key())
        .bind_blob(4, chunk.SerializeAsString())
        .step();
  }
  tx.commit();
  return result;
}

v1::Manifest SqliteIndex::get_manifest(std::string_view bucket, std::string_view key) {
  std::lock_guard lock(mutex_);
  Stmt s(db_.get(), "SELECT manifest FROM manifests WHERE bucket = ?1 AND key = ?2");
  s.bind_text(1, bucket).bind_text(2, key);
  if (s.step()) return parse_manifest(s.blob(0), bucket, key);
  if (!bucket_exists(db_.get(), bucket)) throw IndexError(IndexErrorKind::BucketNotFound, bucket_missing_message(bucket));
  throw IndexError(IndexErrorKind::ObjectNotFound, object_missing_message(bucket, key));
}

v1::Manifest SqliteIndex::delete_manifest(std::string_view bucket, std::string_view key) {
  std::lock_guard lock(mutex_);
  WriteTransaction tx(db_.get());
  v1::Manifest manifest;
  {
    Stmt s(db_.get(), "SELECT manifest FROM manifests WHERE bucket = ?1 AND key = ?2");
    s.bind_text(1, bucket).bind_text(2, key);
    if (!s.step()) {
      if (!bucket_exists(db_.get(), bucket)) {
        throw IndexError(IndexErrorKind::BucketNotFound, bucket_missing_message(bucket));
      }
      throw IndexError(IndexErrorKind::ObjectNotFound, object_missing_message(bucket, key));
    }
    // Decode before deleting: a corrupt row is reported as INTERNAL and left in place for inspection.
    manifest = parse_manifest(s.blob(0), bucket, key);
  }
  Stmt(db_.get(), "DELETE FROM manifests WHERE bucket = ?1 AND key = ?2").bind_text(1, bucket).bind_text(2, key).step();
  Stmt(db_.get(), "DELETE FROM chunk_refs WHERE bucket = ?1 AND key = ?2").bind_text(1, bucket).bind_text(2, key).step();
  tx.commit();
  return manifest;
}

ListObjectsPage SqliteIndex::list_objects(std::string_view bucket, std::string_view prefix,
                                          std::string_view start_after, uint32_t max_keys) {
  if (max_keys > kMaxMaxKeys) {
    throw IndexError(IndexErrorKind::InvalidArgument, "max_keys above the maximum of " + std::to_string(kMaxMaxKeys));
  }
  if (max_keys == 0) max_keys = kDefaultMaxKeys;

  std::lock_guard lock(mutex_);
  if (!bucket_exists(db_.get(), bucket)) throw IndexError(IndexErrorKind::BucketNotFound, bucket_missing_message(bucket));

  // key > start_after AND key >= prefix [AND key < successor(prefix)]. Keys compare bytewise
  // (SQLite's default BINARY collation), and LIKE is avoided because %, _ and \ would need escaping.
  // The upper bound is a separate statement so the planner always sees a plain range on the primary key.
  auto successor = prefix_successor(prefix);
  const char* sql = successor ? "SELECT key, size, etag, created_ns FROM manifests "
                                "WHERE bucket = ?1 AND key > ?2 AND key >= ?3 AND key < ?4 ORDER BY key LIMIT ?5"
                              : "SELECT key, size, etag, created_ns FROM manifests "
                                "WHERE bucket = ?1 AND key > ?2 AND key >= ?3 ORDER BY key LIMIT ?5";
  Stmt s(db_.get(), sql);
  s.bind_text(1, bucket).bind_text(2, start_after).bind_text(3, prefix);
  if (successor) s.bind_text(4, *successor);
  s.bind_int(5, static_cast<int64_t>(max_keys) + 1);  // one extra row tells us whether the page is truncated

  ListObjectsPage page;
  while (s.step()) {
    if (page.objects.size() == max_keys) {
      page.is_truncated = true;
      break;
    }
    v1::ObjectSummary o;
    o.set_key(s.text(0));
    o.set_size(static_cast<uint64_t>(s.integer(1)));
    o.set_etag(s.text(2));
    *o.mutable_last_modified() = from_nanos(s.integer(3));
    page.objects.push_back(std::move(o));
  }
  if (page.is_truncated) page.next_start_after = page.objects.back().key();
  return page;
}

ChunkRefsPage SqliteIndex::list_chunk_refs(uint32_t page_size, std::string_view page_token) {
  if (page_size > kMaxChunkRefsPage) {
    throw IndexError(IndexErrorKind::InvalidArgument,
                     "page_size above the maximum of " + std::to_string(kMaxChunkRefsPage));
  }
  if (page_size == 0) page_size = kDefaultChunkRefsPage;
  // The token is the last chunk id returned, so it has the chunk id shape (or is empty).
  if (!page_token.empty() && (page_token.size() != kChunkIdLength || !is_lower_hex(page_token))) {
    throw IndexError(IndexErrorKind::InvalidArgument, "bad page_token");
  }

  std::lock_guard lock(mutex_);
  // GROUP BY makes the result distinct by chunk_id if two objects ever reference the same chunk.
  Stmt s(db_.get(), "SELECT chunk_id, ref FROM chunk_refs WHERE chunk_id > ?1 "
                    "GROUP BY chunk_id ORDER BY chunk_id LIMIT ?2");
  s.bind_text(1, page_token).bind_int(2, static_cast<int64_t>(page_size) + 1);

  ChunkRefsPage page;
  while (s.step()) {
    if (page.refs.size() == page_size) {
      page.next_page_token = page.refs.back().chunk_id();
      break;
    }
    v1::ChunkRef ref;
    if (!ref.ParseFromString(s.blob(1))) {
      throw IndexError(IndexErrorKind::Internal, "stored chunk ref cannot be decoded: " + s.text(0));
    }
    page.refs.push_back(std::move(ref));
  }
  return page;
}

}  // namespace ts2
