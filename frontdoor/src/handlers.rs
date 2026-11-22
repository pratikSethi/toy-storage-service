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
    /// 3 to 63 characters: lowercase letters, digits, hyphens.
    #[schema(example = "photos")]
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct BucketInfo {
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct BucketList {
    pub buckets: Vec<BucketInfo>,
}

#[derive(Serialize, ToSchema)]
pub struct PutResult {
    pub bucket: String,
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub version: u64,
}

#[derive(Serialize, ToSchema)]
pub struct ObjectInfo {
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub last_modified: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct ObjectList {
    pub objects: Vec<ObjectInfo>,
    pub is_truncated: bool,
    /// Pass as `start_after` to get the next page.
    pub next_start_after: Option<String>,
}

#[derive(Deserialize, IntoParams)]
pub struct ListQuery {
    /// Only keys starting with this prefix.
    prefix: Option<String>,
    /// Only keys after this one, in bytewise order.
    start_after: Option<String>,
    /// Page size, default and maximum 1000.
    max_keys: Option<usize>,
}

impl From<ts2_core::Bucket> for BucketInfo {
    fn from(b: ts2_core::Bucket) -> Self {
        BucketInfo { name: b.name, created_at: b.created_at }
    }
}

#[utoipa::path(get, path = "/healthz", responses((status = 200, description = "Alive", body = String)))]
pub async fn healthz() -> &'static str {
    "ok"
}

#[utoipa::path(get, path = "/v1/buckets", responses((status = 200, body = BucketList)))]
pub async fn list_buckets(State(s): State<AppState>) -> ApiResult<Json<BucketList>> {
    let buckets = s.index.list_buckets().await?.into_iter().map(Into::into).collect();
    Ok(Json(BucketList { buckets }))
}

#[utoipa::path(
    post, path = "/v1/buckets", request_body = CreateBucket,
    responses(
        (status = 201, body = BucketInfo),
        (status = 400, body = Problem, content_type = "application/problem+json", description = "invalid-bucket-name"),
        (status = 409, body = Problem, content_type = "application/problem+json", description = "bucket-exists"),
    )
)]
pub async fn create_bucket(
    State(s): State<AppState>,
    body: Result<Json<CreateBucket>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<BucketInfo>)> {
    let Json(req) = body.map_err(|_| Error::InvalidBucketName)?;
    validate_bucket_name(&req.name)?;
    Ok((StatusCode::CREATED, Json(s.index.create_bucket(&req.name).await?.into())))
}

#[utoipa::path(
    get, path = "/v1/buckets/{bucket}", params(("bucket" = String, Path, description = "Bucket name")),
    responses(
        (status = 200, body = BucketInfo),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "bucket-not-found"),
    )
)]
pub async fn get_bucket(State(s): State<AppState>, Path(bucket): Path<String>) -> ApiResult<Json<BucketInfo>> {
    Ok(Json(s.index.get_bucket(&bucket).await?.into()))
}

#[utoipa::path(
    delete, path = "/v1/buckets/{bucket}", params(("bucket" = String, Path, description = "Bucket name")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "bucket-not-found"),
        (status = 409, body = Problem, content_type = "application/problem+json", description = "bucket-not-empty"),
    )
)]
pub async fn delete_bucket(State(s): State<AppState>, Path(bucket): Path<String>) -> ApiResult<StatusCode> {
    s.index.delete_bucket(&bucket).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    put, path = "/v1/buckets/{bucket}/objects/{key}",
    params(("bucket" = String, Path, description = "Bucket name"), ("key" = String, Path, description = "Object key, may contain slashes")),
    request_body(content = String, content_type = "application/octet-stream"),
    responses(
        (status = 201, body = PutResult, headers(("ETag" = String, description = "Quoted hex sha256 of the object"))),
        (status = 400, body = Problem, content_type = "application/problem+json", description = "invalid-key"),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "bucket-not-found"),
        (status = 413, body = Problem, content_type = "application/problem+json", description = "object-too-large"),
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
    let declared = headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > s.limits.max_object_bytes) {
        return Err(Error::ObjectTooLarge { max: s.limits.max_object_bytes }.into());
    }
    let content_type = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream");
    let stream = body.into_data_stream().map_err(std::io::Error::other);
    let m = ts2_core::put_object(stream, &bucket, &key, content_type, &s.limits, &*s.index, &*s.chunks).await?;
    let mut res = (
        StatusCode::CREATED,
        Json(PutResult { bucket: m.bucket, key: m.key, size: m.size, etag: m.etag.clone(), version: m.version }),
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
    let Some(spec) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("bytes=")) else {
        return Ok(None);
    };
    if spec.contains(',') {
        return Err(unsatisfiable);
    }
    let Some((a, b)) = spec.trim().split_once('-') else { return Ok(None) };
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

