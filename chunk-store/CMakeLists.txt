cmake_minimum_required(VERSION 3.25)
project(ts2_chunk_store LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

find_package(Protobuf CONFIG REQUIRED)
find_package(gRPC CONFIG REQUIRED)
find_package(spdlog CONFIG REQUIRED)
find_package(Catch2 3 CONFIG REQUIRED)

# Generated C++ for the shared contract in ../proto.
set(PROTO_ROOT ${CMAKE_CURRENT_SOURCE_DIR}/../proto)
set(GEN_DIR ${CMAKE_CURRENT_BINARY_DIR}/gen)
file(MAKE_DIRECTORY ${GEN_DIR})
add_library(ts2_proto STATIC)
protobuf_generate(TARGET ts2_proto LANGUAGE cpp
  IMPORT_DIRS ${PROTO_ROOT} PROTOC_OUT_DIR ${GEN_DIR}
  PROTOS ${PROTO_ROOT}/ts2/v1/chunk_store.proto)
protobuf_generate(TARGET ts2_proto LANGUAGE grpc
  GENERATE_EXTENSIONS .grpc.pb.h .grpc.pb.cc
  PLUGIN "protoc-gen-grpc=$<TARGET_FILE:gRPC::grpc_cpp_plugin>"
  IMPORT_DIRS ${PROTO_ROOT} PROTOC_OUT_DIR ${GEN_DIR}
  PROTOS ${PROTO_ROOT}/ts2/v1/chunk_store.proto)
target_include_directories(ts2_proto PUBLIC ${GEN_DIR})
target_link_libraries(ts2_proto PUBLIC protobuf::libprotobuf gRPC::grpc++)

add_library(chunk_store_lib STATIC src/service.cpp)
target_include_directories(chunk_store_lib PUBLIC src)
target_link_libraries(chunk_store_lib PUBLIC ts2_proto spdlog::spdlog)
target_compile_options(chunk_store_lib PRIVATE -Wall -Wextra)

add_executable(ts2-chunk-store src/main.cpp)
target_link_libraries(ts2-chunk-store PRIVATE chunk_store_lib)
install(TARGETS ts2-chunk-store RUNTIME DESTINATION bin)

enable_testing()
include(Catch)
add_executable(chunk_store_tests tests/service_test.cpp)
target_link_libraries(chunk_store_tests PRIVATE chunk_store_lib Catch2::Catch2WithMain)
catch_discover_tests(chunk_store_tests)
