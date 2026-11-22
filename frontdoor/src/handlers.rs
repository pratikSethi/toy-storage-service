use crate::AppState;
use crate::error::{ApiError, Problem};
use axum::Json;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use ts2_core::{Error, object_stream, validate_bucket_name, validate_key};
use utoipa::{IntoParams, ToSchema};

type ApiResult<T> = Result<T, ApiError>;

#[derive(Deserialize, ToSchema)]
pub struct CreateBucket {
    /// Bucket name: 3 to 63 characters, lowercase letters, digits and hyphens.
    #[schema(example = "photos")]
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct BucketInfo {
    /// Bucket name.
    #[schema(example = "photos")]
    pub name: String,
    /// When the bucket was created (UTC).
    #[schema(example = "2026-11-22T10:15:00Z")]
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct BucketList {
    /// All buckets.
    pub buckets: Vec<BucketInfo>,
}

#[derive(Serialize, ToSchema)]
pub struct PutResult {
    /// Bucket the object was written to.
    #[schema(example = "photos")]
    pub bucket: String,
    /// Object key.
    #[schema(example = "docs/hello.txt")]
    pub key: String,
    /// Object size in bytes.
    #[schema(example = 48213)]
    pub size: u64,
    /// Hex sha256 of the object bytes, without quotes. The ETag header carries it quoted.
    #[schema(example = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08")]
    pub etag: String,
    /// Starts at 1 and increases by one on each overwrite of the same key.
    #[schema(example = 1)]
    pub version: u64,
}

#[derive(Serialize, ToSchema)]
pub struct ObjectInfo {
    /// Object key.
    #[schema(example = "docs/hello.txt")]
    pub key: String,
    /// Object size in bytes.
    #[schema(example = 48213)]
    pub size: u64,
    /// Hex sha256 of the object bytes, without quotes.
    #[schema(example = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08")]
    pub etag: String,
    /// When the object was last written (UTC).
    #[schema(example = "2026-11-22T10:20:00Z")]
    pub last_modified: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct ObjectList {
    /// One page of objects in lexicographic (bytewise) key order.
    pub objects: Vec<ObjectInfo>,
    /// True when more keys match than fit in this page.
    pub is_truncated: bool,
    /// Pass as `start_after` to get the next page. Null on the last page.
    #[schema(example = "docs/hello.txt")]
    pub next_start_after: Option<String>,
}

#[derive(Deserialize, IntoParams)]
pub struct ListQuery {
    /// Only keys starting with this prefix. Default: no prefix.
    #[param(example = "docs/")]
    prefix: Option<String>,
    /// Only keys after this one in bytewise order. Use `next_start_after` from the previous page.
    start_after: Option<String>,
    /// Page size. Default and maximum 1000. Larger values are capped at 1000, 0 is raised to 1.
    #[param(example = 100)]
    max_keys: Option<usize>,
}

impl From<ts2_core::Bucket> for BucketInfo {
    fn from(b: ts2_core::Bucket) -> Self {
        BucketInfo {
            name: b.name,
            created_at: b.created_at,
        }
    }
}

/// Check that the service is up
///
/// Returns the plain text `ok` as soon as the server can answer. It does not touch the object index or the chunk store.
///
/// ```sh
/// curl http://127.0.0.1:9000/healthz
/// ```
#[utoipa::path(get, path = "/healthz", tag = "Health", responses((status = 200, description = "Service is alive. Body is the text `ok`.", body = String, content_type = "text/plain")))]
pub async fn healthz() -> &'static str {
    "ok"
}

/// List buckets
///
/// Returns every bucket. The list is not paged.
///
/// ```sh
/// curl http://127.0.0.1:9000/v1/buckets
/// ```
#[utoipa::path(get, path = "/v1/buckets", tag = "Buckets", responses((status = 200, description = "All buckets.", body = BucketList)))]
pub async fn list_buckets(State(s): State<AppState>) -> ApiResult<Json<BucketList>> {
    let buckets = s
        .index
        .list_buckets()
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(BucketList { buckets }))
}

/// Create a bucket
///
/// Creates an empty bucket. Names are 3 to 63 characters of lowercase letters, digits and hyphens. A body that is not valid JSON is reported as an invalid bucket name.
///
/// ```sh
/// curl -X POST http://127.0.0.1:9000/v1/buckets -H 'content-type: application/json' -d '{"name":"photos"}'
/// ```
#[utoipa::path(
    post, path = "/v1/buckets", tag = "Buckets",
    request_body(content = CreateBucket, description = "Name of the bucket to create."),
    responses(
        (status = 201, description = "Bucket created.", body = BucketInfo),
        (status = 400, body = Problem, content_type = "application/problem+json", description = "Problem type `invalid-bucket-name`: the name breaks the naming rules or the body is not valid JSON."),
        (status = 409, body = Problem, content_type = "application/problem+json", description = "Problem type `bucket-exists`: a bucket with this name already exists."),
    )
)]
pub async fn create_bucket(
    State(s): State<AppState>,
    body: Result<Json<CreateBucket>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<BucketInfo>)> {
    let Json(req) = body.map_err(|_| Error::InvalidBucketName)?;
    validate_bucket_name(&req.name)?;
    Ok((
        StatusCode::CREATED,
        Json(s.index.create_bucket(&req.name).await?.into()),
    ))
}