async fn object_response(s: AppState, bucket: String, key: String, headers: HeaderMap, with_body: bool) -> ApiResult<Response> {
    let m = s.index.get_manifest(&bucket, &key).await?;
    let range = parse_range(&headers, m.size)?;
    let (status, start, end) = match range {
        Some((a, b)) => (StatusCode::PARTIAL_CONTENT, a, b + 1),
        None => (StatusCode::OK, 0, m.size),
    };
    let mut res = Response::new(Body::empty());
    *res.status_mut() = status;
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&m.content_type).unwrap_or(HeaderValue::from_static("application/octet-stream")));
    h.insert(header::ETAG, etag_value(&m.etag));
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    h.insert(header::CONTENT_LENGTH, (end - start).into());
    if range.is_some() {
        h.insert(header::CONTENT_RANGE, format!("bytes {start}-{}/{}", end - 1, m.size).parse().unwrap());
    }
    if with_body {
        *res.body_mut() = Body::from_stream(object_stream(m, s.chunks.clone(), start, end).map_err(std::io::Error::other));
    }
    Ok(res)
}

#[utoipa::path(
    get, path = "/v1/buckets/{bucket}/objects/{key}",
    params(("bucket" = String, Path, description = "Bucket name"), ("key" = String, Path, description = "Object key, may contain slashes")),
    responses(
        (status = 200, description = "Object bytes", body = String, content_type = "application/octet-stream"),
        (status = 206, description = "Requested byte range. Supported forms: bytes=a-b, bytes=a-, bytes=-n"),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "bucket-not-found or object-not-found"),
        (status = 416, body = Problem, content_type = "application/problem+json", description = "range-not-satisfiable (multi-range is not supported)"),
    )
)]
pub async fn get_object(State(s): State<AppState>, Path((b, k)): Path<(String, String)>, headers: HeaderMap) -> ApiResult<Response> {
    object_response(s, b, k, headers, true).await
}

#[utoipa::path(
    head, path = "/v1/buckets/{bucket}/objects/{key}",
    params(("bucket" = String, Path, description = "Bucket name"), ("key" = String, Path, description = "Object key, may contain slashes")),
    responses(
        (status = 200, description = "Object headers only"),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "bucket-not-found or object-not-found"),
    )
)]
pub async fn head_object(State(s): State<AppState>, Path((b, k)): Path<(String, String)>, headers: HeaderMap) -> ApiResult<Response> {
    object_response(s, b, k, headers, false).await
}

#[utoipa::path(
    delete, path = "/v1/buckets/{bucket}/objects/{key}",
    params(("bucket" = String, Path, description = "Bucket name"), ("key" = String, Path, description = "Object key, may contain slashes")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "bucket-not-found or object-not-found"),
    )
)]
pub async fn delete_object(State(s): State<AppState>, Path((b, k)): Path<(String, String)>) -> ApiResult<StatusCode> {
    ts2_core::delete_object(&*s.index, &*s.chunks, &b, &k).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/v1/buckets/{bucket}/objects",
    params(("bucket" = String, Path, description = "Bucket name"), ListQuery),
    responses(
        (status = 200, body = ObjectList),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "bucket-not-found"),
    )
)]
pub async fn list_objects(State(s): State<AppState>, Path(bucket): Path<String>, Query(q): Query<ListQuery>) -> ApiResult<Json<ObjectList>> {
    let max_keys = q.max_keys.unwrap_or(1000).clamp(1, 1000);
    let page = s.index.list(&bucket, q.prefix.as_deref().unwrap_or(""), q.start_after.as_deref().unwrap_or(""), max_keys).await?;
    Ok(Json(ObjectList {
        objects: page.objects.into_iter().map(|o| ObjectInfo { key: o.key, size: o.size, etag: o.etag, last_modified: o.last_modified }).collect(),
        is_truncated: page.is_truncated,
        next_start_after: page.next_start_after,
    }))
}
