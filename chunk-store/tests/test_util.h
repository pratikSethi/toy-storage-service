#pragma once

// Small helpers shared by the store and service tests.

#include <unistd.h>

#include <cstdio>
#include <filesystem>
#include <fstream>
#include <string>

#include "disk_store.h"

namespace ts2::testing {

// A fresh directory that is deleted when the object goes out of scope.
class TempDir {
 public:
  TempDir() {
    auto base = std::filesystem::temp_directory_path() / "ts2-chunk-store-test-XXXXXX";
    std::string pattern = base.string();
    path_ = ::mkdtemp(pattern.data());
  }
  ~TempDir() {
    std::error_code ec;
    // The permission test may leave a read-only directory behind.
    std::filesystem::permissions(path_ / "tmp", std::filesystem::perms::owner_all, ec);
    std::filesystem::remove_all(path_, ec);
  }
  const std::filesystem::path& path() const { return path_; }

 private:
  std::filesystem::path path_;
};

// A valid chunk id: two hex characters of fan-out directory, then n in the remaining 30.
inline std::string make_id(unsigned fanout, unsigned n) {
  char buf[40];
  std::snprintf(buf, sizeof buf, "%02x%030x", fanout, n);
  return buf;
}

inline std::string make_bytes(size_t size, char seed = 'a') {
  std::string s(size, '\0');
  for (size_t i = 0; i < size; ++i) s[i] = static_cast<char>(seed + static_cast<char>(i % 23));
  return s;
}

// Number of regular files under dir (recursive).
inline size_t count_files(const std::filesystem::path& dir) {
  size_t n = 0;
  for (const auto& e : std::filesystem::recursive_directory_iterator(dir)) {
    if (e.is_regular_file()) ++n;
  }
  return n;
}

// Stores a whole chunk in one call through the writer, the way the service does.
inline StoreStatus put_chunk(DiskStore& store, const std::string& id, const std::string& bytes) {
  std::unique_ptr<ChunkWriter> writer;
  auto st = store.begin_put(id, bytes.size(), sha256_hex(bytes), writer);
  if (!st.ok()) return st;
  st = writer->append(bytes);
  if (!st.ok()) return st;
  return writer->commit();
}

}  // namespace ts2::testing
