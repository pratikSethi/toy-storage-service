use crate::{ChunkStore, Error, Result};
use bytes::Bytes;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

/// One file per chunk: `<root>/data/chunks/ab/cd/<id>`. Writes go to `<root>/tmp/`, are fsynced, then renamed.
pub struct LocalChunkStore {
    root: PathBuf,
}

impl LocalChunkStore {
    /// Creates the layout and clears `tmp/`: leftovers there are writes that never finished.
    pub fn open(root: &Path) -> Result<Self> {
        let tmp = root.join("tmp");
        if tmp.exists() {
            fs::remove_dir_all(&tmp)?;
        }
        fs::create_dir_all(&tmp)?;
        fs::create_dir_all(root.join("data/chunks"))?;
        Ok(Self { root: root.to_owned() })
    }

    fn path(&self, id: &str) -> Result<PathBuf> {
        if id.len() < 4 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Internal(format!("bad chunk id {id:?}")));
        }
        Ok(self.root.join("data/chunks").join(&id[0..2]).join(&id[2..4]).join(id))
    }
}

/// Flush to stable storage. fsync on macOS does not flush the drive cache, F_FULLFSYNC does.
fn full_sync(f: &File) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::fcntl(f.as_raw_fd(), libc::F_FULLFSYNC) } == -1 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    f.sync_all()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| Error::Internal(e.to_string()))?
}

#[async_trait::async_trait]
impl ChunkStore for LocalChunkStore {
    async fn put(&self, chunk_id: &str, data: Bytes) -> Result<()> {
        let dest = self.path(chunk_id)?;
        let tmp = self.root.join("tmp").join(chunk_id);
        blocking(move || {
            let mut f = File::create(&tmp)?;
            f.write_all(&data)?;
            full_sync(&f)?;
            let dir = dest.parent().expect("chunk path has a parent");
            fs::create_dir_all(dir)?;
            fs::rename(&tmp, &dest)?;
            // Make the rename itself durable.
            full_sync(&File::open(dir)?)?;
            Ok(())
        })
        .await
    }

    async fn get(&self, chunk_id: &str, range: Option<Range<u64>>) -> Result<Bytes> {
        let path = self.path(chunk_id)?;
        blocking(move || {
            let mut f = File::open(&path)?;
            let mut buf = Vec::new();
            match range {
                None => f.read_to_end(&mut buf)?,
                Some(r) => {
                    f.seek(SeekFrom::Start(r.start))?;
                    f.take(r.end - r.start).read_to_end(&mut buf)?
                }
            };
            Ok(buf.into())
        })
        .await
    }

    async fn delete(&self, chunk_id: &str) -> Result<()> {
        let path = self.path(chunk_id)?;
        blocking(move || match fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        })
        .await
    }
}
