//! gRPC clients that implement `ObjectIndex` and `ChunkStore` against the C++ services.
//! Callers cannot tell them from the in-process versions; only errors differ (see `map_status`).

use crate::{
    Bucket, ChunkRef, ChunkStore, Error, ListPage, Manifest, ObjectIndex, ObjectSummary, Result,
    valid_chunk_id,
};
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use chrono::{DateTime, TimeZone, Utc};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::future::Future;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tonic::transport::{Channel, Endpoint};
use tonic::{Code, Status};
use ts2_proto::v1 as pb;
use ts2_proto::v1::chunk_store_service_client::ChunkStoreServiceClient;
use ts2_proto::v1::object_index_service_client::ObjectIndexServiceClient;

/// The proto allows at most 1 MiB of chunk data per message.
const FRAME_BYTES: usize = 1024 * 1024;

/// Deadlines from the proto comments: 30 s for PutChunk, 10 s for GetChunk, 5 s for everything else.
#[derive(Clone, Copy, Debug)]
pub struct Deadlines {
    pub put_chunk: Duration,
    pub get_chunk: Duration,
    pub other: Duration,
}

impl Default for Deadlines {
    fn default() -> Self {
        Self {
            put_chunk: Duration::from_secs(30),
            get_chunk: Duration::from_secs(10),
            other: Duration::from_secs(5),
        }
    }
}

/// Runs one RPC under a client-side deadline. Elapsed time becomes `Error::Timeout`.
async fn with_deadline<T>(
    limit: Duration,
    rpc: impl Future<Output = std::result::Result<T, Status>>,
) -> Result<std::result::Result<T, Status>> {
    tokio::time::timeout(limit, rpc)
        .await
        .map_err(|_| Error::Timeout(format!("no answer within {limit:?}")))
}

/// Runs `call`, and once more if the first try came back `Unavailable` (a stale connection after a restart,
/// or a service that is just coming up). Only for calls that are safe to repeat; never PutManifest, which
/// has no idempotency key.
async fn retry_once_if_unavailable<T, F, Fut>(mut call: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    match call().await {
        Err(Error::Unavailable(_)) => {
            tokio::time::sleep(Duration::from_millis(100)).await;
            call().await
        }
        r => r,
    }
}

/// Which service answered, because `NOT_FOUND` means different things to each.
enum Source {
    Index,
    Chunk,
}

/// gRPC status to `Error`. Frontdoor turns these into 503, 504, 507 and 404 instead of 500.
fn map_status(s: Status, from: Source, chunk_id: &str) -> Error {
    let msg = s.message().to_owned();
    match (s.code(), from) {
        (Code::Unavailable, _) => Error::Unavailable(msg),
        (Code::DeadlineExceeded, _) => Error::Timeout(msg),
        (Code::ResourceExhausted, _) => Error::DiskFull,
        (Code::DataLoss, _) => Error::Corrupt(chunk_id.to_owned()),
        (Code::NotFound, Source::Chunk) => Error::ChunkNotFound(chunk_id.to_owned()),
        // The index says which one is missing in the message: "bucket not found: .." or "object not found: ..".
        (Code::NotFound, Source::Index) if msg.starts_with("object") => Error::ObjectNotFound,
        (Code::NotFound, Source::Index) => Error::BucketNotFound,
        (Code::AlreadyExists, Source::Index) => Error::BucketExists,
        (Code::FailedPrecondition, Source::Index) => Error::BucketNotEmpty,
        (Code::InvalidArgument, Source::Index) => Error::InvalidKey(msg),
        (code, _) => Error::Internal(format!("{code}: {msg}")),
    }
}

fn channel(addr: &str) -> Result<Channel> {
    let url = if addr.contains("://") {
        addr.to_owned()
    } else {
        format!("http://{addr}")
    };
    // Lazy: frontdoor starts even when a service is still booting; the first call connects.
    // Keepalives notice a peer that restarted or vanished, so a dead connection is replaced instead of
    // failing the next request.
    Ok(Endpoint::from_shared(url)
        .map_err(|e| Error::Internal(format!("bad address {addr:?}: {e}")))?
        .connect_timeout(Duration::from_secs(3))
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .http2_keep_alive_interval(Duration::from_secs(10))
        .keep_alive_timeout(Duration::from_secs(5))
        .keep_alive_while_idle(true)
        .connect_lazy())
}

