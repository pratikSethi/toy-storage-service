use bytes::Bytes;
use futures_util::{TryStreamExt, stream};
use std::sync::Arc;
use ts2_core::*;

fn limits() -> Limits {
    Limits { max_object_bytes: 100, max_key_bytes: 64, inline_threshold_bytes: 10, chunk_bytes: 16 }
}

fn body(parts: &[&[u8]]) -> impl futures_util::Stream<Item = std::io::Result<Bytes>> + Unpin {
    stream::iter(parts.iter().map(|p| Ok(Bytes::copy_from_slice(p))).collect::<Vec<_>>())
}

async fn setup(dir: &std::path::Path) -> (SqliteObjectIndex, Arc<LocalChunkStore>) {
    let index = SqliteObjectIndex::open(&dir.join("index.db")).unwrap();
    index.create_bucket("b").await.unwrap();
    (index, Arc::new(LocalChunkStore::open(dir).unwrap()))
}

fn chunk_files(dir: &std::path::Path) -> usize {
    walk(&dir.join("data/chunks"))
}

fn walk(p: &std::path::Path) -> usize {
    std::fs::read_dir(p).unwrap().map(|e| {
        let e = e.unwrap().path();
        if e.is_dir() { walk(&e) } else { 1 }
    }).sum()
}

#[tokio::test]
async fn inline_then_chunked_roundtrip_and_range() {
    let dir = tempfile::tempdir().unwrap();
    let (index, chunks) = setup(dir.path()).await;
    let small = put_object(body(&[b"hello"]), "b", "s", "text/plain", &limits(), &index, &*chunks).await.unwrap();
    assert!(small.inline_data.is_some() && small.chunks.is_empty());

    let data: Vec<u8> = (0..40u8).collect(); // 16 + 16 + 8
    let m = put_object(body(&[&data[..7], &data[7..]]), "b", "k", "x", &limits(), &index, &*chunks).await.unwrap();
    assert_eq!((m.version, m.chunks.len()), (1, 3));
    let all: Vec<Bytes> = object_stream(m.clone(), chunks.clone(), 0, 40).try_collect().await.unwrap();
    assert_eq!(all.concat(), data);
    let part: Vec<Bytes> = object_stream(m, chunks.clone(), 14, 18).try_collect().await.unwrap();
    assert_eq!(part.concat(), &data[14..18]);
}

#[tokio::test]
async fn too_large_deletes_written_chunks() {
    let dir = tempfile::tempdir().unwrap();
    let (index, chunks) = setup(dir.path()).await;
    let big = vec![1u8; 101];
    let r = put_object(body(&[&big[..50], &big[50..]]), "b", "k", "x", &limits(), &index, &*chunks).await;
    assert!(matches!(r, Err(Error::ObjectTooLarge { .. })));
    assert_eq!(chunk_files(dir.path()), 0);
    assert!(matches!(index.get_manifest("b", "k").await, Err(Error::ObjectNotFound)));
}

#[tokio::test]
async fn corrupt_chunk_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let (index, chunks) = setup(dir.path()).await;
    let m = put_object(body(&[&[7u8; 20]]), "b", "k", "x", &limits(), &index, &*chunks).await.unwrap();
    chunks.put(&m.chunks[0].chunk_id, Bytes::from_static(&[0u8; 16])).await.unwrap();
    let r = object_stream(m, chunks.clone(), 0, 20).try_collect::<Vec<_>>().await;
    assert!(matches!(r, Err(Error::Corrupt(_))));
}

#[tokio::test]
async fn delete_removes_chunks_and_overwrite_bumps_version() {
    let dir = tempfile::tempdir().unwrap();
    let (index, chunks) = setup(dir.path()).await;
    put_object(body(&[&[1u8; 20]]), "b", "k", "x", &limits(), &index, &*chunks).await.unwrap();
    let m = put_object(body(&[&[2u8; 5]]), "b", "k", "x", &limits(), &index, &*chunks).await.unwrap();
    assert_eq!(m.version, 2);
    put_object(body(&[&[3u8; 20]]), "b", "j", "x", &limits(), &index, &*chunks).await.unwrap();
    delete_object(&index, &*chunks, "b", "j").await.unwrap();
    assert_eq!(chunk_files(dir.path()), 2); // overwritten "k" chunks stay orphaned until GC exists
}

#[test]
fn key_validation() {
    assert!(validate_key("a/b/c.txt", 64).is_ok());
    assert!(validate_key("", 64).is_err());
    assert!(validate_key("a/../b", 64).is_err());
    assert!(validate_key(&"k".repeat(65), 64).is_err());
    assert!(validate_bucket_name("my-bucket-1").is_ok());
    assert!(validate_bucket_name("Ab").is_err());
}
