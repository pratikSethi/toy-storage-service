use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize)]
pub struct Limits {
    pub max_object_bytes: u64,
    pub max_key_bytes: usize,
    pub inline_threshold_bytes: usize,
    pub chunk_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChunkRef {
    pub chunk_id: String,
    pub node_id: String,
    pub length: u64,
    /// Hex sha256 of the chunk bytes.
    pub checksum: String,
}

/// One per object key. Either `inline_data` or `chunks` is set, never both.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub bucket: String,
    pub key: String,
    pub version: u64,
    pub size: u64,
    /// Hex sha256 of the whole object.
    pub etag: String,
    pub content_type: String,
    pub created_at: DateTime<Utc>,
    pub metadata: BTreeMap<String, String>,
    #[serde(default, with = "inline_b64", skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<Vec<u8>>,
    pub chunk_bytes: u64,
    #[serde(default)]
    pub chunks: Vec<ChunkRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bucket {
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectSummary {
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub last_modified: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct ListPage {
    pub objects: Vec<ObjectSummary>,
    pub is_truncated: bool,
    pub next_start_after: Option<String>,
}

mod inline_b64 {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(b) => s.serialize_str(&STANDARD.encode(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        Option::<String>::deserialize(d)?
            .map(|s| STANDARD.decode(s).map_err(serde::de::Error::custom))
            .transpose()
    }
}
