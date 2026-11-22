#include "disk_store.h"

#include <fcntl.h>
#include <openssl/evp.h>
#include <sys/stat.h>
#include <unistd.h>

#include <algorithm>
#include <cerrno>
#include <cstring>
#include <fstream>
#include <random>
#include <system_error>
#include <utility>

namespace ts2 {

namespace fs = std::filesystem;

namespace {

StoreStatus error(Code code, std::string message) { return {code, std::move(message)}; }

// A failed system call, with errno text. A full disk is reported as RESOURCE_EXHAUSTED.
StoreStatus errno_error(const std::string& what, int err = errno) {
  auto code = (err == ENOSPC || err == EDQUOT) ? Code::kResourceExhausted : Code::kInternal;
  return error(code, what + ": " + std::strerror(err));
}

bool is_lower_hex(std::string_view s) {
  return std::all_of(s.begin(), s.end(), [](char c) { return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'); });
}

bool valid_sha256_hex(std::string_view s) { return s.size() == 64 && is_lower_hex(s); }

// Incremental sha256 (OpenSSL EVP API).
class Sha256 {
 public:
  Sha256() : ctx_(EVP_MD_CTX_new(), &EVP_MD_CTX_free) { EVP_DigestInit_ex(ctx_.get(), EVP_sha256(), nullptr); }

  void update(std::string_view bytes) { EVP_DigestUpdate(ctx_.get(), bytes.data(), bytes.size()); }

  std::string finish_hex() {
    unsigned char digest[EVP_MAX_MD_SIZE];
    unsigned int len = 0;
    EVP_DigestFinal_ex(ctx_.get(), digest, &len);
    static constexpr char kHex[] = "0123456789abcdef";
    std::string out;
    for (unsigned int i = 0; i < len; ++i) {
      out += kHex[digest[i] >> 4];
      out += kHex[digest[i] & 0x0f];
    }
    return out;
  }

 private:
  std::unique_ptr<EVP_MD_CTX, decltype(&EVP_MD_CTX_free)> ctx_;
};

// Owns a file descriptor and closes it on destruction.
class Fd {
 public:
  Fd() = default;
  explicit Fd(int fd) : fd_(fd) {}
  Fd(Fd&& other) noexcept : fd_(std::exchange(other.fd_, -1)) {}
  Fd& operator=(Fd&& other) noexcept {
    close();
    fd_ = std::exchange(other.fd_, -1);
    return *this;
  }
  ~Fd() { close(); }

  int get() const { return fd_; }
  bool valid() const { return fd_ >= 0; }
  void close() {
    if (fd_ >= 0) ::close(fd_);
    fd_ = -1;
  }

 private:
  int fd_ = -1;
};

bool write_all(int fd, std::string_view bytes) {
  while (!bytes.empty()) {
    ssize_t n = ::write(fd, bytes.data(), bytes.size());
    if (n < 0) {
      if (errno == EINTR) continue;
      return false;
    }
    bytes.remove_prefix(static_cast<size_t>(n));
  }
  return true;
}

bool read_file(const fs::path& path, std::string& out) {
  std::ifstream in(path, std::ios::binary);
  if (!in) return false;
  in.seekg(0, std::ios::end);
  auto size = in.tellg();
  if (size < 0) return false;
  in.seekg(0);
  out.resize(static_cast<size_t>(size));
  in.read(out.data(), size);
  return static_cast<bool>(in) || size == 0;
}

// Returns the stored sha256, or false when the sidecar is missing or malformed.
bool read_sidecar(const fs::path& path, std::string& sha) {
  std::string text;
  if (!read_file(path, text)) return false;
  if (text.size() > 64 && text[64] == '\n') text.resize(64);
  if (!valid_sha256_hex(text)) return false;
  sha = std::move(text);
  return true;
}

std::string random_suffix() {
  static thread_local std::mt19937_64 rng{std::random_device{}()};
  static constexpr char kHex[] = "0123456789abcdef";
  uint64_t v = rng();
  std::string out;
  for (int i = 0; i < 16; ++i, v >>= 4) out += kHex[v & 0x0f];
  return out;
}

}  // namespace

FsOps default_fs_ops() {
  FsOps ops;
  ops.sync_file = [](int fd, const fs::path&) {
#ifdef __APPLE__
    // Plain fsync on macOS only reaches the drive's cache. F_FULLFSYNC asks the drive to flush.
    if (::fcntl(fd, F_FULLFSYNC) == 0) return true;
    // Some file systems do not support it; fall back to fsync.
#endif
    return ::fsync(fd) == 0;
  };
  ops.sync_dir = [](const fs::path& dir) {
    Fd fd(::open(dir.c_str(), O_RDONLY | O_DIRECTORY));
    if (!fd.valid()) return false;
#ifdef __APPLE__
    if (::fcntl(fd.get(), F_FULLFSYNC) == 0) return true;
#endif
    return ::fsync(fd.get()) == 0;
  };
  ops.rename = [](const fs::path& from, const fs::path& to) { return ::rename(from.c_str(), to.c_str()) == 0; };
  return ops;
}

std::string sha256_hex(std::string_view bytes) {
  Sha256 h;
  h.update(bytes);
  return h.finish_hex();
}

// ---- ChunkWriter ----

struct ChunkWriter::Impl {
  Sha256 hash;
  Fd data_fd;
  Fd sidecar_fd;
};

ChunkWriter::ChunkWriter(DiskStore& store, std::string id, uint64_t expected_length, std::string expected_sha256,
                         bool already_stored)
    : store_(store),
      id_(std::move(id)),
      expected_length_(expected_length),
      expected_sha256_(std::move(expected_sha256)),
      already_stored_(already_stored),
      impl_(std::make_unique<Impl>()) {}

ChunkWriter::~ChunkWriter() { abort(); }

StoreStatus ChunkWriter::open_tmp_files() {
  if (already_stored_) return {};
  // The random suffix keeps two concurrent puts of one id from sharing a tmp file.
  tmp_data_ = store_.tmp_dir() / (id_ + "." + random_suffix());
  tmp_sidecar_ = tmp_data_;
  tmp_sidecar_ += ".sha256";
  impl_->data_fd = Fd(::open(tmp_data_.c_str(), O_WRONLY | O_CREAT | O_EXCL, 0644));
  if (!impl_->data_fd.valid()) return errno_error("create tmp file");
  impl_->sidecar_fd = Fd(::open(tmp_sidecar_.c_str(), O_WRONLY | O_CREAT | O_EXCL, 0644));
  if (!impl_->sidecar_fd.valid()) return errno_error("create tmp sidecar");
  return {};
}

void ChunkWriter::abort() {
  impl_->data_fd.close();
  impl_->sidecar_fd.close();
  std::error_code ec;  // best effort: startup clears tmp/ anyway
  if (!tmp_data_.empty()) fs::remove(tmp_data_, ec);
  if (!tmp_sidecar_.empty()) fs::remove(tmp_sidecar_, ec);
  tmp_data_.clear();
  tmp_sidecar_.clear();
}

StoreStatus ChunkWriter::append(std::string_view bytes) {
  if (length_ + bytes.size() > expected_length_) {
    return error(Code::kInvalidArgument, "received more bytes than expected_length");
  }
  impl_->hash.update(bytes);
  length_ += bytes.size();
  if (already_stored_) return {};
  if (!write_all(impl_->data_fd.get(), bytes)) return errno_error("write tmp file");
  return {};
}

StoreStatus ChunkWriter::commit() {
  if (length_ != expected_length_) {
    return error(Code::kInvalidArgument, "received " + std::to_string(length_) + " bytes, header said " +
                                             std::to_string(expected_length_));
  }
  std::string actual = impl_->hash.finish_hex();
  if (actual != expected_sha256_) {
    return error(Code::kInvalidArgument, "sha256 of received bytes does not match expected_sha256");
  }
  sha256_ = actual;
  if (already_stored_) return {};  // identical chunk is already on disk

  auto sidecar_text = actual + "\n";
  if (!write_all(impl_->sidecar_fd.get(), sidecar_text)) return errno_error("write tmp sidecar");

  // Order matters: fsync both temp files, rename the sidecar first and the data file last, then
  // fsync the directory. A crash can then leave a sidecar without data (ignored), never data
  // without a sidecar (which would read as corrupt).
  if (!store_.ops_.sync_file(impl_->data_fd.get(), tmp_data_)) return errno_error("fsync tmp file");
  if (!store_.ops_.sync_file(impl_->sidecar_fd.get(), tmp_sidecar_)) return errno_error("fsync tmp sidecar");
  impl_->data_fd.close();
  impl_->sidecar_fd.close();

  auto data_final = store_.data_path(id_);
  auto dir = data_final.parent_path();
  std::error_code ec;
  if (fs::create_directory(dir, ec)) {
    // New fan-out directory: its entry in the root must be durable too.
    if (!store_.ops_.sync_dir(store_.root_)) return errno_error("fsync root dir");
  } else if (ec) {
    return error(Code::kInternal, "create " + dir.string() + ": " + ec.message());
  }

  if (!store_.ops_.rename(tmp_sidecar_, store_.sidecar_path(id_))) return errno_error("rename sidecar");
  tmp_sidecar_.clear();
  if (!store_.ops_.rename(tmp_data_, data_final)) return errno_error("rename data file");
  tmp_data_.clear();
  if (!store_.ops_.sync_dir(dir)) return errno_error("fsync chunk dir");
  return {};
}

// ---- DiskStore ----

DiskStore::DiskStore(fs::path root, FsOps ops) : root_(std::move(root)), ops_(std::move(ops)) {}

bool DiskStore::valid_chunk_id(std::string_view id) { return id.size() == 32 && is_lower_hex(id); }

fs::path DiskStore::data_path(std::string_view id) const {
  return root_ / std::string(id.substr(0, 2)) / std::string(id);
}

fs::path DiskStore::sidecar_path(std::string_view id) const {
  auto path = data_path(id);
  path += ".sha256";
  return path;
}

StoreStatus DiskStore::open() {
  std::error_code ec;
  fs::create_directories(tmp_dir(), ec);
  if (ec) return error(Code::kInternal, "create " + tmp_dir().string() + ": " + ec.message());
  // Anything in tmp/ is a write that never finished.
  for (const auto& entry : fs::directory_iterator(tmp_dir(), ec)) {
    std::error_code rm_ec;
    fs::remove_all(entry.path(), rm_ec);
    if (rm_ec) return error(Code::kInternal, "clear tmp: " + rm_ec.message());
  }
  if (ec) return error(Code::kInternal, "scan tmp: " + ec.message());
  return {};
}

bool DiskStore::writable() const {
  return ::access(root_.c_str(), W_OK | X_OK) == 0 && ::access(tmp_dir().c_str(), W_OK | X_OK) == 0;
}

StoreStatus DiskStore::begin_put(std::string_view id, uint64_t expected_length, std::string_view expected_sha256,
                                 std::unique_ptr<ChunkWriter>& writer) {
  if (!valid_chunk_id(id)) return error(Code::kInvalidArgument, "chunk_id must be 32 lowercase hex characters");
  if (!valid_sha256_hex(expected_sha256)) {
    return error(Code::kInvalidArgument, "expected_sha256 must be 64 lowercase hex characters");
  }

  bool already_stored = false;
  std::error_code ec;
  if (fs::exists(data_path(id), ec)) {
    std::string stored;
    // A data file with no readable sidecar is corrupt, so a put is allowed to replace it.
    if (read_sidecar(sidecar_path(id), stored)) {
      if (stored != expected_sha256) {
        return error(Code::kAlreadyExists, "chunk exists with a different sha256");
      }
      already_stored = true;
    }
  }

  if (!already_stored) {
    auto space = fs::space(root_, ec);
    if (!ec && space.available < expected_length) {
      return error(Code::kResourceExhausted, "not enough free space");
    }
  }

  // Private constructor, so no make_unique.
  std::unique_ptr<ChunkWriter> w(
      new ChunkWriter(*this, std::string(id), expected_length, std::string(expected_sha256), already_stored));
  if (auto st = w->open_tmp_files(); !st.ok()) return st;
  writer = std::move(w);
  return {};
}

StoreStatus DiskStore::get(std::string_view id, uint64_t start, uint64_t end, std::string& out) {
  if (!valid_chunk_id(id)) return error(Code::kInvalidArgument, "chunk_id must be 32 lowercase hex characters");
  if (end != 0 && end <= start) return error(Code::kInvalidArgument, "range_end must be greater than range_start");

  std::string bytes;
  std::error_code ec;
  if (!fs::exists(data_path(id), ec)) return error(Code::kNotFound, "no such chunk");
  if (!read_file(data_path(id), bytes)) return error(Code::kInternal, "read chunk");

  // Verify the whole chunk before returning any of it, ranged reads included.
  std::string stored_sha;
  if (!read_sidecar(sidecar_path(id), stored_sha)) return error(Code::kDataLoss, "sidecar missing or unreadable");
  if (sha256_hex(bytes) != stored_sha) return error(Code::kDataLoss, "stored bytes do not match their sha256");

  uint64_t length = bytes.size();
  if (start > length || end > length) return error(Code::kOutOfRange, "range is past the end of the chunk");
  if (end == 0) end = length;
  out.assign(bytes, start, end - start);
  return {};
}

StoreStatus DiskStore::remove(std::string_view id) {
  if (!valid_chunk_id(id)) return error(Code::kInvalidArgument, "chunk_id must be 32 lowercase hex characters");
  std::error_code ec;
  // Sidecar first: a crash in between leaves data without a sidecar, which reads as corrupt
  // rather than as a valid chunk.
  bool removed_sidecar = fs::remove(sidecar_path(id), ec);
  if (ec) return error(Code::kInternal, "remove sidecar: " + ec.message());
  bool removed_data = fs::remove(data_path(id), ec);
  if (ec) return error(Code::kInternal, "remove data file: " + ec.message());
  if ((removed_sidecar || removed_data) && !ops_.sync_dir(data_path(id).parent_path())) {
    return errno_error("fsync chunk dir");
  }
  return {};
}

// shortcut: walks fan-out directories in order and stops when the page is full, but one directory
// is read whole. Fine for a toy store; build an in-memory or on-disk index when nodes hold millions.
StoreStatus DiskStore::list(uint32_t page_size, std::string_view token, ListPage& out) {
  // Copy first: callers may pass out.next_page_token, which `out = {}` below would clear.
  const std::string page_token(token);
  if (page_size == 0) page_size = kDefaultPageSize;
  if (page_size > kMaxPageSize) return error(Code::kInvalidArgument, "page_size is above the maximum of 10000");
  if (!page_token.empty() && !valid_chunk_id(page_token)) {
    return error(Code::kInvalidArgument, "page_token is not a chunk id");
  }
  out = {};

  std::error_code ec;
  std::vector<std::string> fanouts;
  for (const auto& entry : fs::directory_iterator(root_, ec)) {
    auto name = entry.path().filename().string();
    if (name.size() == 2 && is_lower_hex(name) && entry.is_directory()) fanouts.push_back(name);
  }
  if (ec) return error(Code::kInternal, "scan " + root_.string() + ": " + ec.message());
  std::sort(fanouts.begin(), fanouts.end());

  // Ids sort the same way as (fan-out directory, id), so directories can be visited in order.
  for (const auto& fan : fanouts) {
    if (!page_token.empty() && fan < page_token.substr(0, 2)) continue;

    std::vector<std::string> ids;
    for (const auto& entry : fs::directory_iterator(root_ / fan, ec)) {
      auto name = entry.path().filename().string();
      // Sidecars have a ".sha256" suffix, so they fail the id check and are never listed.
      if (valid_chunk_id(name) && name > page_token) ids.push_back(std::move(name));
    }
    if (ec) return error(Code::kInternal, "scan " + fan + ": " + ec.message());
    std::sort(ids.begin(), ids.end());

    for (const auto& id : ids) {
      if (out.chunks.size() == page_size) {
        out.next_page_token = out.chunks.back().id;
        return {};
      }
      struct stat st {};
      if (::stat(data_path(id).c_str(), &st) != 0) continue;  // deleted since the scan
      ChunkInfo info;
      info.id = id;
      info.length = static_cast<uint64_t>(st.st_size);
      info.created_unix = static_cast<int64_t>(st.st_mtime);
      info.corrupt = !fs::exists(sidecar_path(id), ec);
      out.chunks.push_back(std::move(info));
    }
  }
  return {};
}

}  // namespace ts2
