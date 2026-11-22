#include "service.h"

#include <filesystem>
#include <functional>
#include <utility>

namespace ts2 {
namespace {

grpc::StatusCode to_code(IndexErrorKind kind) {
  switch (kind) {
    case IndexErrorKind::BucketNotFound:
    case IndexErrorKind::ObjectNotFound:
      return grpc::StatusCode::NOT_FOUND;
    case IndexErrorKind::BucketExists:
      return grpc::StatusCode::ALREADY_EXISTS;
    case IndexErrorKind::BucketNotEmpty:
      return grpc::StatusCode::FAILED_PRECONDITION;
    case IndexErrorKind::InvalidArgument:
      return grpc::StatusCode::INVALID_ARGUMENT;
    case IndexErrorKind::Unavailable:
      return grpc::StatusCode::UNAVAILABLE;
    case IndexErrorKind::Internal:
      break;
  }
  return grpc::StatusCode::INTERNAL;
}

// Runs one RPC body and turns any exception into a status, so no error can escape into gRPC.
grpc::Status guard(const std::function<void()>& body) {
  try {
    body();
    return grpc::Status::OK;
  } catch (const IndexError& e) {
    return {to_code(e.kind()), e.what()};
  } catch (const std::exception& e) {
    return {grpc::StatusCode::INTERNAL, e.what()};
  }
}

std::filesystem::path open_data_dir(const NodeConfig& config) {
  std::filesystem::create_directories(config.data_dir);
  return std::filesystem::path(config.data_dir) / "index.db";
}

}  // namespace

ObjectIndexServiceImpl::ObjectIndexServiceImpl(NodeConfig config)
    : config_(std::move(config)), index_(open_data_dir(config_)) {}

grpc::Status ObjectIndexServiceImpl::CreateBucket(grpc::ServerContext*, const v1::CreateBucketRequest* req,
                                                  v1::Bucket* resp) {
  return guard([&] { *resp = index_.create_bucket(req->name()); });
}

grpc::Status ObjectIndexServiceImpl::GetBucket(grpc::ServerContext*, const v1::GetBucketRequest* req,
                                               v1::Bucket* resp) {
  return guard([&] { *resp = index_.get_bucket(req->name()); });
}

grpc::Status ObjectIndexServiceImpl::DeleteBucket(grpc::ServerContext*, const v1::DeleteBucketRequest* req,
                                                  v1::DeleteBucketResponse*) {
  return guard([&] { index_.delete_bucket(req->name()); });
}

grpc::Status ObjectIndexServiceImpl::ListBuckets(grpc::ServerContext*, const v1::ListBucketsRequest*,
                                                 v1::ListBucketsResponse* resp) {
  return guard([&] {
    for (auto& b : index_.list_buckets()) *resp->add_buckets() = std::move(b);
  });
}

grpc::Status ObjectIndexServiceImpl::PutManifest(grpc::ServerContext*, const v1::PutManifestRequest* req,
                                                 v1::PutManifestResponse* resp) {
  return guard([&] {
    auto result = index_.put_manifest(req->manifest());
    *resp->mutable_manifest() = std::move(result.stored);
    if (result.previous) *resp->mutable_previous() = std::move(*result.previous);
  });
}

grpc::Status ObjectIndexServiceImpl::GetManifest(grpc::ServerContext*, const v1::GetManifestRequest* req,
                                                 v1::Manifest* resp) {
  return guard([&] { *resp = index_.get_manifest(req->bucket(), req->key()); });
}

grpc::Status ObjectIndexServiceImpl::DeleteManifest(grpc::ServerContext*, const v1::DeleteManifestRequest* req,
                                                    v1::Manifest* resp) {
  return guard([&] { *resp = index_.delete_manifest(req->bucket(), req->key()); });
}

grpc::Status ObjectIndexServiceImpl::ListObjects(grpc::ServerContext*, const v1::ListObjectsRequest* req,
                                                 v1::ListObjectsResponse* resp) {
  return guard([&] {
    auto page = index_.list_objects(req->bucket(), req->prefix(), req->start_after(), req->max_keys());
    for (auto& o : page.objects) *resp->add_objects() = std::move(o);
    resp->set_is_truncated(page.is_truncated);
    resp->set_next_start_after(std::move(page.next_start_after));
  });
}

grpc::Status ObjectIndexServiceImpl::ListChunkRefs(grpc::ServerContext*, const v1::ListChunkRefsRequest* req,
                                                   v1::ListChunkRefsResponse* resp) {
  return guard([&] {
    auto page = index_.list_chunk_refs(req->page_size(), req->page_token());
    for (auto& r : page.refs) *resp->add_refs() = std::move(r);
    resp->set_next_page_token(std::move(page.next_page_token));
  });
}

grpc::Status ObjectIndexServiceImpl::NodeInfo(grpc::ServerContext*, const v1::ObjectIndexNodeInfoRequest*,
                                              v1::ObjectIndexNodeInfoResponse* response) {
  response->set_node_id(config_.node_id);
  response->set_cell(config_.cell);
  response->set_healthy(true);
  response->set_version("0.1.0");
  return grpc::Status::OK;
}

}  // namespace ts2