/// Get a bucket
///
/// Returns the bucket name and creation time.
///
/// ```sh
/// curl http://127.0.0.1:9000/v1/buckets/photos
/// ```
#[utoipa::path(
    get, path = "/v1/buckets/{bucket}", tag = "Buckets", params(("bucket" = String, Path, description = "Bucket name, 3 to 63 characters: lowercase letters, digits, hyphens.", example = "photos")),
    responses(
        (status = 200, description = "The bucket.", body = BucketInfo),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "Problem type `bucket-not-found`: no bucket with this name."),
    )
)]
pub async fn get_bucket(
    State(s): State<AppState>,
    Path(bucket): Path<String>,
) -> ApiResult<Json<BucketInfo>> {
    Ok(Json(s.index.get_bucket(&bucket).await?.into()))
}

/// Delete a bucket
///
/// Deletes an empty bucket. A bucket that still holds objects is refused with 409, so delete its objects first.
///
/// ```sh
/// curl -X DELETE http://127.0.0.1:9000/v1/buckets/photos
/// ```
#[utoipa::path(
    delete, path = "/v1/buckets/{bucket}", tag = "Buckets", params(("bucket" = String, Path, description = "Bucket name, 3 to 63 characters: lowercase letters, digits, hyphens.", example = "photos")),
    responses(
        (status = 204, description = "Bucket deleted. No body."),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "Problem type `bucket-not-found`: no bucket with this name."),
        (status = 409, body = Problem, content_type = "application/problem+json", description = "Problem type `bucket-not-empty`: the bucket still contains objects."),
    )
)]
pub async fn delete_bucket(
    State(s): State<AppState>,
    Path(bucket): Path<String>,
) -> ApiResult<StatusCode> {
    s.index.delete_bucket(&bucket).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Upload an object
///
/// Streams the request body into the bucket under the given key. Objects under the inline threshold (64 KiB by default) are stored inside the manifest. Larger ones are split into fixed-size chunks. The `Content-Type` header is stored and returned on read, and defaults to `application/octet-stream`.
///
/// Writing to an existing key replaces the manifest and increases `version` by one. Bodies above `max_object_bytes` (256 MiB by default) are refused with 413, using `Content-Length` when present and the byte count while streaming otherwise.
///
/// ```sh
/// curl -X PUT http://127.0.0.1:9000/v1/buckets/photos/objects/docs/hello.txt -H 'content-type: text/plain' -d 'hello world'
/// ```
#[utoipa::path(
    put, path = "/v1/buckets/{bucket}/objects/{key}", tag = "Objects",
    params(("bucket" = String, Path, description = "Bucket name, 3 to 63 characters: lowercase letters, digits, hyphens.", example = "photos"), ("key" = String, Path, description = "Object key. May contain slashes, for example `docs/a/b.txt`. At most max_key_bytes bytes (default 1024).", example = "docs/hello.txt")),
    request_body(content = String, content_type = "application/octet-stream", description = "Raw object bytes. Any content type is accepted and stored with the object."),
    responses(
        (status = 201, description = "Object stored. The ETag header is the quoted hex sha256.", body = PutResult, headers(("ETag" = String, description = "Quoted hex sha256 of the object"))),
        (status = 400, body = Problem, content_type = "application/problem+json", description = "Problem type `invalid-key`: the key is empty, too long or has disallowed characters."),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "Problem type `bucket-not-found`: no bucket with this name."),
        (status = 413, body = Problem, content_type = "application/problem+json", description = "Problem type `object-too-large`: the body is larger than max_object_bytes."),
    )
)]
pub async fn put_object(
    State(s): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<Response> {
    // Admission: everything that can be decided before the body is read.
    validate_key(&key, s.limits.max_key_bytes)?;
    s.index.get_bucket(&bucket).await?;
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > s.limits.max_object_bytes) {
        return Err(Error::ObjectTooLarge {
            max: s.limits.max_object_bytes,
        }
        .into());
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    let stream = body.into_data_stream().map_err(std::io::Error::other);
    // A cancelled put leaves its chunks behind; GC removes them because min_age is at least twice this timeout.
    let put = ts2_core::put_object(
        stream,
        &bucket,
        &key,
        content_type,
        &s.limits,
        &*s.index,
        &*s.chunks,
    );
    let m = tokio::time::timeout(s.put_timeout, put)
        .await
        .map_err(|_| Error::Timeout(format!("PUT did not finish within {:?}", s.put_timeout)))??;
    let mut res = (
        StatusCode::CREATED,
        Json(PutResult {
            bucket: m.bucket,
            key: m.key,
            size: m.size,
            etag: m.etag.clone(),
            version: m.version,
        }),
    )
        .into_response();
    res.headers_mut().insert(header::ETAG, etag_value(&m.etag));
    Ok(res)
}

fn etag_value(etag: &str) -> HeaderValue {
    HeaderValue::from_str(&format!("\"{etag}\"")).expect("hex is a valid header value")
}

/// Single range only. `Ok(None)` means no usable Range header: serve the whole object.
/// Returns inclusive (start, end).
fn parse_range(headers: &HeaderMap, size: u64) -> Result<Option<(u64, u64)>, Error> {
    let unsatisfiable = Error::RangeNotSatisfiable { size };
    let Some(spec) = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
    else {
        return Ok(None);
    };
    if spec.contains(',') {
        return Err(unsatisfiable);
    }
    let Some((a, b)) = spec.trim().split_once('-') else {
        return Ok(None);
    };
    let (start, end) = match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(a), Ok(b)) if a <= b => (a, b),
        (Ok(a), Err(_)) if b.is_empty() => (a, u64::MAX),
        (Err(_), Ok(n)) if a.is_empty() => {
            if n == 0 || size == 0 {
                return Err(unsatisfiable);
            }
            (size.saturating_sub(n), size - 1)
        }
        _ => return Ok(None),
    };
    if start >= size {
        return Err(unsatisfiable);
    }
    Ok(Some((start, end.min(size - 1))))
}

