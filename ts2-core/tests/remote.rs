//! The tonic clients against in-process fake servers, so no C++ binaries are needed.

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{TryStreamExt, stream};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tonic::{Code, Request, Response, Status, Streaming};
use ts2_core::remote::{Deadlines, RemoteChunkStore, RemoteObjectIndex};
use ts2_core::*;
use ts2_proto::v1 as pb;
use ts2_proto::v1::chunk_store_service_server::{ChunkStoreService, ChunkStoreServiceServer};
use ts2_proto::v1::object_index_service_server::{ObjectIndexService, ObjectIndexServiceServer};

type Rpc<T> = std::result::Result<Response<T>, Status>;

#[derive(Default)]
struct Fake {
    /// chunk id to (bytes, created_unix)
    chunks: HashMap<String, (Vec<u8>, i64)>,
    buckets: BTreeSet<String>,
    manifests: HashMap<(String, String), pb::Manifest>,
    /// Every RPC on that service fails with this code.
    chunk_fail: Option<(Code, String)>,
    index_fail: Option<(Code, String)>,
    /// PutChunk fails with UNAVAILABLE once this many puts have succeeded.
    puts_before_down: Option<usize>,
    puts: usize,
    put_delay: Duration,
    /// Frame sizes of the last PutChunk.
    put_frames: Vec<usize>,
    /// Runs once, right after ListChunks has answered. Used to commit an upload between the two GC lists.
    after_list_chunks: Option<Hook>,
}

type Shared = Arc<Mutex<Fake>>;
type Hook = Box<dyn FnOnce(&mut Fake) + Send>;

fn fail(code: &Option<(Code, String)>) -> std::result::Result<(), Status> {
    code.as_ref()
        .map_or(Ok(()), |(c, m)| Err(Status::new(*c, m.clone())))
}

struct ChunkSvc(Shared);

#[async_trait]
impl ChunkStoreService for ChunkSvc {
    async fn put_chunk(
        &self,
        req: Request<Streaming<pb::PutChunkRequest>>,
    ) -> Rpc<pb::PutChunkResponse> {
        let mut s = req.into_inner();
        let (mut id, mut data, mut frames) = (String::new(), Vec::new(), Vec::new());
        while let Some(m) = s.message().await? {
            match m.payload {
                Some(pb::put_chunk_request::Payload::Header(h)) => {
                    assert_eq!(
                        h.expected_sha256.len(),
                        64,
                        "header must carry a hex sha256"
                    );
                    id = h.chunk_id;
                }
                Some(pb::put_chunk_request::Payload::Data(d)) => {
                    frames.push(d.len());
                    data.extend(d);
                }
                None => {}
            }
        }
        let delay = {
            let mut f = self.0.lock().unwrap();
            fail(&f.chunk_fail)?;
            if f.puts_before_down.is_some_and(|n| f.puts >= n) {
                return Err(Status::unavailable("node went away"));
            }
            f.puts += 1;
            f.put_frames = frames;
            f.put_delay
        };
        tokio::time::sleep(delay).await;
        let len = data.len() as u64;
        self.0.lock().unwrap().chunks.insert(id, (data, 0));
        Ok(Response::new(pb::PutChunkResponse {
            stored_length: len,
            sha256: String::new(),
        }))
    }

    type GetChunkStream =
        stream::Iter<std::vec::IntoIter<std::result::Result<pb::GetChunkResponse, Status>>>;

    async fn get_chunk(&self, req: Request<pb::GetChunkRequest>) -> Rpc<Self::GetChunkStream> {
        let r = req.into_inner();
        let f = self.0.lock().unwrap();
        fail(&f.chunk_fail)?;
        let (data, _) = f
            .chunks
            .get(&r.chunk_id)
            .ok_or_else(|| Status::not_found("no such chunk"))?;
        let end = if r.range_end == 0 {
            data.len()
        } else {
            r.range_end as usize
        };
        let body = data[r.range_start as usize..end].to_vec();
        Ok(Response::new(stream::iter(vec![Ok(
            pb::GetChunkResponse { data: body },
        )])))
    }

