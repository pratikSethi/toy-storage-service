pub mod error;
pub mod gc;
pub mod handlers;
pub mod metrics;

use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{HeaderName, HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::request_id::{
    MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer,
};
use tower_http::set_header::SetResponseHeaderLayer;
use ts2_core::{ChunkStore, Limits, ObjectIndex};
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};

#[derive(Deserialize)]
pub struct Config {
    pub listen: String,
    pub data_dir: PathBuf,
    pub sqlite_path: PathBuf,
    pub limits: Limits,
}

#[derive(Clone)]
pub struct AppState {
    pub index: Arc<dyn ObjectIndex>,
    pub chunks: Arc<dyn ChunkStore>,
    pub limits: Limits,
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "TS2 frontdoor",
        description = "TS2 is a small S3-like object store. This is its JSON REST API: buckets hold objects, and objects are addressed by key, which may contain slashes.\n\nThe API is version 1 and lives under `/v1`. Every response carries an `x-ts2-request-id` header. Errors are RFC 9457 problem details (`application/problem+json`) with a `type` URL and a `request_id` field that matches that header."
    ),
    tags(
        (name = "Buckets", description = "Create, inspect, list and delete buckets."),
        (name = "Objects", description = "Upload, download, inspect, list and delete objects."),
        (name = "Health", description = "Liveness check.")
    ),
    paths(
        handlers::healthz,
        handlers::list_buckets,
        handlers::create_bucket,
        handlers::get_bucket,
        handlers::delete_bucket,
        handlers::put_object,
        handlers::get_object,
        handlers::head_object,
        handlers::delete_object,
        handlers::list_objects
    ),
    components(schemas(
        handlers::CreateBucket,
        handlers::BucketInfo,
        handlers::BucketList,
        handlers::PutResult,
        handlers::ObjectInfo,
        handlers::ObjectList,
        error::Problem
    ))
)]
pub struct ApiDoc;

const REQUEST_ID: HeaderName = HeaderName::from_static("x-ts2-request-id");

#[derive(Clone)]
struct MakeId;

impl MakeRequestId for MakeId {
    fn make_request_id<B>(&mut self, _: &axum::http::Request<B>) -> Option<RequestId> {
        uuid::Uuid::new_v4()
            .to_string()
            .parse()
            .ok()
            .map(RequestId::new)
    }
}

async fn request_scope(req: Request, next: Next) -> Response {
    let id = req
        .headers()
        .get(REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let path = req.uri().path().to_owned();
    error::REQUEST.scope((id, path), next.run(req)).await
}

pub fn router(state: AppState) -> Router {
    router_with_meter(state, &opentelemetry::global::meter("ts2-frontdoor"))
}

pub fn router_with_meter(state: AppState, meter: &opentelemetry::metrics::Meter) -> Router {
    use handlers::*;
    let metrics = metrics::Metrics::new(meter);
    let server = HeaderValue::from_str(&format!("ts2-frontdoor/{}", env!("CARGO_PKG_VERSION")))
        .expect("valid header");
    let layers = ServiceBuilder::new()
        .layer(SetRequestIdLayer::new(REQUEST_ID, MakeId))
        .layer(middleware::from_fn(move |req, next| {
            metrics::track(metrics.clone(), req, next)
        }))
        .layer(PropagateRequestIdLayer::new(REQUEST_ID))
        .layer(SetResponseHeaderLayer::overriding(header::SERVER, server))
        .layer(middleware::from_fn(request_scope));

    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/buckets", get(list_buckets).post(create_bucket))
        .route(
            "/v1/buckets/{bucket}",
            get(get_bucket).delete(delete_bucket),
        )
        .route("/v1/buckets/{bucket}/objects", get(list_objects))
        .route(
            "/v1/buckets/{bucket}/objects/{*key}",
            get(get_object)
                .head(head_object)
                .put(put_object)
                .delete(delete_object),
        )
        .route("/openapi.json", get(|| async { Json(ApiDoc::openapi()) }))
        .merge(Scalar::with_url("/docs", ApiDoc::openapi()))
        .with_state(state)
        // Streaming PUT: max_object_bytes is enforced while reading, not by axum's 2 MiB default.
        .layer(DefaultBodyLimit::disable())
        .layer(layers)
}