fn to_ts(t: DateTime<Utc>) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: t.timestamp(),
        nanos: t.timestamp_subsec_nanos() as i32,
    }
}

fn from_ts(t: Option<prost_types::Timestamp>) -> DateTime<Utc> {
    let t = t.unwrap_or_default();
    Utc.timestamp_opt(t.seconds, t.nanos.max(0) as u32)
        .single()
        .unwrap_or_default()
}

fn manifest_to_pb(m: Manifest) -> Result<pb::Manifest> {
    Ok(pb::Manifest {
        bucket: m.bucket,
        key: m.key,
        version: m.version,
        size: m.size,
        etag: m.etag,
        content_type: m.content_type,
        created_at: Some(to_ts(m.created_at)),
        metadata: m.metadata.into_iter().collect(),
        inline_data: m.inline_data,
        chunk_bytes: m.chunk_bytes,
        chunks: m
            .chunks
            .into_iter()
            .map(|c| pb::ChunkRef {
                chunk_id: c.chunk_id,
                node_ids: vec![c.node_id],
                length: c.length,
                sha256: c.checksum,
            })
            .collect(),
    })
}

fn manifest_from_pb(m: pb::Manifest) -> Result<Manifest> {
    let chunks =
        m.chunks
            .into_iter()
            .map(|c| {
                // v0.1 has one replica per chunk; replicas arrive in v0.4.
                let node_id =
                    c.node_ids.into_iter().next().ok_or_else(|| {
                        Error::Internal(format!("chunk {} has no node", c.chunk_id))
                    })?;
                Ok(ChunkRef {
                    chunk_id: c.chunk_id,
                    node_id,
                    length: c.length,
                    checksum: c.sha256,
                })
            })
            .collect::<Result<_>>()?;
    Ok(Manifest {
        bucket: m.bucket,
        key: m.key,
        version: m.version,
        size: m.size,
        etag: m.etag,
        content_type: m.content_type,
        created_at: from_ts(m.created_at),
        metadata: m.metadata.into_iter().collect::<BTreeMap<_, _>>(),
        inline_data: m.inline_data,
        chunk_bytes: m.chunk_bytes,
        chunks,
    })
}

fn bucket_from_pb(b: pb::Bucket) -> Bucket {
    Bucket {
        name: b.name,
        created_at: from_ts(b.created_at),
    }
}

/// `ObjectIndex` over gRPC to ObjectIndexService.
pub struct RemoteObjectIndex {
    client: ObjectIndexServiceClient<Channel>,
    deadlines: Deadlines,
}

impl RemoteObjectIndex {
    pub fn connect_lazy(addr: &str, deadlines: Deadlines) -> Result<Self> {
        Ok(Self {
            client: ObjectIndexServiceClient::new(channel(addr)?),
            deadlines,
        })
    }

    /// One attempt on a fresh clone of the client (clones share the connection) under the 5 s deadline.
    async fn rpc_once<T, F, Fut>(&self, call: F) -> Result<T>
    where
        F: FnOnce(ObjectIndexServiceClient<Channel>) -> Fut,
        Fut: Future<Output = std::result::Result<tonic::Response<T>, Status>>,
    {
        let r = with_deadline(self.deadlines.other, call(self.client.clone())).await?;
        r.map(tonic::Response::into_inner)
            .map_err(|s| map_status(s, Source::Index, ""))
    }

    /// `rpc_once`, repeated once on `Unavailable`. For reads and other calls that are safe to repeat.
    async fn rpc<T, F, Fut>(&self, call: F) -> Result<T>
    where
        F: Fn(ObjectIndexServiceClient<Channel>) -> Fut,
        Fut: Future<Output = std::result::Result<tonic::Response<T>, Status>>,
    {
        retry_once_if_unavailable(|| self.rpc_once(&call)).await
    }

    /// Every chunk id some manifest points at, for orphan GC. Pages until the index says it is done.
    pub async fn list_chunk_refs(&self) -> Result<std::collections::HashSet<String>> {
        let (mut refs, mut token) = (std::collections::HashSet::new(), String::new());
        loop {
            let req = pb::ListChunkRefsRequest {
                page_size: 0,
                page_token: token.clone(),
            };
            let page = self
                .rpc(|mut c| {
                    let req = req.clone();
                    async move { c.list_chunk_refs(req).await }
                })
                .await?;
            refs.extend(page.refs.into_iter().map(|r| r.chunk_id));
            if page.next_page_token.is_empty() {
                return Ok(refs);
            }
            token = page.next_page_token;
        }
    }
}