    async fn delete_chunk(
        &self,
        req: Request<pb::DeleteChunkRequest>,
    ) -> Rpc<pb::DeleteChunkResponse> {
        let mut f = self.0.lock().unwrap();
        fail(&f.chunk_fail)?;
        f.chunks.remove(&req.into_inner().chunk_id);
        Ok(Response::new(pb::DeleteChunkResponse {}))
    }

    async fn list_chunks(&self, _: Request<pb::ListChunksRequest>) -> Rpc<pb::ListChunksResponse> {
        let mut f = self.0.lock().unwrap();
        fail(&f.chunk_fail)?;
        let mut chunks: Vec<_> = f
            .chunks
            .iter()
            .map(|(id, (d, t))| pb::ChunkInfo {
                chunk_id: id.clone(),
                length: d.len() as u64,
                created_unix: *t,
                corrupt: false,
            })
            .collect();
        chunks.sort_by(|a, b| a.chunk_id.cmp(&b.chunk_id));
        if let Some(hook) = f.after_list_chunks.take() {
            hook(&mut f);
        }
        Ok(Response::new(pb::ListChunksResponse {
            chunks,
            next_page_token: String::new(),
        }))
    }

    async fn node_info(
        &self,
        _: Request<pb::ChunkStoreNodeInfoRequest>,
    ) -> Rpc<pb::ChunkStoreNodeInfoResponse> {
        Ok(Response::new(pb::ChunkStoreNodeInfoResponse::default()))
    }
}

struct IndexSvc(Shared);

#[async_trait]
impl ObjectIndexService for IndexSvc {
    async fn create_bucket(&self, req: Request<pb::CreateBucketRequest>) -> Rpc<pb::Bucket> {
        let mut f = self.0.lock().unwrap();
        fail(&f.index_fail)?;
        let name = req.into_inner().name;
        f.buckets.insert(name.clone());
        Ok(Response::new(pb::Bucket {
            name,
            created_at: Some(Default::default()),
        }))
    }
    async fn get_bucket(&self, req: Request<pb::GetBucketRequest>) -> Rpc<pb::Bucket> {
        let f = self.0.lock().unwrap();
        fail(&f.index_fail)?;
        let name = req.into_inner().name;
        if f.buckets.contains(&name) {
            Ok(Response::new(pb::Bucket {
                name,
                created_at: None,
            }))
        } else {
            Err(Status::not_found(format!("bucket not found: {name}")))
        }
    }
    async fn delete_bucket(
        &self,
        _: Request<pb::DeleteBucketRequest>,
    ) -> Rpc<pb::DeleteBucketResponse> {
        fail(&self.0.lock().unwrap().index_fail)?;
        Ok(Response::new(pb::DeleteBucketResponse {}))
    }
    async fn list_buckets(
        &self,
        _: Request<pb::ListBucketsRequest>,
    ) -> Rpc<pb::ListBucketsResponse> {
        fail(&self.0.lock().unwrap().index_fail)?;
        Ok(Response::new(pb::ListBucketsResponse::default()))
    }
    async fn put_manifest(
        &self,
        req: Request<pb::PutManifestRequest>,
    ) -> Rpc<pb::PutManifestResponse> {
        let mut f = self.0.lock().unwrap();
        fail(&f.index_fail)?;
        let mut m = req.into_inner().manifest.unwrap();
        let k = (m.bucket.clone(), m.key.clone());
        let previous = f.manifests.get(&k).cloned();
        m.version = previous.as_ref().map_or(1, |p| p.version + 1);
        f.manifests.insert(k, m.clone());
        Ok(Response::new(pb::PutManifestResponse {
            manifest: Some(m),
            previous,
        }))
    }
    async fn get_manifest(&self, req: Request<pb::GetManifestRequest>) -> Rpc<pb::Manifest> {
        let f = self.0.lock().unwrap();
        fail(&f.index_fail)?;
        let r = req.into_inner();
        f.manifests
            .get(&(r.bucket, r.key))
            .cloned()
            .map(Response::new)
            .ok_or_else(|| Status::not_found("object not found: b/k"))
    }
    async fn delete_manifest(&self, req: Request<pb::DeleteManifestRequest>) -> Rpc<pb::Manifest> {
        let mut f = self.0.lock().unwrap();
        fail(&f.index_fail)?;
        let r = req.into_inner();
        f.manifests
            .remove(&(r.bucket, r.key))
            .map(Response::new)
            .ok_or_else(|| Status::not_found("object not found: b/k"))
    }
    async fn list_objects(
        &self,
        _: Request<pb::ListObjectsRequest>,
    ) -> Rpc<pb::ListObjectsResponse> {
        fail(&self.0.lock().unwrap().index_fail)?;
        Ok(Response::new(pb::ListObjectsResponse::default()))
    }
    async fn list_chunk_refs(
        &self,
        _: Request<pb::ListChunkRefsRequest>,
    ) -> Rpc<pb::ListChunkRefsResponse> {
        let f = self.0.lock().unwrap();
        fail(&f.index_fail)?;
        let refs = f
            .manifests
            .values()
            .flat_map(|m| m.chunks.clone())
            .collect();
        Ok(Response::new(pb::ListChunkRefsResponse {
            refs,
            next_page_token: String::new(),
        }))
    }
    async fn node_info(
        &self,
        _: Request<pb::ObjectIndexNodeInfoRequest>,
    ) -> Rpc<pb::ObjectIndexNodeInfoResponse> {
        Ok(Response::new(pb::ObjectIndexNodeInfoResponse::default()))
    }
}

