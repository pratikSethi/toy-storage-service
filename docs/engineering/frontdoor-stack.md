# Frontdoor stack: Rust next to Spring Boot and FastAPI

A reference for reading `frontdoor/` when you know Spring Boot or FastAPI. Crate versions are from `frontdoor/Cargo.toml`, `ts2-core/Cargo.toml` and `Cargo.lock`.

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **gRPC**: a way for services to call each other over the network (a remote procedure call, RPC), sending Protobuf messages over HTTP/2. The TS2 services talk to each other this way.
> - **SQLite**: a small database that lives in one file.

## The pieces

| Role | FastAPI | Spring Boot | Frontdoor (Rust) |
|---|---|---|---|
| Async runtime | asyncio (event loop) | None before Loom. Netty event loop in WebFlux | tokio |
| HTTP server | uvicorn (h11/httptools) | Tomcat, Jetty, Netty | hyper (`axum::serve` wraps it). It is compiled into the binary, like Spring Boot's embedded Tomcat. There is no separate server process |
| Web framework (routing, extractors, responses) | FastAPI on Starlette | Spring MVC | axum |
| Middleware | Starlette middleware | Servlet filters, interceptors | tower / tower-http |
| OpenAPI docs | Built into FastAPI | springdoc-openapi | utoipa (spec from `#[utoipa::path]`) + utoipa-scalar (the `/docs` page) |
| Models and validation | Pydantic | Jackson + Bean Validation | serde |
| gRPC client and connection reuse | grpcio | grpc-java | tonic. One HTTP/2 connection per service carries many calls |

HikariCP is a database connection pool, not a thread pool. The gRPC row is about HTTP/2 multiplexing, which is a different thing.

## Where things live

Line numbers move, so this table names files and symbols.

| Spring concept | In this repo |
|---|---|
| `@RestController`, `@RequestMapping` | `router()` in `frontdoor/src/lib.rs` |
| Controller methods | `async fn`s in `frontdoor/src/handlers.rs` |
| `@PathVariable`, `@RequestBody`, `@RequestHeader` | Extractors `Path`, `Json`, `HeaderMap`, `Body` |
| `@Service` | `ts2_core` functions, e.g. `put_object` in `ts2-core/src/object.rs` |
| `@Repository` interfaces | Traits `ObjectIndex` and `ChunkStore` in `ts2-core/src/lib.rs` |
| Repository implementations | `SqliteObjectIndex` and `LocalChunkStore` (local), `RemoteObjectIndex` and `RemoteChunkStore` (gRPC) in `ts2-core/src/remote.rs` |
| DI container | `AppState`, built in `main` and passed to handlers through `State` |
| `@ControllerAdvice` | `impl IntoResponse for ApiError` in `frontdoor/src/error.rs` |
| Filters | tower layers in `router_with_meter()` in `lib.rs`: `request_scope`, the request id layers, `metrics::track` |
| `@Profile` bean choice | `backends()` in `frontdoor/src/main.rs` |
| `SpringApplication.run` | `axum::serve(...).await` in `main` |

## How concurrent requests are handled

1. `#[tokio::main]` starts tokio's multi-thread runtime. By default it runs one worker thread per core, and idle workers steal tasks from busy ones.
2. `axum::serve` runs an accept loop. It spawns one tokio task per TCP connection.
3. An `async fn` compiles to a state machine (a future). It runs until an `.await` cannot make progress, then yields its worker thread to other tasks.
4. The OS tells tokio which sockets are ready (kqueue on macOS, epoll on Linux). Tokio then wakes the matching task.
5. Frontdoor enables axum's `http1` feature only, so a connection serves one request at a time. With HTTP/2 enabled, one connection would carry many requests at once.
6. Blocking work goes to `spawn_blocking`, which runs it on a separate thread pool. `SqliteObjectIndex` does this in `ts2-core/src/object_index.rs`, and `LocalChunkStore` does it through its `blocking()` helper in `ts2-core/src/chunk_store.rs`.

| Model | One request is | Waiting costs | What pauses it |
|---|---|---|---|
| Tomcat, classic thread pool | One OS thread | One parked OS thread (about 1 MB of stack reserved) | Any blocking call. The OS parks the thread |
| Loom virtual threads | One virtual thread | A small heap object | Any blocking call. The JVM unmounts the virtual thread |
| asyncio / uvicorn | One coroutine | A small object | `await` only. A blocking call freezes the whole event loop |
| tokio | One task (a future) | A small state machine | `.await` only. A blocking call stalls that worker thread |

The key difference from Loom: Loom makes ordinary blocking calls cheap automatically. Tokio only pauses at `.await`. A blocking call on a worker thread stalls every task queued on it, so blocking work goes through `spawn_blocking`.

The local SQLite index is one connection behind a `Mutex` (`conn: Arc<Mutex<Connection>>`). Index calls take turns, even though many requests are in flight.

## Where the accept loop is

The serve block in `frontdoor/src/main.rs`:

```rust
let listener = tokio::net::TcpListener::bind(&config.listen).await?;
axum::serve(listener, router(state))
    .with_graceful_shutdown(async {
        tokio::signal::ctrl_c().await.ok();
    })
    .await?;
```

| Line | What it does |
|---|---|
| `TcpListener::bind(...)` | Opens the listening socket. Nothing is accepted yet |
| `axum::serve(listener, router(state))` | Pairs the listener with the router. It builds a `Serve` value and does no work yet |
| `.with_graceful_shutdown(...)` | Adds a future. When it completes (Ctrl+C), the loop stops accepting and waits for open connections to finish |
| `.await?` | Runs the accept loop. `main` stays here until shutdown |

The loop is not in this repo. It is in axum 0.8.9, `src/serve/mod.rs`, in the graceful shutdown path (`~/.cargo/registry/src/*/axum-0.8.9/src/serve/mod.rs`):

```rust
loop {
    let (io, remote_addr) = tokio::select! {
        conn = listener.accept() => conn,
        _ = signal_tx.closed() => {
            trace!("signal received, not accepting new connections");
            break;
        }
    };

    handle_connection(&mut make_service, &signal_tx, &close_rx, io, remote_addr).await;
}
```

`tokio::select!` waits for whichever comes first: a new connection or the shutdown signal. `handle_connection` then calls `tokio::spawn` once per connection. That task runs hyper's `serve_connection_with_upgrades`, which parses requests and calls the router.

Tip: a breakpoint on the `listener.accept()` line stops once per new connection, not once per request. A client that reuses a connection (keep-alive) hits it only the first time.
