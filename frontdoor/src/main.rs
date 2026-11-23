use std::sync::Arc;
use std::time::Duration;
use ts2_core::remote::{Deadlines, RemoteChunkStore, RemoteObjectIndex};
use ts2_core::{ChunkStore, LocalChunkStore, ObjectIndex, SqliteObjectIndex};
use ts2_frontdoor::gc::{self, GcConfig};
use ts2_frontdoor::{AppState, Config, DEFAULT_PUT_TIMEOUT, router};

/// `TS2_CHUNK_NODES=node-1=chunk-store:50051,node-2=...` as (id, address) pairs.
fn parse_nodes(s: &str) -> Result<Vec<(String, String)>, String> {
    s.split(',')
        .map(|p| {
            p.split_once('=')
                .filter(|(id, addr)| !id.is_empty() && !addr.is_empty())
                .map(|(id, addr)| (id.trim().to_owned(), addr.trim().to_owned()))
                .ok_or_else(|| format!("TS2_CHUNK_NODES entry {p:?} is not id=address"))
        })
        .collect()
}

fn env_secs(name: &str, default: u64) -> Result<Duration, String> {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .map(Duration::from_secs)
            .map_err(|_| format!("{name} must be a whole number of seconds")),
        Err(_) => Ok(Duration::from_secs(default)),
    }
}

/// Per-call deadlines. The defaults are the proto values. Raise them to pause at a breakpoint without a 504.
fn deadlines_from_env() -> Result<Deadlines, String> {
    let d = Deadlines::default();
    Ok(Deadlines {
        put_chunk: env_secs("TS2_DEADLINE_PUT_CHUNK_SECS", d.put_chunk.as_secs())?,
        get_chunk: env_secs("TS2_DEADLINE_GET_CHUNK_SECS", d.get_chunk.as_secs())?,
        other: env_secs("TS2_DEADLINE_OTHER_SECS", d.other.as_secs())?,
    })
}

/// Builds the object index and chunk store that the handlers talk to.
///
/// Default mode is in-process: a SQLite index at `config.sqlite_path` and chunks on local disk under
/// `config.data_dir`. Setting both `TS2_INDEX_ADDR` and `TS2_CHUNK_NODES` switches to remote mode, where
/// both go over gRPC to ObjectIndexService and ChunkStoreService. Setting only one of them is an error.
///
/// Remote mode also starts two background tasks: a periodic chunk node health refresh, and the orphan chunk
/// garbage collector. It first checks that each node reports the id it is configured with, so a swapped
/// address fails the start instead of corrupting manifests.
async fn backends(
    config: &Config,
    put_timeout: Duration,
) -> Result<(Arc<dyn ObjectIndex>, Arc<dyn ChunkStore>), Box<dyn std::error::Error>> {
    let backends: (Arc<dyn ObjectIndex>, Arc<dyn ChunkStore>) = match (
        std::env::var("TS2_INDEX_ADDR"),
        std::env::var("TS2_CHUNK_NODES"),
    ) {
        (Ok(addr), Ok(nodes)) => {
            let deadlines = deadlines_from_env()?;
            let index = Arc::new(RemoteObjectIndex::connect_lazy(&addr, deadlines)?);
            let chunks = Arc::new(RemoteChunkStore::connect_lazy(
                &parse_nodes(&nodes)?,
                deadlines,
            )?);
            // The ids end up in every manifest, so a swapped address must stop the start, not corrupt data.
            chunks.verify_node_ids(Duration::from_secs(10)).await?;
            chunks.spawn_health_refresh(env_secs("TS2_HEALTH_INTERVAL_SECS", 5)?);
            let cfg = GcConfig {
                min_age: env_secs("TS2_GC_MIN_AGE_SECS", 3600)?,
                interval: env_secs("TS2_GC_INTERVAL_SECS", 600)?,
            };
            cfg.check_against_put_timeout(put_timeout)?;
            tracing::info!(index = %addr, chunk_nodes = %nodes, min_age = ?cfg.min_age, "remote backends, orphan gc on");
            gc::spawn(index.clone(), chunks.clone(), cfg);
            (index, chunks)
        }
        (Err(_), Err(_)) => (
            Arc::new(SqliteObjectIndex::open(&config.sqlite_path)?),
            Arc::new(LocalChunkStore::open(&config.data_dir)?),
        ),
        _ => return Err("set both TS2_INDEX_ADDR and TS2_CHUNK_NODES, or neither".into()),
    };
    Ok(backends)
}

/// Starts the frontdoor server. Steps:
/// 1. Set up JSON logging.
/// 2. Read the PUT timeout from the environment.
/// 3. Load the YAML config given as the first argument.
/// 4. Choose the storage backends (see `backends()`).
/// 5. Build the shared `AppState`.
/// 6. Start the optional OTLP metrics export.
/// 7. Serve HTTP until Ctrl+C.
///
/// Request routing is not here. It lives in `router()` in `frontdoor/src/lib.rs`, which maps each API path
/// to a handler in `handlers.rs`.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Logging: JSON lines, level from RUST_LOG (default info).
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Longest a PUT may take. GC uses it to avoid deleting chunks of an upload still in flight.
    let put_timeout = env_secs("TS2_PUT_TIMEOUT_SECS", DEFAULT_PUT_TIMEOUT.as_secs())?;
    if put_timeout.is_zero() {
        return Err("TS2_PUT_TIMEOUT_SECS must be at least 1".into());
    }
    // Config: YAML file path from the command line, and make sure its data dir exists.
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: ts2-frontdoor <config.yaml>")?;
    let config: Config = serde_yaml_ng::from_str(&std::fs::read_to_string(&path)?)?;
    std::fs::create_dir_all(&config.data_dir)?;
    // Pick the index and chunk store implementations.
    let (index, chunks) = backends(&config, put_timeout).await?;
    // Bundle everything the handlers need.
    let state = AppState {
        index,
        chunks,
        limits: config.limits,
        put_timeout,
    };

    // Metrics: export over OTLP only if OTEL_EXPORTER_OTLP_ENDPOINT is set.
    let otlp = ts2_frontdoor::metrics::init_otlp();

    // Serve until Ctrl+C, then let in-flight requests finish and flush metrics.
    let listener = tokio::net::TcpListener::bind(&config.listen).await?;
    tracing::info!(listen = %config.listen, "frontdoor started");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;
    if let Some(provider) = otlp {
        provider.shutdown().ok();
    }
    Ok(())
}
