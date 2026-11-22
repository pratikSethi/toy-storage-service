cmake_minimum_required(VERSION 3.25)
project(ts2_object_index LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

find_package(Protobuf CONFIG REQUIRED)
find_package(gRPC CONFIG REQUIRED)
find_package(spdlog CONFIG REQUIRED)
find_package(Catch2 3 CONFIG REQUIRED)
find_package(SQLite3 REQUIRED)

# Generated C++ and gRPC code for the shared contract in ../proto.
include(${CMAKE_CURRENT_SOURCE_DIR}/../cmake/ts2_proto.cmake)

add_library(object_index_lib STATIC src/service.cpp)
target_include_directories(object_index_lib PUBLIC src)
target_link_libraries(object_index_lib PUBLIC ts2_proto spdlog::spdlog SQLite::SQLite3)
target_compile_options(object_index_lib PRIVATE -Wall -Wextra)

add_executable(ts2-object-index src/main.cpp)
target_link_libraries(ts2-object-index PRIVATE object_index_lib)
install(TARGETS ts2-object-index RUNTIME DESTINATION bin)

enable_testing()
include(Catch)
add_executable(object_index_tests tests/service_test.cpp)
target_link_libraries(object_index_tests PRIVATE object_index_lib Catch2::Catch2WithMain)
catch_discover_tests(object_index_tests)
