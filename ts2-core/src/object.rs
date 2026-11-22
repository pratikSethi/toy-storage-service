use crate::{ChunkRef, ChunkStore, Error, Limits, Manifest, ObjectIndex, Result};
use bytes::{Bytes, BytesMut};
use chrono::Utc;
use futures_util::stream::{self, BoxStream};
use futures_util::{Stream, StreamExt};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Placement is decided here, not in the index: the first 8 hex digits of the (random) chunk id,
/// modulo the node count. Stable for a given node list; rebalancing is a later problem.
fn place(chunk_id: &str, nodes: &[String]) -> String {
    let n = u32::from_str_radix(&chunk_id[..8], 16).unwrap_or(0) as usize;
    nodes[n % nodes.len()].clone()
}

pub fn validate_bucket_name(name: &str) -> Result<()> {
    let ok = (3..=63).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    ok.then_some(()).ok_or(Error::InvalidBucketName)
}

/// Keys may contain slashes. Empty keys, control characters and `..` segments are rejected.
pub fn validate_key(key: &str, max_key_bytes: usize) -> Result<()> {
    let bad = |m: &str| Err(Error::InvalidKey(m.to_owned()));
    if key.is_empty() {
        return bad("key must not be empty");
    }
    if key.len() > max_key_bytes {
        return bad(&format!("key exceeds {max_key_bytes} bytes"));
    }
    if key.chars().any(char::is_control) {
        return bad("key must not contain control characters");
    }
    if key.split('/').any(|s| s == "..") {
        return bad("key must not contain a '..' segment");
    }
    Ok(())
}

async fn write_chunk(
    chunks: &dyn ChunkStore,
    data: Bytes,
    written: &mut Vec<ChunkRef>,
) -> Result<()> {
    let chunk_id = uuid::Uuid::new_v4().simple().to_string();
    let node_id = place(&chunk_id, &chunks.nodes());
    chunks.put(&node_id, &chunk_id, data.clone()).await?;
    written.push(ChunkRef {
        node_id,
        length: data.len() as u64,
        checksum: hex::encode(Sha256::digest(&data)),
        chunk_id,
    });
    Ok(())
}

/// Reads the body in `chunk_bytes` slices, writes each chunk, then commits the manifest.
/// Chunks are durable before the manifest exists. On failure the chunks already written are deleted.
pub async fn put_object<S>(
    mut body: S,
    bucket: &str,
    key: &str,
    content_type: &str,
    limits: &Limits,
    index: &dyn ObjectIndex,
    chunks: &dyn ChunkStore,
) -> Result<Manifest>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin,
{
    let mut written = Vec::new();
    let result = read_and_commit(
        &mut body,
        bucket,
        key,
        content_type,
        limits,
        index,
        chunks,
        &mut written,
    )
    .await;
    if result.is_err() {
        for c in &written {
            let _ = chunks.delete(&c.node_id, &c.chunk_id).await;
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn read_and_commit<S>(
    body: &mut S,
    bucket: &str,
    key: &str,
    content_type: &str,
    limits: &Limits,
    index: &dyn ObjectIndex,
    chunks: &dyn ChunkStore,
    written: &mut Vec<ChunkRef>,
) -> Result<Manifest>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin,
{
    let (mut buf, mut total, mut etag) = (BytesMut::new(), 0u64, Sha256::new());
    while let Some(item) = body.next().await {
        let data = item?;
        total += data.len() as u64;
        if total > limits.max_object_bytes {
            return Err(Error::ObjectTooLarge {
                max: limits.max_object_bytes,
            });
        }
        etag.update(&data);
        buf.extend_from_slice(&data);
        // Hold everything back until the object is known to be over the inline threshold.
        while buf.len() >= limits.chunk_bytes && total >= limits.inline_threshold_bytes as u64 {
            write_chunk(chunks, buf.split_to(limits.chunk_bytes).freeze(), written).await?;
        }
    }
    let inline = written.is_empty() && total < limits.inline_threshold_bytes as u64;
    let inline_data = inline.then(|| buf.to_vec());
    if !inline && !buf.is_empty() {
        write_chunk(chunks, buf.freeze(), written).await?;
    }
    let (stored, previous) = index
        .put_manifest(Manifest {
            bucket: bucket.to_owned(),
            key: key.to_owned(),
            version: 0, // assigned by the index
            size: total,
            etag: hex::encode(etag.finalize()),
            content_type: content_type.to_owned(),
            created_at: Utc::now(),
            metadata: BTreeMap::new(),
            inline_data,
            chunk_bytes: limits.chunk_bytes as u64,
            chunks: written.clone(),
        })
        .await?;
    // An overwrite replaced the old manifest, so its chunks are garbage now. Best effort: orphan GC is the safety net.
    // A GET that is still streaming the old version can fail when this lands.
    if let Some(old) = previous {
        for c in old.chunks {
            let _ = chunks.delete(&c.node_id, &c.chunk_id).await;
        }
    }
    Ok(stored)
}

/// Streams bytes `start..end` (end exclusive, already clamped to the size). Each chunk that overlaps the
/// range is read whole and checked against its checksum before it is sliced.
pub fn object_stream(
    manifest: Manifest,
    chunks: Arc<dyn ChunkStore>,
    start: u64,
    end: u64,
) -> BoxStream<'static, Result<Bytes>> {
    if start >= end {
        return stream::empty().boxed();
    }
    if let Some(data) = &manifest.inline_data {
        return stream::once(std::future::ready(Ok(Bytes::copy_from_slice(
            &data[start as usize..end as usize],
        ))))
        .boxed();
    }
    let cb = manifest.chunk_bytes;
    let manifest = Arc::new(manifest);
    stream::iter(start / cb..=(end - 1) / cb)
        .then(move |i| {
            let (m, chunks) = (manifest.clone(), chunks.clone());
            async move {
                let c = &m.chunks[i as usize];
                let data = chunks.get(&c.node_id, &c.chunk_id, None).await?;
                if data.len() as u64 != c.length || hex::encode(Sha256::digest(&data)) != c.checksum
                {
                    return Err(Error::Corrupt(c.chunk_id.clone()));
                }
                let base = i * cb;
                Ok(data.slice(
                    (start.max(base) - base) as usize..(end.min(base + c.length) - base) as usize,
                ))
            }
        })
        .boxed()
}

/// Removes the manifest first, then its chunks, so a manifest never points at a missing chunk.
/// Chunk deletes are best effort; a failure leaves orphans for a future GC.
pub async fn delete_object(
    index: &dyn ObjectIndex,
    chunks: &dyn ChunkStore,
    bucket: &str,
    key: &str,
) -> Result<()> {
    for c in index.delete_manifest(bucket, key).await?.chunks {
        let _ = chunks.delete(&c.node_id, &c.chunk_id).await;
    }
    Ok(())
}
