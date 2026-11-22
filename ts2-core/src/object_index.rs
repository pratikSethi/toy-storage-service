use crate::{Bucket, Error, ListPage, Manifest, ObjectIndex, ObjectSummary, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::{Arc, Mutex};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS buckets (
    name TEXT PRIMARY KEY,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS manifests (
    bucket TEXT NOT NULL REFERENCES buckets(name),
    key TEXT NOT NULL,
    version INTEGER NOT NULL,
    size INTEGER NOT NULL,
    etag TEXT NOT NULL,
    created_at TEXT NOT NULL,
    manifest TEXT NOT NULL,
    PRIMARY KEY (bucket, key)
) WITHOUT ROWID;";

/// SQLite-backed ObjectIndex. Calls run on the blocking pool, one connection behind a mutex.
pub struct SqliteObjectIndex {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteObjectIndex {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || f(&mut conn.lock().unwrap_or_else(|e| e.into_inner())))
            .await
            .map_err(|e| Error::Internal(e.to_string()))?
    }
}

fn bucket_exists(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM buckets WHERE name = ?1", [name], |_| Ok(()))
        .optional()?
        .is_some())
}

fn parse_manifest(json: String) -> Result<Manifest> {
    serde_json::from_str(&json).map_err(|e| Error::Internal(e.to_string()))
}

#[async_trait::async_trait]
impl ObjectIndex for SqliteObjectIndex {
    async fn create_bucket(&self, name: &str) -> Result<Bucket> {
        let name = name.to_owned();
        self.run(move |conn| {
            let tx = conn.transaction()?;
            if bucket_exists(&tx, &name)? {
                return Err(Error::BucketExists);
            }
            let created_at = Utc::now();
            tx.execute(
                "INSERT INTO buckets (name, created_at) VALUES (?1, ?2)",
                params![name, created_at],
            )?;
            tx.commit()?;
            Ok(Bucket { name, created_at })
        })
        .await
    }

    async fn get_bucket(&self, name: &str) -> Result<Bucket> {
        let name = name.to_owned();
        self.run(move |conn| {
            conn.query_row(
                "SELECT created_at FROM buckets WHERE name = ?1",
                [&name],
                |r| r.get(0),
            )
            .optional()?
            .map(|created_at| Bucket { name, created_at })
            .ok_or(Error::BucketNotFound)
        })
        .await
    }

    async fn delete_bucket(&self, name: &str) -> Result<()> {
        let name = name.to_owned();
        self.run(move |conn| {
            let tx = conn.transaction()?;
            if !bucket_exists(&tx, &name)? {
                return Err(Error::BucketNotFound);
            }
            let objects: i64 = tx.query_row(
                "SELECT count(*) FROM manifests WHERE bucket = ?1",
                [&name],
                |r| r.get(0),
            )?;
            if objects > 0 {
                return Err(Error::BucketNotEmpty);
            }
            tx.execute("DELETE FROM buckets WHERE name = ?1", [&name])?;
            Ok(tx.commit()?)
        })
        .await
    }

    async fn list_buckets(&self) -> Result<Vec<Bucket>> {
        self.run(|conn| {
            let mut stmt = conn.prepare("SELECT name, created_at FROM buckets ORDER BY name")?;
            let rows = stmt.query_map([], |r| {
                Ok(Bucket {
                    name: r.get(0)?,
                    created_at: r.get(1)?,
                })
            })?;
            Ok(rows.collect::<std::result::Result<_, _>>()?)
        })
        .await
    }

    async fn put_manifest(&self, mut manifest: Manifest) -> Result<Manifest> {
        self.run(move |conn| {
            let tx = conn.transaction()?;
            if !bucket_exists(&tx, &manifest.bucket)? {
                return Err(Error::BucketNotFound);
            }
            let previous: Option<i64> = tx
                .query_row(
                    "SELECT version FROM manifests WHERE bucket = ?1 AND key = ?2",
                    params![manifest.bucket, manifest.key],
                    |r| r.get(0),
                )
                .optional()?;
            manifest.version = previous.map_or(1, |v| v as u64 + 1);
            let json = serde_json::to_string(&manifest).map_err(|e| Error::Internal(e.to_string()))?;
            tx.execute(
                "INSERT OR REPLACE INTO manifests (bucket, key, version, size, etag, created_at, manifest)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    manifest.bucket,
                    manifest.key,
                    manifest.version as i64,
                    manifest.size as i64,
                    manifest.etag,
                    manifest.created_at,
                    json
                ],
            )?;
            tx.commit()?;
            Ok(manifest)
        })
        .await
    }

    async fn get_manifest(&self, bucket: &str, key: &str) -> Result<Manifest> {
        let (bucket, key) = (bucket.to_owned(), key.to_owned());
        self.run(move |conn| {
            let json: Option<String> = conn
                .query_row(
                    "SELECT manifest FROM manifests WHERE bucket = ?1 AND key = ?2",
                    params![bucket, key],
                    |r| r.get(0),
                )
                .optional()?;
            json.map(parse_manifest).unwrap_or_else(|| {
                Err(if bucket_exists(conn, &bucket)? {
                    Error::ObjectNotFound
                } else {
                    Error::BucketNotFound
                })
            })
        })
        .await
    }

    async fn delete_manifest(&self, bucket: &str, key: &str) -> Result<Manifest> {
        let (bucket, key) = (bucket.to_owned(), key.to_owned());
        self.run(move |conn| {
            let tx = conn.transaction()?;
            let json: Option<String> = tx
                .query_row(
                    "SELECT manifest FROM manifests WHERE bucket = ?1 AND key = ?2",
                    params![bucket, key],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(json) = json else {
                return Err(if bucket_exists(&tx, &bucket)? {
                    Error::ObjectNotFound
                } else {
                    Error::BucketNotFound
                });
            };
            tx.execute(
                "DELETE FROM manifests WHERE bucket = ?1 AND key = ?2",
                params![bucket, key],
            )?;
            tx.commit()?;
            parse_manifest(json)
        })
        .await
    }

    async fn list(
        &self,
        bucket: &str,
        prefix: &str,
        start_after: &str,
        max_keys: usize,
    ) -> Result<ListPage> {
        let (bucket, prefix, start_after) =
            (bucket.to_owned(), prefix.to_owned(), start_after.to_owned());
        self.run(move |conn| {
            if !bucket_exists(conn, &bucket)? {
                return Err(Error::BucketNotFound);
            }
            // Keys are ordered bytewise (SQLite BINARY collation). One extra row tells us if the page is truncated.
            let mut stmt = conn.prepare(
                "SELECT key, size, etag, created_at FROM manifests
                 WHERE bucket = ?1 AND key > ?2 AND key >= ?3 AND substr(key, 1, length(?3)) = ?3
                 ORDER BY key LIMIT ?4",
            )?;
            let mut objects = stmt
                .query_map(
                    params![bucket, start_after, prefix, max_keys as i64 + 1],
                    |r| {
                        Ok(ObjectSummary {
                            key: r.get(0)?,
                            size: r.get::<_, i64>(1)? as u64,
                            etag: r.get(2)?,
                            last_modified: r.get(3)?,
                        })
                    },
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let is_truncated = objects.len() > max_keys;
            objects.truncate(max_keys);
            let next_start_after = if is_truncated {
                objects.last().map(|o| o.key.clone())
            } else {
                None
            };
            Ok(ListPage {
                objects,
                is_truncated,
                next_start_after,
            })
        })
        .await
    }
}
