use std::sync::Arc;
use ts2_core::{LocalChunkStore, SqliteObjectIndex};
use ts2_frontdoor::{AppState, Config, router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let path = std::env::args().nth(1).ok_or("usage: ts2-frontdoor <config.yaml>")?;
    let config: Config = serde_yaml_ng::from_str(&std::fs::read_to_string(&path)?)?;
    std::fs::create_dir_all(&config.data_dir)?;
    let state = AppState {
        index: Arc::new(SqliteObjectIndex::open(&config.sqlite_path)?),
        chunks: Arc::new(LocalChunkStore::open(&config.data_dir)?),
        limits: config.limits,
    };

    let listener = tokio::net::TcpListener::bind(&config.listen).await?;
    tracing::info!(listen = %config.listen, "frontdoor started");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async { tokio::signal::ctrl_c().await.ok(); })
        .await?;
    Ok(())
}
