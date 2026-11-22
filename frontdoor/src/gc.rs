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

impl GcConfig {
    /// A chunk can be written at the start of a PUT and committed at its end, so it must stay younger than
    /// `min_age` for the whole PUT. Twice the timeout leaves room for the retries and the slow commit.
    pub fn check_against_put_timeout(&self, put_timeout: Duration) -> Result<(), String> {
        if self.min_age < put_timeout * 2 {
            return Err(format!(
                "TS2_GC_MIN_AGE_SECS ({}) must be at least twice TS2_PUT_TIMEOUT_SECS ({})",
                self.min_age.as_secs(),
                put_timeout.as_secs()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(min_age: u64) -> GcConfig {
        GcConfig {
            min_age: Duration::from_secs(min_age),
            interval: Duration::from_secs(600),
        }
    }

    #[test]
    fn min_age_must_be_twice_the_put_timeout() {
        let t = Duration::from_secs(900);
        assert!(cfg(1799).check_against_put_timeout(t).is_err());
        assert!(cfg(1800).check_against_put_timeout(t).is_ok());
        assert!(cfg(3600).check_against_put_timeout(t).is_ok());
    }
}
