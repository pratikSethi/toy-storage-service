use axum::http::StatusCode;
use axum::response::IntoResponse;
use ts2_core::Error;
use ts2_frontdoor::error::ApiError;

/// Backing-service failures must not surface as 500.
#[test]
fn backing_service_errors_map_to_their_own_statuses() {
    let table = [
        (
            Error::Unavailable("node down".into()),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (Error::Timeout("5s".into()), StatusCode::GATEWAY_TIMEOUT),
        (Error::DiskFull, StatusCode::INSUFFICIENT_STORAGE),
        (
            Error::ChunkNotFound("ab".into()),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (
            Error::Corrupt("ab".into()),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    ];
    for (err, want) in table {
        let label = format!("{err:?}");
        assert_eq!(ApiError(err).into_response().status(), want, "{label}");
    }
}