#[async_trait]
impl ObjectIndex for RemoteObjectIndex {
    async fn create_bucket(&self, name: &str) -> Result<Bucket> {
        let name = name.to_owned();
        Ok(bucket_from_pb(
            // Not repeated: if the first try committed, the second would answer BucketExists.
            self.rpc_once(|mut c| async move {
                c.create_bucket(pb::CreateBucketRequest { name }).await
            })
            .await?,
        ))
    }

    async fn get_bucket(&self, name: &str) -> Result<Bucket> {
        let name = name.to_owned();
        Ok(bucket_from_pb(
            self.rpc(|mut c| {
                let req = pb::GetBucketRequest { name: name.clone() };
                async move { c.get_bucket(req).await }
            })
            .await?,
        ))
    }

    async fn delete_bucket(&self, name: &str) -> Result<()> {
        let name = name.to_owned();
        self.rpc_once(
            |mut c| async move { c.delete_bucket(pb::DeleteBucketRequest { name }).await },
        )
        .await?;
        Ok(())
    }

    async fn list_buckets(&self) -> Result<Vec<Bucket>> {
        let r = self
            .rpc(|mut c| async move { c.list_buckets(pb::ListBucketsRequest {}).await })
            .await?;
        Ok(r.buckets.into_iter().map(bucket_from_pb).collect())
    }

    async fn put_manifest(&self, manifest: Manifest) -> Result<(Manifest, Option<Manifest>)> {
        let req = pb::PutManifestRequest {
            manifest: Some(manifest_to_pb(manifest)?),
        };
        // Not repeated: there is no idempotency key, and a lost response may hide a commit.
        let r = self
            .rpc_once(|mut c| async move { c.put_manifest(req).await })
            .await?;
        let stored = r
            .manifest
            .ok_or_else(|| Error::Internal("PutManifest returned no manifest".into()))?;
        Ok((
            manifest_from_pb(stored)?,
            r.previous.map(manifest_from_pb).transpose()?,
        ))
    }

    async fn get_manifest(&self, bucket: &str, key: &str) -> Result<Manifest> {
        let req = pb::GetManifestRequest {
            bucket: bucket.to_owned(),
            key: key.to_owned(),
        };
        manifest_from_pb(
            self.rpc(|mut c| {
                let req = req.clone();
                async move { c.get_manifest(req).await }
            })
            .await?,
        )
    }

    async fn delete_manifest(&self, bucket: &str, key: &str) -> Result<Manifest> {
        let req = pb::DeleteManifestRequest {
            bucket: bucket.to_owned(),
            key: key.to_owned(),
        };
        manifest_from_pb(
            // Not repeated: if the first try committed, the second would lose the removed chunk list.
            self.rpc_once(|mut c| async move { c.delete_manifest(req).await })
                .await?,
        )
    }

    async fn list(
        &self,
        bucket: &str,
        prefix: &str,
        start_after: &str,
        max_keys: usize,
    ) -> Result<ListPage> {
        let req = pb::ListObjectsRequest {
            bucket: bucket.to_owned(),
            prefix: prefix.to_owned(),
            start_after: start_after.to_owned(),
            max_keys: max_keys as u32,
        };
        let r = self
            .rpc(|mut c| {
                let req = req.clone();
                async move { c.list_objects(req).await }
            })
            .await?;
        Ok(ListPage {
            objects: r
                .objects
                .into_iter()
                .map(|o| ObjectSummary {
                    key: o.key,
                    size: o.size,
                    etag: o.etag,
                    last_modified: from_ts(o.last_modified),
                })
                .collect(),
            is_truncated: r.is_truncated,
            next_start_after: (!r.next_start_after.is_empty()).then_some(r.next_start_after),
        })
    }
}

/// One chunk as a node lists it.
pub struct ListedChunk {
    pub chunk_id: String,
    pub created_unix: i64,
}

/// How long a node that answered RESOURCE_EXHAUSTED is left out of placement.
const EXHAUSTED_BACKOFF: Duration = Duration::from_secs(60);

/// What frontdoor believes about one node. Starts healthy so the first requests work before any refresh.
struct NodeState {
    healthy: bool,
    exhausted_until: Option<Instant>,
}

struct Node {
    id: String,
    addr: String,
    client: ChunkStoreServiceClient<Channel>,
    state: Mutex<NodeState>,
}