/// One fake chunk node and one fake index, each on its own port, sharing state.
struct Cluster {
    state: Shared,
    index: Arc<RemoteObjectIndex>,
    chunks: Arc<RemoteChunkStore>,
}

async fn listen() -> (tokio_stream::wrappers::TcpListenerStream, String) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap().to_string();
    (tokio_stream::wrappers::TcpListenerStream::new(l), addr)
}

async fn cluster(deadlines: Deadlines) -> Cluster {
    let state: Shared = Default::default();
    state.lock().unwrap().buckets.insert("b".into());
    let (chunk_in, chunk_addr) = listen().await;
    let (index_in, index_addr) = listen().await;
    let s = state.clone();
    tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(ChunkStoreServiceServer::new(ChunkSvc(s)))
            .serve_with_incoming(chunk_in),
    );
    let s = state.clone();
    tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(ObjectIndexServiceServer::new(IndexSvc(s)))
            .serve_with_incoming(index_in),
    );
    Cluster {
        state,
        index: Arc::new(RemoteObjectIndex::connect_lazy(&index_addr, deadlines).unwrap()),
        chunks: Arc::new(
            RemoteChunkStore::connect_lazy(&[("n1".into(), chunk_addr)], deadlines).unwrap(),
        ),
    }
}

fn limits() -> Limits {
    Limits {
        max_object_bytes: 1 << 30,
        max_key_bytes: 64,
        inline_threshold_bytes: 10,
        chunk_bytes: 16,
    }
}

fn body(data: &[u8]) -> impl futures_util::Stream<Item = std::io::Result<Bytes>> + Unpin {
    stream::iter(vec![Ok(Bytes::copy_from_slice(data))])
}

async fn put(c: &Cluster, key: &str, data: &[u8]) -> Result<Manifest> {
    put_object(body(data), "b", key, "x", &limits(), &*c.index, &*c.chunks).await
}

async fn read(c: &Cluster, m: Manifest, start: u64, end: u64) -> Vec<u8> {
    let parts: Vec<Bytes> = object_stream(m, c.chunks.clone(), start, end)
        .try_collect()
        .await
        .unwrap();
    parts.concat()
}

fn id(n: u8) -> String {
    format!("{n:032x}")
}

