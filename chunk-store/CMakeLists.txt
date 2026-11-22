cmake_minimum_required(VERSION 3.25)
project(ts2_chunk_store LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

find_package(Protobuf CONFIG REQUIRED)
find_package(gRPC CONFIG REQUIRED)
find_package(spdlog CONFIG REQUIRED)
find_package(OpenSSL REQUIRED)  # sha256, already pulled in by gRPC
find_package(Catch2 3 CONFIG REQUIRED)

# Generated C++ and gRPC code for the shared contract in ../proto.
include(${CMAKE_CURRENT_SOURCE_DIR}/../cmake/ts2_proto.cmake)

add_library(chunk_store_lib STATIC src/disk_store.cpp src/service.cpp)
target_include_directories(chunk_store_lib PUBLIC src)
target_link_libraries(chunk_store_lib PUBLIC ts2_proto spdlog::spdlog OpenSSL::Crypto)
target_compile_options(chunk_store_lib PRIVATE -Wall -Wextra)

add_executable(ts2-chunk-store src/main.cpp)
target_link_libraries(ts2-chunk-store PRIVATE chunk_store_lib)
install(TARGETS ts2-chunk-store RUNTIME DESTINATION bin)

enable_testing()
include(Catch)
add_executable(chunk_store_tests tests/disk_store_test.cpp tests/service_test.cpp)
target_link_libraries(chunk_store_tests PRIVATE chunk_store_lib Catch2::Catch2WithMain)
catch_discover_tests(chunk_store_tests)
