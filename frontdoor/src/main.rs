use std::sync::Arc;
use std::time::Duration;
use ts2_core::remote::{Deadlines, RemoteChunkStore, RemoteObjectIndex};
use ts2_core::{ChunkStore, LocalChunkStore, ObjectIndex, SqliteObjectIndex};
use ts2_frontdoor::gc::{self, GcConfig};
use ts2_frontdoor::{AppState, Config, router};

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let path = std::env::args()
        .nth(1)
        .ok_or("usage: ts2-frontdoor <config.yaml>")?;
    let config: Config = serde_yaml_ng::from_str(&std::fs::read_to_string(&path)?)?;
    std::fs::create_dir_all(&config.data_dir)?;
    // In-process backends by default. Setting both TS2_INDEX_ADDR and TS2_CHUNK_NODES switches to the gRPC services.
    let (index, chunks): (Arc<dyn ObjectIndex>, Arc<dyn ChunkStore>) = match (
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
            let cfg = GcConfig {
                min_age: env_secs("TS2_GC_MIN_AGE_SECS", 3600)?,
                interval: env_secs("TS2_GC_INTERVAL_SECS", 600)?,
            };
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
    let state = AppState {
        index,
        chunks,
        limits: config.limits,
    };

    let otlp = ts2_frontdoor::metrics::init_otlp();

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
