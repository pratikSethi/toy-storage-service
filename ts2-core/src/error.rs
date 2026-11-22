#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("bucket name must be 3 to 63 characters: lowercase letters, digits and hyphens")]
    InvalidBucketName,
    #[error("{0}")]
    InvalidKey(String),
    #[error("bucket does not exist")]
    BucketNotFound,
    #[error("bucket already exists")]
    BucketExists,
    #[error("bucket is not empty")]
    BucketNotEmpty,
    #[error("object does not exist")]
    ObjectNotFound,
    #[error("object exceeds the maximum size of {max} bytes")]
    ObjectTooLarge { max: u64 },
    #[error("range not satisfiable for an object of {size} bytes")]
    RangeNotSatisfiable { size: u64 },
    #[error("chunk {0} failed its checksum")]
    Corrupt(String),
    #[error("chunk {0} not found on its node")]
    ChunkNotFound(String),
    /// A backing service cannot be reached or is starting up (gRPC UNAVAILABLE).
    #[error("backing service unavailable: {0}")]
    Unavailable(String),
    /// A backing service missed its deadline.
    #[error("backing service timed out: {0}")]
    Timeout(String),
    #[error("chunk node is out of disk space")]
    DiskFull,
    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Internal(e.to_string())
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Internal(e.to_string())
    }
}
