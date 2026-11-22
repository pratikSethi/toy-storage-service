use axum::extract::{MatchedPath, Request};
use axum::http::HeaderName;
use axum::middleware::Next;
use axum::response::Response;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter, UpDownCounter};
use std::sync::Arc;
use std::time::Instant;

const REQUEST_ID: HeaderName = HeaderName::from_static("x-ts2-request-id");
const LATENCY_BUCKETS: [f64; 12] = [0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

pub struct Metrics {
    requests: Counter<u64>,
    duration: Histogram<f64>,
    bytes_in: Counter<u64>,
    bytes_out: Counter<u64>,
    inflight: UpDownCounter<i64>,
}

impl Metrics {
    pub fn new(meter: &Meter) -> Arc<Self> {
        Arc::new(Self {
            requests: meter.u64_counter("ts2_http_requests_total").build(),
            duration: meter
                .f64_histogram("ts2_http_request_duration_seconds")
                .with_boundaries(LATENCY_BUCKETS.to_vec())
                .build(),
            bytes_in: meter.u64_counter("ts2_http_request_bytes_total").build(),
            bytes_out: meter.u64_counter("ts2_http_response_bytes_total").build(),
            inflight: meter.i64_up_down_counter("ts2_http_inflight_requests").build(),
        })
    }
}

fn status_class(code: u16) -> &'static str {
    match code / 100 {
        1 => "1xx",
        2 => "2xx",
        3 => "3xx",
        4 => "4xx",
        _ => "5xx",
    }
}

/// Records RED metrics and emits one JSON log line per request.
/// shortcut: duration is time to response headers and byte counts come from Content-Length (or an exact body size),
/// so a streamed body without a length is not counted. Upgrade by wrapping the body when that matters.
pub async fn track(metrics: Arc<Metrics>, req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    // The route template keeps label cardinality bounded; unmatched paths collapse into one label.
    let route = req.extensions().get::<MatchedPath>().map_or("unmatched", |p| p.as_str()).to_owned();
    let request_id = req
        .headers()
        .get(&REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes_in = req
        .headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
        .unwrap_or(0);

    metrics.inflight.add(1, &[]);
    let start = Instant::now();
    let res = next.run(req).await;
    let elapsed = start.elapsed();
    metrics.inflight.add(-1, &[]);

    let status = res.status().as_u16();
    let bytes_out = axum::body::HttpBody::size_hint(res.body()).exact().unwrap_or(0);
    let route_labels = [KeyValue::new("method", method.clone()), KeyValue::new("route", route.clone())];
    metrics.duration.record(elapsed.as_secs_f64(), &route_labels);
    metrics.requests.add(
        1,
        &[
            KeyValue::new("method", method.clone()),
            KeyValue::new("route", route.clone()),
            KeyValue::new("status_class", status_class(status)),
        ],
    );
    metrics.bytes_in.add(bytes_in, &route_labels);
    metrics.bytes_out.add(bytes_out, &route_labels);

    tracing::info!(
        method = %method,
        route = %route,
        status,
        duration_ms = elapsed.as_secs_f64() * 1000.0,
        bytes = bytes_in + bytes_out,
        request_id = %request_id,
        "request"
    );
    res
}

/// OTLP/HTTP exporter driven by OTEL_EXPORTER_OTLP_ENDPOINT. Returns None when it is unset.
pub fn init_otlp() -> Option<opentelemetry_sdk::metrics::SdkMeterProvider> {
    std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").filter(|v| !v.is_empty())?;
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_http()
        .build()
        .map_err(|e| tracing::error!(error = %e, "otlp exporter disabled"))
        .ok()?;
    let provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
        .with_periodic_exporter(exporter)
        .build();
    opentelemetry::global::set_meter_provider(provider.clone());
    Some(provider)
}