impl Node {
    fn placeable(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.healthy && s.exhausted_until.is_none_or(|t| Instant::now() >= t)
    }
}

/// `ChunkStore` over gRPC to one ChunkStoreService per node. `node` arguments are the ids in `connect_lazy`.
pub struct RemoteChunkStore {
    /// In config order, which is the order `configured_nodes()` reports.
    nodes: Vec<Node>,
    deadlines: Deadlines,
}

impl RemoteChunkStore {
    /// `nodes` is (node id, address) pairs. The ids end up in manifests, so renaming one orphans its chunks.
    pub fn connect_lazy(nodes: &[(String, String)], deadlines: Deadlines) -> Result<Self> {
        if nodes.is_empty() {
            return Err(Error::Internal(
                "at least one chunk node is required".into(),
            ));
        }
        let nodes = nodes
            .iter()
            .map(|(id, addr)| {
                Ok(Node {
                    id: id.clone(),
                    addr: addr.clone(),
                    client: ChunkStoreServiceClient::new(channel(addr)?),
                    state: Mutex::new(NodeState {
                        healthy: true,
                        exhausted_until: None,
                    }),
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self { nodes, deadlines })
    }

    fn node(&self, id: &str) -> Result<&Node> {
        self.nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(|| Error::Internal(format!("unknown chunk node {id:?}")))
    }

    fn client(&self, node: &str) -> Result<ChunkStoreServiceClient<Channel>> {
        Ok(self.node(node)?.client.clone())
    }

    /// Every configured node, healthy or not. GC lists all of them; `nodes()` is only where new chunks may go.
    pub fn configured_nodes(&self) -> Vec<String> {
        self.nodes.iter().map(|n| n.id.clone()).collect()
    }

    /// Asks one node who it is. An `Err` means it could not be asked; `Ok(false)` means it answered as a
    /// different node or reported itself unhealthy.
    async fn probe(&self, n: &Node) -> Result<bool> {
        let mut c = n.client.clone();
        let info = with_deadline(
            self.deadlines.other,
            c.node_info(pb::ChunkStoreNodeInfoRequest {}),
        )
        .await?
        .map_err(|s| map_status(s, Source::Chunk, ""))?
        .into_inner();
        if info.node_id != n.id {
            return Err(Error::Internal(format!(
                "chunk node at {} reports id {:?}, but the config says {:?}",
                n.addr, info.node_id, n.id
            )));
        }
        Ok(info.healthy)
    }

    /// Startup check: every node must report the id it has in the config, because the id is written into
    /// every manifest. A node that cannot be reached yet is retried for `wait`, then left to the health
    /// refresh (it is also checked there). A wrong id fails at once.
    pub async fn verify_node_ids(&self, wait: Duration) -> Result<()> {
        let give_up = Instant::now() + wait;
        for n in &self.nodes {
            loop {
                match self.probe(n).await {
                    Ok(_) => break,
                    Err(e @ Error::Internal(_)) => return Err(e),
                    Err(e) if Instant::now() >= give_up => {
                        tracing::warn!(node = %n.id, error = %e, "chunk node not reachable at startup; id not verified");
                        break;
                    }
                    Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
                }
            }
        }
        Ok(())
    }

    /// One health pass over all nodes. An unreachable node, one that says it is unhealthy and one that
    /// answers with the wrong id are all left out of placement until a later pass says otherwise.
    pub async fn refresh_health(&self) {
        for n in &self.nodes {
            let healthy = match self.probe(n).await {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!(node = %n.id, error = %e, "chunk node health check failed");
                    false
                }
            };
            n.state.lock().unwrap().healthy = healthy;
        }
    }

    /// Runs `refresh_health` every `interval` until the process exits.
    pub fn spawn_health_refresh(self: &Arc<Self>, interval: Duration) {
        let store = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                store.refresh_health().await;
            }
        });
    }

    /// Every chunk on one node, oldest-first order is not promised. Held in memory: fine for a toy.
    pub async fn list_chunks(&self, node: &str) -> Result<Vec<ListedChunk>> {
        let (mut all, mut token) = (Vec::new(), String::new());
        loop {
            let req = pb::ListChunksRequest {
                page_size: 0,
                page_token: std::mem::take(&mut token),
            };
            let page = retry_once_if_unavailable(|| async {
                let mut c = self.client(node)?;
                with_deadline(self.deadlines.other, c.list_chunks(req.clone()))
                    .await?
                    .map_err(|s| map_status(s, Source::Chunk, ""))
            })
            .await?
            .into_inner();
            all.extend(page.chunks.into_iter().map(|i| ListedChunk {
                chunk_id: i.chunk_id,
                created_unix: i.created_unix,
            }));
            if page.next_page_token.is_empty() {
                return Ok(all);
            }
            token = page.next_page_token;
        }
    }
}

