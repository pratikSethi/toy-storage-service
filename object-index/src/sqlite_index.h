#pragma once

#include <sqlite3.h>

#include <cstdint>
#include <filesystem>
#include <memory>
#include <mutex>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

#include "ts2/v1/object_index.pb.h"

namespace ts2 {

// Every failure the index reports. The gRPC layer maps each kind to one status code.
enum class IndexErrorKind {
  BucketNotFound,
  ObjectNotFound,
  BucketExists,
  BucketNotEmpty,
  InvalidArgument,
  Unavailable,  // database busy or locked past busy_timeout
  Internal,     // storage failure or a stored manifest that cannot be decoded
};

class IndexError : public std::runtime_error {
 public:
  IndexError(IndexErrorKind kind, const std::string& message) : std::runtime_error(message), kind_(kind) {}
  IndexErrorKind kind() const { return kind_; }

 private:
  IndexErrorKind kind_;
};

inline constexpr uint32_t kDefaultMaxKeys = 1000;
inline constexpr uint32_t kMaxMaxKeys = 1000;
inline constexpr uint32_t kDefaultChunkRefsPage = 1000;
inline constexpr uint32_t kMaxChunkRefsPage = 10000;

// Validation rules from the proto header (same as ts2-core validate_bucket_name / validate_key,
// minus the key length limit, which is frontdoor config). Throw IndexError(InvalidArgument).
void validate_bucket_name(std::string_view name);
void validate_key(std::string_view key);

struct PutResult {
  v1::Manifest stored;                  // with the assigned version
  std::optional<v1::Manifest> previous;  // the manifest this call replaced, if any
};

struct ListObjectsPage {
  std::vector<v1::ObjectSummary> objects;
  bool is_truncated = false;
  std::string next_start_after;  // empty when not truncated
};

struct ChunkRefsPage {
  std::vector<v1::ChunkRef> refs;
  std::string next_page_token;  // empty when there are no more pages
};

// SQLite-backed object index. Knows nothing about gRPC, so it is unit-testable on its own.
//
// Thread safety: one connection guarded by one mutex. Every public method takes the lock, so calls
// are serialized. That is enough for v0.1 (SQLite allows one writer anyway); a read-connection
// pool is the upgrade if list calls start to block writes. Writes also use BEGIN IMMEDIATE so
// a second process on the same file would wait on busy_timeout rather than fail midway.
//
// All methods throw IndexError.
class SqliteIndex {
 public:
  // Opens (creating if needed) the database file and applies the schema.
  explicit SqliteIndex(const std::filesystem::path& db_path);
  ~SqliteIndex();
  SqliteIndex(const SqliteIndex&) = delete;
  SqliteIndex& operator=(const SqliteIndex&) = delete;

  v1::Bucket create_bucket(std::string_view name);
  v1::Bucket get_bucket(std::string_view name);
  void delete_bucket(std::string_view name);  // only when empty; the check is atomic with the delete
  std::vector<v1::Bucket> list_buckets();

  // Assigns version = previous + 1 (1 for a new key). Everything else is stored as given.
  PutResult put_manifest(const v1::Manifest& manifest);
  v1::Manifest get_manifest(std::string_view bucket, std::string_view key);
  v1::Manifest delete_manifest(std::string_view bucket, std::string_view key);

  // max_keys 0 means the default. Keys are ordered bytewise.
  ListObjectsPage list_objects(std::string_view bucket, std::string_view prefix, std::string_view start_after,
                               uint32_t max_keys);

  // Distinct by chunk_id, ascending. page_size 0 means the default.
  ChunkRefsPage list_chunk_refs(uint32_t page_size, std::string_view page_token);

  // Current value of a PRAGMA on the connection, for tests and diagnostics.
  std::string pragma(const std::string& name);

 private:
  struct Closer {
    void operator()(sqlite3* db) const { sqlite3_close(db); }
  };
  std::mutex mutex_;
  std::unique_ptr<sqlite3, Closer> db_;
};

}  // namespace ts2
