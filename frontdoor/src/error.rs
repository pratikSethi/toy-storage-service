use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use ts2_core::Error;
use utoipa::ToSchema;

tokio::task_local! {
    /// (request id, request path), set by the request-scope middleware so errors can fill `request_id` and `instance`.
    pub static REQUEST: (String, String);
}

/// RFC 9457 problem details.
#[derive(Serialize, ToSchema)]
pub struct Problem {
    /// URI reference identifying the problem type.
    #[serde(rename = "type")]
    #[schema(rename = "type", example = "https://ts2.dev/problems/object-not-found")]
    pub kind: String,
    /// Short, human-readable summary of the problem type.
    #[schema(example = "Object not found")]
    pub title: String,
    /// HTTP status code, repeated from the response.
    #[schema(example = 404)]
    pub status: u16,
    /// Explanation specific to this occurrence. Internal errors give no detail.
    #[schema(example = "object not found")]
    pub detail: String,
    /// The request path.
    #[schema(example = "/v1/buckets/photos/objects/missing.txt")]
    pub instance: String,
    /// Value of the `x-ts2-request-id` response header, for correlating with logs.
    #[schema(example = "2f1c6b0e-8a52-4e1b-9d7c-0a3c5b1f7e44")]
    pub request_id: String,
}

pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, slug, title) = match &self.0 {
            Error::InvalidBucketName => (
                StatusCode::BAD_REQUEST,
                "invalid-bucket-name",
                "Invalid bucket name",
            ),
            Error::InvalidKey(_) => (StatusCode::BAD_REQUEST, "invalid-key", "Invalid key"),
            Error::BucketNotFound => (
                StatusCode::NOT_FOUND,
                "bucket-not-found",
                "Bucket not found",
            ),
            Error::ObjectNotFound => (
                StatusCode::NOT_FOUND,
                "object-not-found",
                "Object not found",
            ),
            Error::BucketExists => (
                StatusCode::CONFLICT,
                "bucket-exists",
                "Bucket already exists",
            ),
            Error::BucketNotEmpty => (StatusCode::CONFLICT, "bucket-not-empty", "Bucket not empty"),
            Error::ObjectTooLarge { .. } => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "object-too-large",
                "Object too large",
            ),
            Error::RangeNotSatisfiable { .. } => (
                StatusCode::RANGE_NOT_SATISFIABLE,
                "range-not-satisfiable",
                "Range not satisfiable",
            ),
            Error::ChunkNotFound(_) => {
                (StatusCode::NOT_FOUND, "chunk-not-found", "Chunk not found")
            }
            Error::Unavailable(_) => {
                tracing::warn!(error = %self.0, "backing service unavailable");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "service-unavailable",
                    "Service unavailable",
                )
            }
            Error::Timeout(_) => {
                tracing::warn!(error = %self.0, "backing service timed out");
                (
                    StatusCode::GATEWAY_TIMEOUT,
                    "upstream-timeout",
                    "Upstream timeout",
                )
            }
            Error::DiskFull => (
                StatusCode::INSUFFICIENT_STORAGE,
                "insufficient-storage",
                "Insufficient storage",
            ),
            Error::Corrupt(_) | Error::Internal(_) => {
                tracing::error!(error = %self.0, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "Internal error",
                )
            }
        };
        // Internal and upstream details (addresses, gRPC messages) stay in the log.
        let detail = if status == StatusCode::INTERNAL_SERVER_ERROR {
            "internal error".into()
        } else if matches!(self.0, Error::Unavailable(_) | Error::Timeout(_)) {
            title.to_lowercase()
        } else {
            self.0.to_string()
        };
        let (request_id, instance) = REQUEST.try_with(Clone::clone).unwrap_or_default();
        let problem = Problem {
            kind: format!("https://ts2.dev/problems/{slug}"),
            title: title.into(),
            status: status.as_u16(),
            detail,
            instance,
            request_id,
        };
        let mut res = (status, serde_json::to_string(&problem).unwrap_or_default()).into_response();
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if let Error::RangeNotSatisfiable { size } = self.0 {
            res.headers_mut().insert(
                header::CONTENT_RANGE,
                format!("bytes */{size}").parse().unwrap(),
            );
        }
        res
    }
}