#[tokio::test]
async fn range_reads_across_chunks() {
    let c = cluster(Deadlines::default()).await;
    let data: Vec<u8> = (0..40).collect(); // chunks of 16, 16, 8
    let m = put(&c, "k", &data).await.unwrap();
    assert_eq!(m.chunks.len(), 3);
    assert_eq!(read(&c, m.clone(), 0, 5).await, &data[0..5], "first chunk");
    assert_eq!(
        read(&c, m.clone(), 18, 25).await,
        &data[18..25],
        "middle chunk"
    );
    assert_eq!(
        read(&c, m.clone(), 35, 40).await,
        &data[35..40],
        "last chunk"
    );
    assert_eq!(
        read(&c, m.clone(), 10, 36).await,
        &data[10..36],
        "spans all three"
    );
    assert_eq!(read(&c, m, 0, 40).await, data, "whole object");
}

#[tokio::test]
async fn chunk_goes_out_in_1_mib_frames_with_a_header() {
    let c = cluster(Deadlines::default()).await;
    let data = Bytes::from(vec![7u8; 2 * 1024 * 1024 + 100]);
    c.chunks.put("n1", &id(1), data).await.unwrap();
    let f = c.state.lock().unwrap();
    assert_eq!(f.put_frames, [1 << 20, 1 << 20, 100]);
    assert_eq!(f.chunks[&id(1)].0.len(), 2 * 1024 * 1024 + 100);
}

#[tokio::test]
async fn overwrite_deletes_the_old_chunks_eagerly() {
    let c = cluster(Deadlines::default()).await;
    let first = put(&c, "k", &[1u8; 40]).await.unwrap();
    let second = put(&c, "k", &[2u8; 20]).await.unwrap();
    assert_eq!(second.version, 2);
    let f = c.state.lock().unwrap();
    assert!(
        first
            .chunks
            .iter()
            .all(|r| !f.chunks.contains_key(&r.chunk_id))
    );
    assert!(
        second
            .chunks
            .iter()
            .all(|r| f.chunks.contains_key(&r.chunk_id))
    );
}

#[tokio::test]
async fn node_down_mid_upload_is_unavailable_and_cleans_up() {
    let c = cluster(Deadlines::default()).await;
    c.state.lock().unwrap().puts_before_down = Some(1);
    let err = put(&c, "k", &[1u8; 40]).await.unwrap_err();
    assert!(matches!(err, Error::Unavailable(_)), "{err:?}");
    let f = c.state.lock().unwrap();
    assert!(
        f.chunks.is_empty(),
        "the chunk written before the failure is deleted"
    );
    assert!(f.manifests.is_empty());
}

#[tokio::test]
async fn index_down_after_chunks_written_cleans_up() {
    let c = cluster(Deadlines::default()).await;
    c.state.lock().unwrap().index_fail = Some((Code::Unavailable, "starting".into()));
    let err = put(&c, "k", &[1u8; 40]).await.unwrap_err();
    assert!(matches!(err, Error::Unavailable(_)), "{err:?}");
    assert!(
        c.state.lock().unwrap().chunks.is_empty(),
        "no chunk is left without a manifest"
    );
}

#[tokio::test]
async fn slow_node_hits_the_put_deadline() {
    let c = cluster(Deadlines {
        put_chunk: Duration::from_millis(100),
        ..Deadlines::default()
    })
    .await;
    c.state.lock().unwrap().put_delay = Duration::from_secs(2);
    let err = c
        .chunks
        .put("n1", &id(1), Bytes::from_static(b"x"))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Timeout(_)), "{err:?}");
}

#[tokio::test]
async fn chunk_status_mapping() {
    let c = cluster(Deadlines::default()).await;
    let table = [
        (Code::Unavailable, "Unavailable"),
        (Code::DeadlineExceeded, "Timeout"),
        (Code::ResourceExhausted, "DiskFull"),
        (Code::DataLoss, "Corrupt"),
        (Code::NotFound, "ChunkNotFound"),
        (Code::Internal, "Internal"),
        (Code::InvalidArgument, "Internal"),
    ];
    for (code, want) in table {
        c.state.lock().unwrap().chunk_fail = Some((code, "boom".into()));
        let err = c.chunks.get("n1", &id(1), None).await.unwrap_err();
        assert!(
            format!("{err:?}").starts_with(want),
            "{code:?} gave {err:?}, want {want}"
        );
    }
}