async fn object_response(
    s: AppState,
    bucket: String,
    key: String,
    headers: HeaderMap,
    with_body: bool,
) -> ApiResult<Response> {
    let m = s.index.get_manifest(&bucket, &key).await?;
    let range = parse_range(&headers, m.size)?;
    let (status, start, end) = match range {
        Some((a, b)) => (StatusCode::PARTIAL_CONTENT, a, b + 1),
        None => (StatusCode::OK, 0, m.size),
    };
    let mut res = Response::new(Body::empty());
    *res.status_mut() = status;
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&m.content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    h.insert(header::ETAG, etag_value(&m.etag));
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    h.insert(header::CONTENT_LENGTH, (end - start).into());
    if range.is_some() {
        h.insert(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{}", end - 1, m.size)
                .parse()
                .unwrap(),
        );
    }
    if with_body {
        *res.body_mut() = Body::from_stream(
            object_stream(m, s.chunks.clone(), start, end).map_err(std::io::Error::other),
        );
    }
    Ok(res)
}

/// Download an object
///
/// Streams the object bytes with its stored `Content-Type`, `ETag` and `Content-Length`. Send one `Range` header to read part of the object: `bytes=a-b`, `bytes=a-` or `bytes=-n` (last n bytes). A satisfiable range returns 206 with `Content-Range`. Several ranges in one header are not supported and return 416.
///
/// ```sh
/// curl -H 'Range: bytes=0-99' http://127.0.0.1:9000/v1/buckets/photos/objects/docs/hello.txt
/// ```
#[utoipa::path(
    get, path = "/v1/buckets/{bucket}/objects/{key}", tag = "Objects",
    params(("bucket" = String, Path, description = "Bucket name, 3 to 63 characters: lowercase letters, digits, hyphens.", example = "photos"), ("key" = String, Path, description = "Object key. May contain slashes, for example `docs/a/b.txt`. At most max_key_bytes bytes (default 1024).", example = "docs/hello.txt"),
        ("Range" = Option<String>, Header, description = "A single byte range: `bytes=a-b`, `bytes=a-` or `bytes=-n`.", example = "bytes=0-99")),
    responses(
        (status = 200, description = "Full object bytes, with Content-Type, ETag, Content-Length and Accept-Ranges: bytes.", body = String, content_type = "application/octet-stream"),
        (status = 206, description = "The requested byte range, with a Content-Range header.", body = String, content_type = "application/octet-stream"),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "Problem type `bucket-not-found` or `object-not-found`."),
        (status = 416, body = Problem, content_type = "application/problem+json", description = "Problem type `range-not-satisfiable`: the range starts past the end of the object or lists several ranges. Content-Range is `bytes */size`."),
    )
)]
pub async fn get_object(
    State(s): State<AppState>,
    Path((b, k)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    object_response(s, b, k, headers, true).await
}

/// Get object metadata
///
/// Returns the headers a GET would return (`Content-Type`, `ETag`, `Content-Length`, `Accept-Ranges`) with no body. A HEAD response has no problem body, so errors show only the status code.
///
/// ```sh
/// curl -I http://127.0.0.1:9000/v1/buckets/photos/objects/docs/hello.txt
/// ```
#[utoipa::path(
    head, path = "/v1/buckets/{bucket}/objects/{key}", tag = "Objects",
    params(("bucket" = String, Path, description = "Bucket name, 3 to 63 characters: lowercase letters, digits, hyphens.", example = "photos"), ("key" = String, Path, description = "Object key. May contain slashes, for example `docs/a/b.txt`. At most max_key_bytes bytes (default 1024).", example = "docs/hello.txt")),
    responses(
        (status = 200, description = "Object headers only. No body."),
        (status = 404, description = "Problem type `bucket-not-found` or `object-not-found`. HEAD responses carry no body."),
    )
)]
pub async fn head_object(
    State(s): State<AppState>,
    Path((b, k)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    object_response(s, b, k, headers, false).await
}

/// Delete an object
///
/// Removes the object's manifest and releases its chunks. Deleting a key that does not exist returns 404.
///
/// ```sh
/// curl -X DELETE http://127.0.0.1:9000/v1/buckets/photos/objects/docs/hello.txt
/// ```
#[utoipa::path(
    delete, path = "/v1/buckets/{bucket}/objects/{key}", tag = "Objects",
    params(("bucket" = String, Path, description = "Bucket name, 3 to 63 characters: lowercase letters, digits, hyphens.", example = "photos"), ("key" = String, Path, description = "Object key. May contain slashes, for example `docs/a/b.txt`. At most max_key_bytes bytes (default 1024).", example = "docs/hello.txt")),
    responses(
        (status = 204, description = "Object deleted. No body."),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "Problem type `bucket-not-found` or `object-not-found`."),
    )
)]
pub async fn delete_object(
    State(s): State<AppState>,
    Path((b, k)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    ts2_core::delete_object(&*s.index, &*s.chunks, &b, &k).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// List objects
///
/// Lists keys in lexicographic (bytewise) order. `prefix` narrows the keys, `start_after` resumes after a given key, and `max_keys` sets the page size (default and maximum 1000). When `is_truncated` is true, pass `next_start_after` as `start_after` to get the next page.
///
/// ```sh
/// curl 'http://127.0.0.1:9000/v1/buckets/photos/objects?prefix=docs/&max_keys=100'
/// ```
#[utoipa::path(
    get, path = "/v1/buckets/{bucket}/objects", tag = "Objects",
    params(("bucket" = String, Path, description = "Bucket name, 3 to 63 characters: lowercase letters, digits, hyphens.", example = "photos"), ListQuery),
    responses(
        (status = 200, description = "One page of objects.", body = ObjectList),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "Problem type `bucket-not-found`: no bucket with this name."),
    )
)]
pub async fn list_objects(
    State(s): State<AppState>,
    Path(bucket): Path<String>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<ObjectList>> {
    let max_keys = q.max_keys.unwrap_or(1000).clamp(1, 1000);
    let page = s
        .index
        .list(
            &bucket,
            q.prefix.as_deref().unwrap_or(""),
            q.start_after.as_deref().unwrap_or(""),
            max_keys,
        )
        .await?;
    Ok(Json(ObjectList {
        objects: page
            .objects
            .into_iter()
            .map(|o| ObjectInfo {
                key: o.key,
                size: o.size,
                etag: o.etag,
                last_modified: o.last_modified,
            })
            .collect(),
        is_truncated: page.is_truncated,
        next_start_after: page.next_start_after,
    }))
}
