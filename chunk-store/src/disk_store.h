#pragma once

// On-disk chunk store. No gRPC in here, so it can be unit tested on its own.
//
// Layout under the root directory (TS2_DATA_DIR):
//   <root>/<xx>/<id>          chunk bytes   (xx = first two characters of the id)
//   <root>/<xx>/<id>.sha256   sidecar: 64 lowercase hex characters and a newline
//   <root>/tmp/<id>.<random>[.sha256]   in-flight writes, cleared at startup

#include <cstdint>
#include <filesystem>
#include <functional>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

namespace ts2 {

enum class Code {
  kOk,
  kInvalidArgument,
  kNotFound,
  kAlreadyExists,
  kOutOfRange,
  kDataLoss,
  kResourceExhausted,
  kInternal,
};

struct StoreStatus {
  Code code = Code::kOk;
  std::string message;

  bool ok() const { return code == Code::kOk; }
};

// The calls that make a write durable, behind one seam. Tests replace them to record call order.
// Each returns true on success; on failure errno describes the error.
struct FsOps {
  std::function<bool(int fd, const std::filesystem::path& path)> sync_file;
  std::function<bool(const std::filesystem::path& dir)> sync_dir;
  std::function<bool(const std::filesystem::path& from, const std::filesystem::path& to)> rename;
};

// fcntl(F_FULLFSYNC) on macOS (plain fsync does not reach the platter there), fsync elsewhere.
FsOps default_fs_ops();

// Lowercase hex sha256 (64 characters).
std::string sha256_hex(std::string_view bytes);

struct ChunkInfo {
  std::string id;
  uint64_t length = 0;
  int64_t created_unix = 0;  // data file mtime
  bool corrupt = false;      // data file without a sidecar
};

struct ListPage {
  std::vector<ChunkInfo> chunks;
  std::string next_page_token;  // last id of this page, empty when there are no more pages
};

class DiskStore;

// One in-flight PutChunk. Bytes go to tmp/ first; commit() verifies and moves them into place.
// Destroying a writer that was not committed deletes its tmp files (error or client cancel).
class ChunkWriter {
 public:
  ~ChunkWriter();
  ChunkWriter(const ChunkWriter&) = delete;
  ChunkWriter& operator=(const ChunkWriter&) = delete;

  StoreStatus append(std::string_view bytes);
  StoreStatus commit();

  uint64_t length() const { return length_; }
  const std::string& sha256() const { return sha256_; }  // valid after a successful commit

 private:
  friend class DiskStore;
  ChunkWriter(DiskStore& store, std::string id, uint64_t expected_length, std::string expected_sha256,
              bool already_stored);
  StoreStatus open_tmp_files();
  void abort();

  struct Impl;  // hash state and file descriptors, kept out of the header
  DiskStore& store_;
  std::string id_;
  uint64_t expected_length_;
  std::string expected_sha256_;
  // The same chunk is already stored: still receive and verify the bytes, but do not rewrite them.
  bool already_stored_;
  uint64_t length_ = 0;
  std::string sha256_;
  std::filesystem::path tmp_data_;
  std::filesystem::path tmp_sidecar_;
  std::unique_ptr<Impl> impl_;
};

class DiskStore {
 public:
  static constexpr uint32_t kDefaultPageSize = 1000;
  static constexpr uint32_t kMaxPageSize = 10000;

  explicit DiskStore(std::filesystem::path root, FsOps ops = default_fs_ops());

  // Creates the directories and clears tmp/. Call once at startup, before serving.
  StoreStatus open();

  // True when the root and tmp/ can be written to. Used for NodeInfo.healthy.
  bool writable() const;

  const std::filesystem::path& root() const { return root_; }

  // Exactly 32 lowercase hex characters. This is also the path traversal guard.
  static bool valid_chunk_id(std::string_view id);

  // Starts a put. ALREADY_EXISTS if the chunk exists with a different sha256.
  StoreStatus begin_put(std::string_view id, uint64_t expected_length, std::string_view expected_sha256,
                        std::unique_ptr<ChunkWriter>& writer);

  // Reads the whole chunk, verifies it against the sidecar, then returns bytes [start, end).
  // end == 0 means "to the end".
  StoreStatus get(std::string_view id, uint64_t start, uint64_t end, std::string& out);

  // Idempotent: a missing chunk is not an error.
  StoreStatus remove(std::string_view id);

  // page_size 0 means the default. page_token is the last id of the previous page.
  StoreStatus list(uint32_t page_size, std::string_view page_token, ListPage& out);

 private:
  friend class ChunkWriter;
  std::filesystem::path data_path(std::string_view id) const;
  std::filesystem::path sidecar_path(std::string_view id) const;
  std::filesystem::path tmp_dir() const { return root_ / "tmp"; }

  std::filesystem::path root_;
  FsOps ops_;
};

}  // namespace ts2
