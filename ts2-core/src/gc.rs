//! Orphan chunk garbage collection. Frontdoor owns it (review 002): the chunk nodes know which
//! chunks they hold, the index knows which are referenced, and only the gateway talks to both.

use crate::ChunkStore;
use crate::Result;
use crate::remote::{RemoteChunkStore, RemoteObjectIndex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// One sweep. Returns how many chunks it deleted.
///
/// Order matters: list each node's chunks BEFORE listing the index refs. An upload that commits
/// between the two lists then has its chunks in the first list and its refs in the second, so they
/// are kept. The reverse order would delete them. The age rule covers uploads that have written
/// chunks but not yet committed: a chunk younger than `min_age` is never deleted.
pub async fn sweep(
    index: &RemoteObjectIndex,
    chunks: &RemoteChunkStore,
    min_age: Duration,
) -> Result<usize> {
    let mut listed = Vec::new();
    for node in chunks.configured_nodes() {
        for c in chunks.list_chunks(&node).await? {
            listed.push((node.clone(), c));
        }
    }
    let referenced = index.list_chunk_refs().await?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let cutoff = now - min_age.as_secs() as i64;

    let mut deleted = 0;
    for (node, c) in listed {
        if !referenced.contains(&c.chunk_id) && c.created_unix <= cutoff {
            // One failed delete should not stop the sweep; the next sweep retries it.
            if chunks.delete(&node, &c.chunk_id).await.is_ok() {
                deleted += 1;
            }
        }
    }
    Ok(deleted)
}
