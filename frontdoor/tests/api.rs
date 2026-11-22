use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use ts2_core::{Limits, LocalChunkStore, SqliteObjectIndex};
use ts2_frontdoor::{ApiDoc, AppState, router};
use utoipa::OpenApi;

const MIB: usize = 1024 * 1024;

fn limits() -> Limits {
    Limits { max_object_bytes: 256 * MIB as u64, max_key_bytes: 1024, inline_threshold_bytes: 64 * 1024, chunk_bytes: 4 * MIB }
}

fn app(dir: &std::path::Path, limits: Limits) -> Router {
    router(AppState {
        index: Arc::new(SqliteObjectIndex::open(&dir.join("index.db")).unwrap()),
        chunks: Arc::new(LocalChunkStore::open(dir).unwrap()),
        limits,
    })
}

async fn send(app: &Router, method: Method, uri: &str, headers: &[(&str, &str)], body: Vec<u8>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut req = Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = app.clone().oneshot(req.body(Body::from(body)).unwrap()).await.unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    (status, headers, res.into_body().collect().await.unwrap().to_bytes().to_vec())
}

async fn get(app: &Router, uri: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
    send(app, Method::GET, uri, &[], vec![]).await
}

async fn ranged(app: &Router, uri: &str, range: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
    send(app, Method::GET, uri, &[("range", range)], vec![]).await
}

async fn post_bucket(app: &Router, name: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
    let body = json!({ "name": name }).to_string().into_bytes();
    send(app, Method::POST, "/v1/buckets", &[("content-type", "application/json")], body).await
}

async fn create_bucket(app: &Router, name: &str) {
    assert_eq!(post_bucket(app, name).await.0, StatusCode::CREATED);
}

fn json_of(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap()
}

fn problem_type(body: &[u8]) -> String {
    json_of(body)["type"].as_str().unwrap().to_owned()
}

fn pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

#[tokio::test]
async fn bucket_create_list_get_delete() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path(), limits());
    create_bucket(&app, "photos").await;
    let (_, _, body) = get(&app, "/v1/buckets").await;
    assert_eq!(json_of(&body)["buckets"][0]["name"], "photos");
    assert_eq!(get(&app, "/v1/buckets/photos").await.0, StatusCode::OK);
    assert_eq!(send(&app, Method::DELETE, "/v1/buckets/photos", &[], vec![]).await.0, StatusCode::NO_CONTENT);
    assert_eq!(get(&app, "/v1/buckets/photos").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn small_object_roundtrip_is_inline() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path(), limits());
    create_bucket(&app, "bkt1").await;
    let uri = "/v1/buckets/bkt1/objects/hello.txt";
    let (status, headers, body) = send(&app, Method::PUT, uri, &[("content-type", "text/plain")], b"hello".to_vec()).await;
    assert_eq!(status, StatusCode::CREATED);
    let sha = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"; // sha256("hello")
    assert_eq!(json_of(&body), json!({ "bucket": "bkt1", "key": "hello.txt", "size": 5, "etag": sha, "version": 1 }));
    assert_eq!(headers["etag"], format!("\"{sha}\"").as_str());

    let (status, headers, body) = get(&app, uri).await;
    assert_eq!((status, body), (StatusCode::OK, b"hello".to_vec()));
    assert_eq!(headers["etag"], format!("\"{sha}\"").as_str());
    assert_eq!(headers["content-type"], "text/plain");
    assert_eq!(headers["accept-ranges"], "bytes");
    assert_eq!(headers["content-length"], "5");
    assert!(dir.path().join("data/chunks").read_dir().unwrap().next().is_none(), "inline objects write no chunks");

    let (status, headers, body) = send(&app, Method::HEAD, uri, &[], vec![]).await;
    assert_eq!((status, body.len()), (StatusCode::OK, 0));
    assert_eq!(headers["content-length"], "5");
    assert_eq!(send(&app, Method::DELETE, uri, &[], vec![]).await.0, StatusCode::NO_CONTENT);
    assert_eq!(get(&app, uri).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn ten_mib_object_roundtrip_and_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path(), limits());
    create_bucket(&app, "big").await;
    let uri = "/v1/buckets/big/objects/dir/data.bin";
    let data = pattern(10 * MIB); // 3 chunks: 4 + 4 + 2 MiB
    let (status, put_headers, _) = send(&app, Method::PUT, uri, &[], data.clone()).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, headers, body) = get(&app, uri).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body == data);
    assert_eq!(headers["etag"], put_headers["etag"]);

    // Across the first chunk boundary at 4 MiB.
    let edge = 4 * MIB;
    let r = format!("bytes={}-{}", edge - 5, edge + 4);
    let (status, headers, body) = send(&app, Method::GET, uri, &[("range", &r)], vec![]).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(body, data[edge - 5..edge + 5]);
    assert_eq!(headers["content-range"], format!("bytes {}-{}/{}", edge - 5, edge + 4, data.len()).as_str());

    let range = |r: &'static str| ranged(&app, uri, r);
    let (status, _, body) = range("bytes=-10").await;
    assert_eq!((status, body), (StatusCode::PARTIAL_CONTENT, data[data.len() - 10..].to_vec()));
    let (status, _, body) = range("bytes=10485750-").await;
    assert_eq!((status, body), (StatusCode::PARTIAL_CONTENT, data[10485750..].to_vec()));
    let (status, headers, body) = range("bytes=99999999-").await;
    assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(headers["content-range"], format!("bytes */{}", data.len()).as_str());
    assert_eq!(problem_type(&body), "https://ts2.dev/problems/range-not-satisfiable");
    assert_eq!(range("bytes=0-1,5-6").await.0, StatusCode::RANGE_NOT_SATISFIABLE); // multi-range unsupported
}

