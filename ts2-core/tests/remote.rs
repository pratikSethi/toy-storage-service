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
    /// The next this many chunk or index RPCs fail with UNAVAILABLE, then it works again (a restart).
    chunk_flaky: u32,
    index_flaky: u32,
    /// Calls that reached the index's PutManifest / GetManifest, failed ones included.
    put_manifest_calls: u32,
    get_manifest_calls: u32,
    /// What NodeInfo answers.
    node_id: String,
    node_unhealthy: bool,
    /// PutManifest stores the manifest, then answers with this code: the response was lost.
    commit_then_fail: Option<Code>,
    put_delay: Duration,
    /// Frame sizes of the last PutChunk.
    put_frames: Vec<usize>,
    /// Runs once, right after ListChunks has answered. Used to commit an upload between the two GC lists.
    after_list_chunks: Option<Hook>,
}

type Shared = Arc<Mutex<Fake>>;
type Hook = Box<dyn FnOnce(&mut Fake) + Send>;

fn flaky(left: &mut u32) -> std::result::Result<(), Status> {
    if *left == 0 {
        return Ok(());
    }
    *left -= 1;
    Err(Status::unavailable("connection reset"))
}

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
            flaky(&mut f.chunk_flaky)?;
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
        let mut f = self.0.lock().unwrap();
        fail(&f.chunk_fail)?;
        flaky(&mut f.chunk_flaky)?;
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
        flaky(&mut f.chunk_flaky)?;
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
        let f = self.0.lock().unwrap();
        Ok(Response::new(pb::ChunkStoreNodeInfoResponse {
            node_id: f.node_id.clone(),
            healthy: !f.node_unhealthy,
            ..Default::default()
        }))
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
        f.put_manifest_calls += 1;
        fail(&f.index_fail)?;
        flaky(&mut f.index_flaky)?;
        let mut m = req.into_inner().manifest.unwrap();
        let k = (m.bucket.clone(), m.key.clone());
        let previous = f.manifests.get(&k).cloned();
        m.version = previous.as_ref().map_or(1, |p| p.version + 1);
        f.manifests.insert(k, m.clone());
        if let Some(code) = f.commit_then_fail {
            return Err(Status::new(code, "response lost"));
        }
        Ok(Response::new(pb::PutManifestResponse {
            manifest: Some(m),
            previous,
        }))
    }
    async fn get_manifest(&self, req: Request<pb::GetManifestRequest>) -> Rpc<pb::Manifest> {
        let mut f = self.0.lock().unwrap();
        f.get_manifest_calls += 1;
        fail(&f.index_fail)?;
        flaky(&mut f.index_flaky)?;
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
    state.lock().unwrap().node_id = "n1".into();
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
async fn overwrite_keeps_the_old_chunks_until_gc() {
    let c = cluster(Deadlines::default()).await;
    let first = put(&c, "k", &[1u8; 40]).await.unwrap();
    let second = put(&c, "k", &[2u8; 20]).await.unwrap();
    assert_eq!(second.version, 2);
    // A GET that started on the first version can still finish.
    assert_eq!(read(&c, first.clone(), 0, 40).await, [1u8; 40]);
    let deleted = gc::sweep(&c.index, &c.chunks, Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(
        deleted,
        first.chunks.len(),
        "GC removes exactly the old chunks"
    );
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
async fn delete_keeps_the_chunks_until_gc() {
    let c = cluster(Deadlines::default()).await;
    let m = put(&c, "k", &[1u8; 40]).await.unwrap();
    delete_object(&*c.index, &*c.chunks, "b", "k")
        .await
        .unwrap();
    assert_eq!(
        read(&c, m.clone(), 0, 40).await,
        [1u8; 40],
        "an open GET finishes"
    );
    let deleted = gc::sweep(&c.index, &c.chunks, Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(deleted, m.chunks.len());
    assert!(c.state.lock().unwrap().chunks.is_empty());
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
async fn definite_index_rejection_cleans_up_the_chunks() {
    let c = cluster(Deadlines::default()).await;
    c.state.lock().unwrap().index_fail = Some((Code::NotFound, "bucket not found: b".into()));
    let err = put(&c, "k", &[1u8; 40]).await.unwrap_err();
    assert!(matches!(err, Error::BucketNotFound), "{err:?}");
    assert!(
        c.state.lock().unwrap().chunks.is_empty(),
        "the index said no, so nothing references the chunks"
    );
}

#[tokio::test]
async fn index_commits_then_answers_unavailable_keeps_the_chunks() {
    let c = cluster(Deadlines::default()).await;
    c.state.lock().unwrap().commit_then_fail = Some(Code::Unavailable);
    let err = put(&c, "k", &[1u8; 40]).await.unwrap_err();
    assert!(matches!(err, Error::Unavailable(_)), "{err:?}");
    let f = c.state.lock().unwrap();
    let m = &f.manifests[&("b".into(), "k".into())];
    assert_eq!(m.chunks.len(), 3);
    assert!(
        m.chunks.iter().all(|r| f.chunks.contains_key(&r.chunk_id)),
        "the committed manifest still has all its chunks"
    );
}

#[tokio::test]
async fn index_unavailable_before_commit_leaves_chunks_for_gc() {
    // From the client the two cases look the same, so the chunks stay and GC decides after min_age.
    let c = cluster(Deadlines::default()).await;
    c.state.lock().unwrap().index_fail = Some((Code::Unavailable, "starting".into()));
    let err = put(&c, "k", &[1u8; 40]).await.unwrap_err();
    assert!(matches!(err, Error::Unavailable(_)), "{err:?}");
    let deleted = gc::sweep(&c.index, &c.chunks, Duration::from_secs(1))
        .await
        .err();
    assert!(deleted.is_some(), "index is down, so the sweep aborts");
    c.state.lock().unwrap().index_fail = None;
    let deleted = gc::sweep(&c.index, &c.chunks, Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(deleted, 3, "the orphans go once the index answers");
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
async fn gc_adds_a_clock_skew_margin_to_min_age() {
    let c = cluster(Deadlines::default()).await;
    seed_orphan(&c, 1, 3600 + 30); // older than min_age, inside the margin
    seed_orphan(&c, 2, 3600 + 120); // past both
    let deleted = gc::sweep(&c.index, &c.chunks, Duration::from_secs(3600))
        .await
        .unwrap();
    assert_eq!(deleted, 1);
    let f = c.state.lock().unwrap();
    assert!(f.chunks.contains_key(&id(1)));
    assert!(!f.chunks.contains_key(&id(2)));
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

/// A fake chunk node of its own that reports `id`, and its address.
async fn node(id: &str) -> (Shared, String) {
    let state: Shared = Default::default();
    state.lock().unwrap().node_id = id.into();
    let (incoming, addr) = listen().await;
    tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(ChunkStoreServiceServer::new(ChunkSvc(state.clone())))
            .serve_with_incoming(incoming),
    );
    (state, addr)
}

async fn two_nodes() -> (Shared, Shared, RemoteChunkStore) {
    let (s1, a1) = node("n1").await;
    let (s2, a2) = node("n2").await;
    let store = RemoteChunkStore::connect_lazy(
        &[("n1".into(), a1), ("n2".into(), a2)],
        Deadlines::default(),
    )
    .unwrap();
    (s1, s2, store)
}

#[tokio::test]
async fn startup_accepts_nodes_that_report_their_configured_id() {
    let (_, _, store) = two_nodes().await;
    store.verify_node_ids(Duration::ZERO).await.unwrap();
}

#[tokio::test]
async fn startup_refuses_a_node_that_reports_another_id() {
    let (s1, _, store) = two_nodes().await;
    s1.lock().unwrap().node_id = "n2".into(); // n1's address now points at node 2's host
    let err = store.verify_node_ids(Duration::ZERO).await.unwrap_err();
    assert!(err.to_string().contains(r#"reports id "n2""#), "{err}");
}

#[tokio::test]
async fn startup_tolerates_a_node_that_is_not_up_yet() {
    let store = RemoteChunkStore::connect_lazy(
        &[("n1".into(), "127.0.0.1:1".into())],
        Deadlines::default(),
    )
    .unwrap();
    store.verify_node_ids(Duration::ZERO).await.unwrap();
}

#[tokio::test]
async fn placement_skips_an_unhealthy_node_and_returns_when_it_recovers() {
    let (s1, _, store) = two_nodes().await;
    assert_eq!(store.nodes(), ["n1", "n2"]);
    s1.lock().unwrap().node_unhealthy = true;
    store.refresh_health().await;
    assert_eq!(store.nodes(), ["n2"]);
    s1.lock().unwrap().node_unhealthy = false;
    store.refresh_health().await;
    assert_eq!(store.nodes(), ["n1", "n2"]);
}

#[tokio::test]
async fn placement_skips_a_node_that_swapped_ids_after_startup() {
    let (s1, _, store) = two_nodes().await;
    s1.lock().unwrap().node_id = "n9".into();
    store.refresh_health().await;
    assert_eq!(store.nodes(), ["n2"]);
}

#[tokio::test]
async fn placement_skips_a_node_that_just_ran_out_of_disk() {
    let (s1, _, store) = two_nodes().await;
    s1.lock().unwrap().chunk_fail = Some((Code::ResourceExhausted, "full".into()));
    let err = store
        .put("n1", &id(1), Bytes::from_static(b"x"))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::DiskFull), "{err:?}");
    assert_eq!(store.nodes(), ["n2"]);
    assert_eq!(
        store.configured_nodes(),
        ["n1", "n2"],
        "GC still lists both"
    );
}

#[tokio::test]
async fn placement_falls_back_to_all_nodes_when_none_looks_healthy() {
    let (s1, s2, store) = two_nodes().await;
    s1.lock().unwrap().node_unhealthy = true;
    s2.lock().unwrap().node_unhealthy = true;
    store.refresh_health().await;
    assert_eq!(
        store.nodes(),
        ["n1", "n2"],
        "a stale view must not refuse every write"
    );
}

#[tokio::test]
async fn put_object_writes_every_chunk_to_the_healthy_node() {
    let c = cluster(Deadlines::default()).await; // only its index is used
    let (s1, s2, store) = two_nodes().await;
    s1.lock().unwrap().node_unhealthy = true;
    store.refresh_health().await;
    let data = vec![5u8; 16 * 20]; // 20 chunks: with two live nodes about half would hit n1
    let m = put_object(body(&data), "b", "k", "x", &limits(), &*c.index, &store)
        .await
        .unwrap();
    assert_eq!(m.chunks.len(), 20);
    assert!(m.chunks.iter().all(|r| r.node_id == "n2"));
    assert!(s1.lock().unwrap().chunks.is_empty());
    assert_eq!(s2.lock().unwrap().chunks.len(), 20);
}

#[tokio::test]
async fn chunk_calls_retry_once_when_unavailable() {
    let c = cluster(Deadlines::default()).await;
    c.chunks
        .put("n1", &id(1), Bytes::from_static(b"abc"))
        .await
        .unwrap();
    c.state.lock().unwrap().chunk_flaky = 1; // the service restarted: the first call hits a dead connection
    assert_eq!(
        c.chunks.get("n1", &id(1), None).await.unwrap(),
        "abc",
        "get"
    );
    c.state.lock().unwrap().chunk_flaky = 1;
    c.chunks
        .put("n1", &id(2), Bytes::from_static(b"def"))
        .await
        .unwrap();
    c.state.lock().unwrap().chunk_flaky = 1;
    c.chunks.delete("n1", &id(2)).await.unwrap();
    assert!(
        !c.state.lock().unwrap().chunks.contains_key(&id(2)),
        "put and delete"
    );
    c.state.lock().unwrap().chunk_flaky = 1;
    assert_eq!(c.chunks.list_chunks("n1").await.unwrap().len(), 1, "list");
}

#[tokio::test]
async fn chunk_calls_give_up_after_one_retry() {
    let c = cluster(Deadlines::default()).await;
    c.state.lock().unwrap().chunk_flaky = 2;
    let err = c.chunks.get("n1", &id(1), None).await.unwrap_err();
    assert!(matches!(err, Error::Unavailable(_)), "{err:?}");
}

#[tokio::test]
async fn index_reads_retry_once_but_put_manifest_does_not() {
    let c = cluster(Deadlines::default()).await;
    let m = put(&c, "k", &[1u8; 5]).await.unwrap();
    c.state.lock().unwrap().index_flaky = 1;
    assert_eq!(c.index.get_manifest("b", "k").await.unwrap().etag, m.etag);
    assert_eq!(c.state.lock().unwrap().get_manifest_calls, 2);

    let before = c.state.lock().unwrap().put_manifest_calls;
    c.state.lock().unwrap().index_flaky = 1;
    let err = put(&c, "k2", &[1u8; 5]).await.unwrap_err();
    assert!(matches!(err, Error::Unavailable(_)), "{err:?}");
    assert_eq!(
        c.state.lock().unwrap().put_manifest_calls,
        before + 1,
        "one attempt: PutManifest has no idempotency key"
    );
}
