//! Background orphan GC, remote mode only. The sweep itself lives in `ts2_core::gc`.

use std::sync::Arc;
use std::time::Duration;
use ts2_core::remote::{RemoteChunkStore, RemoteObjectIndex};

pub struct GcConfig {
    /// Chunks younger than this are never deleted: they may belong to an upload that has not committed yet.
    pub min_age: Duration,
    pub interval: Duration,
}

/// Sweeps every `interval` until the process exits. A failed sweep (a service is down) is logged and retried.
pub fn spawn(index: Arc<RemoteObjectIndex>, chunks: Arc<RemoteChunkStore>, cfg: GcConfig) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(cfg.interval).await;
            match ts2_core::gc::sweep(&index, &chunks, cfg.min_age).await {
                Ok(0) => tracing::debug!("orphan gc: nothing to delete"),
                Ok(n) => tracing::info!(deleted = n, "orphan gc: deleted chunks"),
                Err(e) => tracing::warn!(error = %e, "orphan gc sweep failed"),
            }
        }
    });
}