#[tokio::test]
async fn too_large_is_413_with_and_without_content_length() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path(), Limits { max_object_bytes: 1000, ..limits() });
    create_bucket(&app, "small").await;
    let uri = "/v1/buckets/small/objects/k";
    let (status, headers, body) = send(&app, Method::PUT, uri, &[("content-length", "2000")], vec![0; 2000]).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(headers["content-type"], "application/problem+json");
    let p = json_of(&body);
    assert_eq!(p["type"], "https://ts2.dev/problems/object-too-large");
    assert_eq!((&p["status"], &p["instance"]), (&json!(413), &json!(uri)));
    assert_eq!(p["request_id"], headers["x-ts2-request-id"].to_str().unwrap());

    let (status, ..) = send(&app, Method::PUT, uri, &[], vec![0; 2000]).await; // no Content-Length: caught while streaming
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(get(&app, uri).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn problems_404_409_400() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path(), limits());
    let (status, _, body) = get(&app, "/v1/buckets/nope/objects/k").await;
    assert_eq!((status, problem_type(&body)), (StatusCode::NOT_FOUND, "https://ts2.dev/problems/bucket-not-found".into()));
    create_bucket(&app, "dup").await;
    let (status, _, body) = get(&app, "/v1/buckets/dup/objects/missing").await;
    assert_eq!((status, problem_type(&body)), (StatusCode::NOT_FOUND, "https://ts2.dev/problems/object-not-found".into()));

    let (status, _, body) = post_bucket(&app, "dup").await;
    assert_eq!((status, problem_type(&body)), (StatusCode::CONFLICT, "https://ts2.dev/problems/bucket-exists".into()));
    send(&app, Method::PUT, "/v1/buckets/dup/objects/k", &[], b"x".to_vec()).await;
    let (status, _, body) = send(&app, Method::DELETE, "/v1/buckets/dup", &[], vec![]).await;
    assert_eq!((status, problem_type(&body)), (StatusCode::CONFLICT, "https://ts2.dev/problems/bucket-not-empty".into()));

    let (status, _, body) = post_bucket(&app, "Bad_Name").await;
    assert_eq!((status, problem_type(&body)), (StatusCode::BAD_REQUEST, "https://ts2.dev/problems/invalid-bucket-name".into()));
    let (status, _, body) = send(&app, Method::PUT, "/v1/buckets/dup/objects/a/../b", &[], b"x".to_vec()).await;
    assert_eq!((status, problem_type(&body)), (StatusCode::BAD_REQUEST, "https://ts2.dev/problems/invalid-key".into()));
    let long = format!("/v1/buckets/dup/objects/{}", "k".repeat(1025));
    assert_eq!(send(&app, Method::PUT, &long, &[], b"x".to_vec()).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn list_with_prefix_and_pagination() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path(), limits());
    create_bucket(&app, "lists").await;
    for key in ["a/1", "a/2", "a/3", "b/1"] {
        send(&app, Method::PUT, &format!("/v1/buckets/lists/objects/{key}"), &[], key.as_bytes().to_vec()).await;
    }
    let (_, _, body) = get(&app, "/v1/buckets/lists/objects?prefix=a/&max_keys=2").await;
    let page = json_of(&body);
    assert_eq!(page["objects"].as_array().unwrap().len(), 2);
    assert_eq!((&page["is_truncated"], &page["next_start_after"]), (&json!(true), &json!("a/2")));
    let (_, _, body) = get(&app, "/v1/buckets/lists/objects?prefix=a/&max_keys=2&start_after=a/2").await;
    let page = json_of(&body);
    assert_eq!(page["objects"][0]["key"], "a/3");
    assert_eq!((&page["is_truncated"], &page["next_start_after"]), (&json!(false), &Value::Null));
    let (_, _, body) = get(&app, "/v1/buckets/lists/objects").await;
    assert_eq!(json_of(&body)["objects"].as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn every_response_has_server_and_request_id() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path(), limits());
    let (status, headers, body) = get(&app, "/healthz").await;
    assert_eq!((status, body), (StatusCode::OK, b"ok".to_vec()));
    assert_eq!(headers["server"], concat!("ts2-frontdoor/", env!("CARGO_PKG_VERSION")));
    assert!(!headers["x-ts2-request-id"].is_empty());
    let (_, headers, _) = send(&app, Method::GET, "/healthz", &[("x-ts2-request-id", "abc-123")], vec![]).await;
    assert_eq!(headers["x-ts2-request-id"], "abc-123");
    assert_eq!(get(&app, "/openapi.json").await.0, StatusCode::OK);
    assert_eq!(get(&app, "/docs").await.0, StatusCode::OK);
}

/// Keeps docs/api/openapi.yaml in step with the handlers, so API changes show up in review diffs.
#[test]
fn write_openapi_yaml() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/api/openapi.yaml");
    std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
    std::fs::write(path, serde_yaml_ng::to_string(&ApiDoc::openapi()).unwrap()).unwrap();
}
