//! TS2 core: manifests, the ObjectIndex and ChunkStore traits, and the object put/get logic.

mod chunk_store;
mod error;
mod model;
mod object;
mod object_index;

pub use chunk_store::LocalChunkStore;
pub use error::{Error, Result};
pub use model::{Bucket, ChunkRef, Limits, ListPage, Manifest, ObjectSummary};
pub use object::{delete_object, object_stream, put_object, validate_bucket_name, validate_key};
pub use object_index::SqliteObjectIndex;

use bytes::Bytes;
use std::ops::Range;

/// Key to manifest mapping. SQLite in v0.1, a C++ service later.
#[async_trait::async_trait]
pub trait ObjectIndex: Send + Sync {
    async fn create_bucket(&self, name: &str) -> Result<Bucket>;
    async fn get_bucket(&self, name: &str) -> Result<Bucket>;
    /// `BucketNotEmpty` when the bucket still has objects.
    async fn delete_bucket(&self, name: &str) -> Result<()>;
    async fn list_buckets(&self) -> Result<Vec<Bucket>>;
    /// Commits the manifest and returns it with `version` assigned (previous version + 1).
    async fn put_manifest(&self, manifest: Manifest) -> Result<Manifest>;
    async fn get_manifest(&self, bucket: &str, key: &str) -> Result<Manifest>;
    /// Returns the removed manifest so the caller can clean up its chunks.
    async fn delete_manifest(&self, bucket: &str, key: &str) -> Result<Manifest>;
    async fn list(&self, bucket: &str, prefix: &str, start_after: &str, max_keys: usize) -> Result<ListPage>;
}

/// Stores chunk bytes. Local disk in v0.1, a C++ service later.
#[async_trait::async_trait]
pub trait ChunkStore: Send + Sync {
    /// Durable when it returns.
    async fn put(&self, chunk_id: &str, data: Bytes) -> Result<()>;
    /// Whole chunk, or the byte range within it.
    async fn get(&self, chunk_id: &str, range: Option<Range<u64>>) -> Result<Bytes>;
    async fn delete(&self, chunk_id: &str) -> Result<()>;
}
