#pragma once

#include <memory>
#include <string>

#include "sqlite_index.h"
#include "ts2/v1/object_index.grpc.pb.h"

namespace ts2 {

struct NodeConfig {
  std::string node_id;
  std::string cell;
  std::string data_dir;  // holds index.db; created if missing
};

// Thin gRPC layer: each RPC calls SqliteIndex and maps IndexError to a gRPC status.
// All behaviour lives in SqliteIndex.
class ObjectIndexServiceImpl final : public v1::ObjectIndexService::Service {
 public:
  // Opens the database in config.data_dir. Throws IndexError if it cannot.
  explicit ObjectIndexServiceImpl(NodeConfig config);

  grpc::Status CreateBucket(grpc::ServerContext*, const v1::CreateBucketRequest*, v1::Bucket*) override;
  grpc::Status GetBucket(grpc::ServerContext*, const v1::GetBucketRequest*, v1::Bucket*) override;
  grpc::Status DeleteBucket(grpc::ServerContext*, const v1::DeleteBucketRequest*, v1::DeleteBucketResponse*) override;
  grpc::Status ListBuckets(grpc::ServerContext*, const v1::ListBucketsRequest*, v1::ListBucketsResponse*) override;
  grpc::Status PutManifest(grpc::ServerContext*, const v1::PutManifestRequest*, v1::PutManifestResponse*) override;
  grpc::Status GetManifest(grpc::ServerContext*, const v1::GetManifestRequest*, v1::Manifest*) override;
  grpc::Status DeleteManifest(grpc::ServerContext*, const v1::DeleteManifestRequest*, v1::Manifest*) override;
  grpc::Status ListObjects(grpc::ServerContext*, const v1::ListObjectsRequest*, v1::ListObjectsResponse*) override;
  grpc::Status ListChunkRefs(grpc::ServerContext*, const v1::ListChunkRefsRequest*, v1::ListChunkRefsResponse*) override;
  grpc::Status NodeInfo(grpc::ServerContext*, const v1::ObjectIndexNodeInfoRequest*,
                        v1::ObjectIndexNodeInfoResponse*) override;

 private:
  NodeConfig config_;
  SqliteIndex index_;
};

}  // namespace ts2
