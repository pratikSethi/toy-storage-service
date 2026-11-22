pub mod error;
pub mod handlers;

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
use tower_http::request_id::{MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
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
    info(title = "TS2 frontdoor", description = "JSON REST object store. Errors are RFC 9457 problem details."),
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
        uuid::Uuid::new_v4().to_string().parse().ok().map(RequestId::new)
    }
}

async fn request_scope(req: Request, next: Next) -> Response {
    let id = req.headers().get(REQUEST_ID).and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    let path = req.uri().path().to_owned();
    error::REQUEST.scope((id, path), next.run(req)).await
}

pub fn router(state: AppState) -> Router {
    use handlers::*;
    let server = HeaderValue::from_str(&format!("ts2-frontdoor/{}", env!("CARGO_PKG_VERSION"))).expect("valid header");
    let layers = ServiceBuilder::new()
        .layer(SetRequestIdLayer::new(REQUEST_ID, MakeId))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|req: &Request| {
                    let id = req.headers().get(REQUEST_ID).and_then(|v| v.to_str().ok()).unwrap_or_default();
                    tracing::info_span!("request", method = %req.method(), path = %req.uri().path(), request_id = %id)
                })
                .on_response(DefaultOnResponse::new().level(tracing::Level::INFO)),
        )
        .layer(PropagateRequestIdLayer::new(REQUEST_ID))
        .layer(SetResponseHeaderLayer::overriding(header::SERVER, server))
        .layer(middleware::from_fn(request_scope));

    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/buckets", get(list_buckets).post(create_bucket))
        .route("/v1/buckets/{bucket}", get(get_bucket).delete(delete_bucket))
        .route("/v1/buckets/{bucket}/objects", get(list_objects))
        .route(
            "/v1/buckets/{bucket}/objects/{*key}",
            get(get_object).head(head_object).put(put_object).delete(delete_object),
        )
        .route("/openapi.json", get(|| async { Json(ApiDoc::openapi()) }))
        .merge(Scalar::with_url("/docs", ApiDoc::openapi()))
        .with_state(state)
        // Streaming PUT: max_object_bytes is enforced while reading, not by axum's 2 MiB default.
        .layer(DefaultBodyLimit::disable())
        .layer(layers)
}