/// Chunk ids become file names on the node, so refuse a bad one here instead of burning a round trip.
fn check_id(id: &str) -> Result<()> {
    valid_chunk_id(id)
        .then_some(())
        .ok_or_else(|| Error::Internal(format!("bad chunk id {id:?}")))
}

#[async_trait]
impl ChunkStore for RemoteChunkStore {
    /// Healthy nodes that did not just run out of disk. If none qualifies, all of them: the health view may
    /// be stale, and trying beats refusing every write.
    fn nodes(&self) -> Vec<String> {
        let ok: Vec<_> = self.nodes.iter().filter(|n| n.placeable()).collect();
        let pick = if ok.is_empty() {
            self.nodes.iter().collect()
        } else {
            ok
        };
        pick.into_iter().map(|n| n.id.clone()).collect()
    }

    /// Orphan GC removes old chunks after `min_age`, so a GET in flight keeps its chunks.
    fn deletes_eagerly(&self) -> bool {
        false
    }

    async fn put(&self, node: &str, chunk_id: &str, data: Bytes) -> Result<()> {
        check_id(chunk_id)?;
        // The node verifies length and sha256 against this header, so hash before sending.
        let header = pb::PutChunkHeader {
            chunk_id: chunk_id.to_owned(),
            expected_length: data.len() as u64,
            expected_sha256: hex::encode(Sha256::digest(&data)),
            write_token: String::new(),
        };
        // Repeating is safe: the same id with the same bytes is the same chunk.
        let result = retry_once_if_unavailable(|| async {
            let mut client = self.client(node)?;
            let frames: Vec<_> =
                std::iter::once(pb::put_chunk_request::Payload::Header(header.clone()))
                    .chain(
                        data.chunks(FRAME_BYTES)
                            .map(|f| pb::put_chunk_request::Payload::Data(f.to_vec())),
                    )
                    .map(|payload| pb::PutChunkRequest {
                        payload: Some(payload),
                    })
                    .collect();
            let call = client.put_chunk(futures_util::stream::iter(frames));
            with_deadline(self.deadlines.put_chunk, call)
                .await?
                .map_err(|s| map_status(s, Source::Chunk, chunk_id))
        })
        .await;
        if matches!(result, Err(Error::DiskFull)) {
            self.node(node)?.state.lock().unwrap().exhausted_until =
                Some(Instant::now() + EXHAUSTED_BACKOFF);
        }
        result.map(|_| ())
    }

    async fn get(&self, node: &str, chunk_id: &str, range: Option<Range<u64>>) -> Result<Bytes> {
        check_id(chunk_id)?;
        let (range_start, range_end) = range.map_or((0, 0), |r| (r.start, r.end));
        let req = pb::GetChunkRequest {
            chunk_id: chunk_id.to_owned(),
            range_start,
            range_end,
        };
        retry_once_if_unavailable(|| async {
            let mut client = self.client(node)?;
            let fetch = async {
                let mut stream = client.get_chunk(req.clone()).await?.into_inner();
                let mut buf = BytesMut::new();
                while let Some(frame) = stream.message().await? {
                    buf.extend_from_slice(&frame.data);
                }
                Ok::<_, Status>(buf.freeze())
            };
            with_deadline(self.deadlines.get_chunk, fetch)
                .await?
                .map_err(|s| map_status(s, Source::Chunk, chunk_id))
        })
        .await
    }

    async fn delete(&self, node: &str, chunk_id: &str) -> Result<()> {
        check_id(chunk_id)?;
        let req = pb::DeleteChunkRequest {
            chunk_id: chunk_id.to_owned(),
        };
        retry_once_if_unavailable(|| async {
            let mut client = self.client(node)?;
            with_deadline(self.deadlines.other, client.delete_chunk(req.clone()))
                .await?
                .map_err(|s| map_status(s, Source::Chunk, chunk_id))
        })
        .await?;
        Ok(())
    }
}