#[tokio::test]
async fn index_status_mapping() {
    let c = cluster(Deadlines::default()).await;
    let table = [
        (Code::Unavailable, "boom", "Unavailable"),
        (Code::DeadlineExceeded, "boom", "Timeout"),
        (Code::NotFound, "bucket not found: b", "BucketNotFound"),
        (Code::NotFound, "object not found: b/k", "ObjectNotFound"),
        (Code::AlreadyExists, "boom", "BucketExists"),
        (Code::FailedPrecondition, "boom", "BucketNotEmpty"),
        (Code::InvalidArgument, "bad key", "InvalidKey"),
        (Code::Internal, "boom", "Internal"),
    ];
    for (code, msg, want) in table {
        c.state.lock().unwrap().index_fail = Some((code, msg.into()));
        let err = c.index.get_manifest("b", "k").await.unwrap_err();
        assert!(
            format!("{err:?}").starts_with(want),
            "{code:?} gave {err:?}, want {want}"
        );
    }
}

/// Seeds an unreferenced chunk with the given age in seconds.
fn seed_orphan(c: &Cluster, n: u8, age_secs: i64) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    c.state
        .lock()
        .unwrap()
        .chunks
        .insert(id(n), (vec![0; 4], now - age_secs));
}

#[tokio::test]
async fn gc_deletes_only_old_unreferenced_chunks() {
    let c = cluster(Deadlines::default()).await;
    let kept = put(&c, "k", &[1u8; 40]).await.unwrap(); // referenced; created_unix is 0 in the fake, so old
    seed_orphan(&c, 1, 7200); // old orphan
    seed_orphan(&c, 2, 10); // young orphan: might belong to an upload in flight
    let deleted = gc::sweep(&c.index, &c.chunks, Duration::from_secs(3600))
        .await
        .unwrap();
    assert_eq!(deleted, 1);
    let f = c.state.lock().unwrap();
    assert!(!f.chunks.contains_key(&id(1)));
    assert!(f.chunks.contains_key(&id(2)));
    assert!(
        kept.chunks
            .iter()
            .all(|r| f.chunks.contains_key(&r.chunk_id))
    );
}

#[tokio::test]
async fn gc_keeps_a_chunk_whose_manifest_commits_between_the_two_lists() {
    let c = cluster(Deadlines::default()).await;
    seed_orphan(&c, 1, 7200); // old enough to delete, if nothing references it
    // Right after the node lists its chunks, an upload commits a manifest that references the chunk.
    c.state.lock().unwrap().after_list_chunks = Some(Box::new(|f| {
        let m = pb::Manifest {
            bucket: "b".into(),
            key: "late".into(),
            chunks: vec![pb::ChunkRef {
                chunk_id: id(1),
                node_ids: vec!["n1".into()],
                length: 4,
                sha256: String::new(),
            }],
            ..Default::default()
        };
        f.manifests.insert(("b".into(), "late".into()), m);
    }));
    let deleted = gc::sweep(&c.index, &c.chunks, Duration::from_secs(3600))
        .await
        .unwrap();
    assert_eq!(
        deleted, 0,
        "chunks are listed first, refs second, so the late commit is seen"
    );
    assert!(c.state.lock().unwrap().chunks.contains_key(&id(1)));
}

#[tokio::test]
async fn gc_aborts_when_the_index_is_down() {
    let c = cluster(Deadlines::default()).await;
    seed_orphan(&c, 1, 7200);
    c.state.lock().unwrap().index_fail = Some((Code::Unavailable, "down".into()));
    assert!(
        gc::sweep(&c.index, &c.chunks, Duration::from_secs(3600))
            .await
            .is_err()
    );
    assert!(
        c.state.lock().unwrap().chunks.contains_key(&id(1)),
        "no ref list means nothing is deleted"
    );
}

#[test]
fn chunk_ids_must_be_32_lowercase_hex() {
    assert!(valid_chunk_id(&id(1)));
    assert!(!valid_chunk_id("abcd"));
    assert!(!valid_chunk_id(&"A".repeat(32)));
    assert!(!valid_chunk_id(&format!("../{}", "a".repeat(29))));
}
