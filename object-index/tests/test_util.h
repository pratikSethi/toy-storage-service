#pragma once

#include <unistd.h>

#include <atomic>
#include <cstdio>
#include <filesystem>
#include <string>

#include "ts2/v1/object_index.pb.h"

// A fresh directory removed on destruction.
class TempDir {
 public:
  TempDir() {
    static std::atomic<int> counter{0};
    path_ = std::filesystem::temp_directory_path() /
            ("ts2-index-test-" + std::to_string(::getpid()) + "-" + std::to_string(counter++));
    std::filesystem::create_directories(path_);
  }
  ~TempDir() {
    std::error_code ec;
    std::filesystem::remove_all(path_, ec);
  }
  TempDir(const TempDir&) = delete;
  TempDir& operator=(const TempDir&) = delete;
  const std::filesystem::path& path() const { return path_; }

 private:
  std::filesystem::path path_;
};

// A chunk id is 32 lowercase hex characters. Distinct for each n.
inline std::string chunk_id(int n) {
  char buf[40];
  std::snprintf(buf, sizeof buf, "%032x", n);
  return buf;
}

inline ts2::v1::Manifest inline_manifest(const std::string& bucket, const std::string& key,
                                         const std::string& data = "hello") {
  ts2::v1::Manifest m;
  m.set_bucket(bucket);
  m.set_key(key);
  m.set_size(data.size());
  m.set_etag("etag-" + key);
  m.set_content_type("text/plain");
  m.mutable_created_at()->set_seconds(1'700'000'000);
  m.mutable_created_at()->set_nanos(123);
  m.set_inline_data(data);  // set even when empty: presence is what matters
  return m;
}

// A chunked manifest whose chunks have ids chunk_id(first) .. chunk_id(first + count - 1).
inline ts2::v1::Manifest chunked_manifest(const std::string& bucket, const std::string& key, int first, int count) {
  ts2::v1::Manifest m;
  m.set_bucket(bucket);
  m.set_key(key);
  m.set_size(static_cast<uint64_t>(count) * 10);
  m.set_etag("etag-" + key);
  m.set_chunk_bytes(10);
  m.mutable_created_at()->set_seconds(1'700'000'000);
  for (int i = 0; i < count; ++i) {
    auto* c = m.add_chunks();
    c->set_chunk_id(chunk_id(first + i));
    c->add_node_ids("node-1");
    c->set_length(10);
    c->set_sha256(std::string(64, 'a'));
  }
  return m;
}
