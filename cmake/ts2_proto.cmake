# Shared by chunk-store/ and object-index/: generates C++ and gRPC code for every proto in
# proto/ts2/v1 into one static library, ts2_proto. Include after find_package(Protobuf gRPC).
set(PROTO_ROOT ${CMAKE_CURRENT_LIST_DIR}/../proto)
set(GEN_DIR ${CMAKE_CURRENT_BINARY_DIR}/gen)
file(MAKE_DIRECTORY ${GEN_DIR})

set(TS2_PROTOS
  ${PROTO_ROOT}/ts2/v1/chunk_store.proto
  ${PROTO_ROOT}/ts2/v1/object_index.proto)

# Protobuf_INCLUDE_DIRS lets protoc find google/protobuf/timestamp.proto on every platform.
add_library(ts2_proto STATIC)
protobuf_generate(TARGET ts2_proto LANGUAGE cpp
  IMPORT_DIRS ${PROTO_ROOT} ${Protobuf_INCLUDE_DIRS} PROTOC_OUT_DIR ${GEN_DIR}
  PROTOS ${TS2_PROTOS})
protobuf_generate(TARGET ts2_proto LANGUAGE grpc
  GENERATE_EXTENSIONS .grpc.pb.h .grpc.pb.cc
  PLUGIN "protoc-gen-grpc=$<TARGET_FILE:gRPC::grpc_cpp_plugin>"
  IMPORT_DIRS ${PROTO_ROOT} ${Protobuf_INCLUDE_DIRS} PROTOC_OUT_DIR ${GEN_DIR}
  PROTOS ${TS2_PROTOS})
target_include_directories(ts2_proto PUBLIC ${GEN_DIR})
target_link_libraries(ts2_proto PUBLIC protobuf::libprotobuf gRPC::grpc++)
