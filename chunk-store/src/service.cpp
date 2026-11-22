#include "service.h"

#include <spdlog/spdlog.h>

#include <algorithm>
#include <filesystem>

namespace ts2 {

namespace {

grpc::StatusCode to_grpc_code(Code code) {
  switch (code) {
    case Code::kOk: return grpc::StatusCode::OK;
    case Code::kInvalidArgument: return grpc::StatusCode::INVALID_ARGUMENT;
    case Code::kNotFound: return grpc::StatusCode::NOT_FOUND;
    case Code::kAlreadyExists: return grpc::StatusCode::ALREADY_EXISTS;
    case Code::kOutOfRange: return grpc::StatusCode::OUT_OF_RANGE;
    case Code::kDataLoss: return grpc::StatusCode::DATA_LOSS;
    case Code::kResourceExhausted: return grpc::StatusCode::RESOURCE_EXHAUSTED;
    case Code::kInternal: return grpc::StatusCode::INTERNAL;
  }
  return grpc::StatusCode::INTERNAL;
}

grpc::Status to_grpc(const StoreStatus& status) {
  if (status.ok()) return grpc::Status::OK;
  if (status.code == Code::kInternal) spdlog::error("internal error: {}", status.message);
  return {to_grpc_code(status.code), status.message};
}

}  // namespace

void apply_server_limits(grpc::ServerBuilder& builder) {
  builder.SetMaxReceiveMessageSize(kMaxMessageBytes);
  builder.SetMaxSendMessageSize(kMaxMessageBytes);
}

grpc::Status ChunkStoreServiceImpl::PutChunk(grpc::ServerContext* context,
                                             grpc::ServerReader<v1::PutChunkRequest>* reader,
                                             v1::PutChunkResponse* response) {
  v1::PutChunkRequest request;
  if (!reader->Read(&request) || !request.has_header()) {
    return {grpc::StatusCode::INVALID_ARGUMENT, "the first message must be a header"};
  }
  const auto& header = request.header();
  if (config_.require_token && header.write_token().empty()) {
    return {grpc::StatusCode::PERMISSION_DENIED, "write_token is required"};
  }

  std::unique_ptr<ChunkWriter> writer;
  if (auto st = store_.begin_put(header.chunk_id(), header.expected_length(), header.expected_sha256(), writer);
      !st.ok()) {
    return to_grpc(st);
  }

  // The writer deletes its tmp files when it goes out of scope without a commit, so every early
  // return below (bad frame, mismatch, client cancel) leaves nothing behind.
  while (reader->Read(&request)) {
    if (!request.has_data()) {
      return {grpc::StatusCode::INVALID_ARGUMENT, "only the first message may be a header"};
    }
    if (request.data().size() > kMaxFrameBytes) {
      return {grpc::StatusCode::INVALID_ARGUMENT, "data frame is larger than 1 MiB"};
    }
    if (auto st = writer->append(request.data()); !st.ok()) return to_grpc(st);
  }
  if (context->IsCancelled()) return {grpc::StatusCode::CANCELLED, "client cancelled the upload"};

  if (auto st = writer->commit(); !st.ok()) return to_grpc(st);
  response->set_stored_length(writer->length());
  response->set_sha256(writer->sha256());
  return grpc::Status::OK;
}

grpc::Status ChunkStoreServiceImpl::GetChunk(grpc::ServerContext*, const v1::GetChunkRequest* request,
                                             grpc::ServerWriter<v1::GetChunkResponse>* writer) {
  // The store verifies the whole chunk first, so a corrupt chunk fails before the first frame.
  std::string bytes;
  if (auto st = store_.get(request->chunk_id(), request->range_start(), request->range_end(), bytes); !st.ok()) {
    return to_grpc(st);
  }
  for (size_t pos = 0; pos < bytes.size(); pos += kMaxFrameBytes) {
    v1::GetChunkResponse frame;
    frame.set_data(bytes.substr(pos, kMaxFrameBytes));
    if (!writer->Write(frame)) return {grpc::StatusCode::CANCELLED, "client went away"};
  }
  return grpc::Status::OK;
}

grpc::Status ChunkStoreServiceImpl::DeleteChunk(grpc::ServerContext*, const v1::DeleteChunkRequest* request,
                                                v1::DeleteChunkResponse*) {
  return to_grpc(store_.remove(request->chunk_id()));
}

grpc::Status ChunkStoreServiceImpl::ListChunks(grpc::ServerContext*, const v1::ListChunksRequest* request,
                                               v1::ListChunksResponse* response) {
  ListPage page;
  if (auto st = store_.list(request->page_size(), request->page_token(), page); !st.ok()) return to_grpc(st);
  for (const auto& chunk : page.chunks) {
    auto* info = response->add_chunks();
    info->set_chunk_id(chunk.id);
    info->set_length(chunk.length);
    info->set_created_unix(chunk.created_unix);
    info->set_corrupt(chunk.corrupt);
  }
  response->set_next_page_token(page.next_page_token);
  return grpc::Status::OK;
}

grpc::Status ChunkStoreServiceImpl::NodeInfo(grpc::ServerContext*, const v1::ChunkStoreNodeInfoRequest*,
                                             v1::ChunkStoreNodeInfoResponse* response) {
  std::error_code ec;
  auto space = std::filesystem::space(store_.root(), ec);
  response->set_node_id(config_.node_id);
  response->set_cell(config_.cell);
  response->set_failure_domain(config_.failure_domain);
  response->set_bytes_free(ec ? 0 : space.available);
  response->set_bytes_total(ec ? 0 : space.capacity);
  response->set_healthy(!ec && store_.writable());
  response->set_version("0.1.0");
  return grpc::Status::OK;
}

}  // namespace ts2
