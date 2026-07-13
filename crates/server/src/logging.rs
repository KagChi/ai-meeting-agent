//! Request logging middleware

use axum::{extract::Request, middleware::Next, response::Response};
use std::time::Instant;

/// Middleware that logs all incoming HTTP requests and their responses
pub async fn log_request(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let start = Instant::now();

    log::info!("Request: {} {}", method, uri);

    let response = next.run(req).await;

    let duration = start.elapsed();
    let status = response.status();

    log::info!(
        "Response: {} {} - {} ({:.2}ms)",
        method,
        uri,
        status.as_u16(),
        duration.as_secs_f64() * 1000.0
    );

    response
}
