//! Only the device-to-device leg benefits from compressing larger JSON replies.
use axum::{
    Router,
    http::{Extensions, HeaderMap, StatusCode, Version, header},
};
use tower_http::compression::{
    CompressionLayer, CompressionLevel,
    predicate::{Predicate, SizeAbove},
};

pub(super) fn compress_json<S: Clone + Send + Sync + 'static>(routes: Router<S>) -> Router<S> {
    let json = |_: StatusCode, _: Version, h: &HeaderMap, _: &Extensions| {
        h.get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.split(';').next() == Some("application/json"))
            && !h.contains_key(header::CONTENT_DISPOSITION)
    };
    routes.layer(
        CompressionLayer::new()
            .gzip(true)
            .quality(CompressionLevel::Fastest)
            .compress_when(SizeAbove::new(1024).and(json)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
        routing::get,
    };
    use std::io::Read;
    use tower::ServiceExt;

    #[tokio::test]
    async fn remote_json_shrinks_and_sse_is_not_buffered() {
        let fixture = serde_json::json!({"items":(0..64).map(|n|serde_json::json!({
            "id":n,"role":"assistant","type":"message","text":format!("Message {n}: build completed.")
        })).collect::<Vec<_>>()}).to_string();
        let payload = fixture.clone();
        let routes =
            compress_json(
                Router::new()
                    .route(
                        "/history",
                        get(move || async move {
                            ([(header::CONTENT_TYPE, "application/json")], payload)
                        }),
                    )
                    .route(
                        "/events",
                        get(|| async {
                            (
                                [(header::CONTENT_TYPE, "text/event-stream")],
                                ":tick\n\n".repeat(500),
                            )
                        }),
                    ),
            );
        let req = |uri| {
            Request::builder()
                .uri(uri)
                .header(header::ACCEPT_ENCODING, "gzip")
                .body(Body::empty())
                .unwrap()
        };
        let result = routes.clone().oneshot(req("/history")).await.unwrap();
        assert_eq!(result.headers()[header::CONTENT_ENCODING], "gzip");
        let bytes = to_bytes(result.into_body(), 64 * 1024).await.unwrap();
        let mut restored = String::new();
        flate2::read::GzDecoder::new(&bytes[..])
            .read_to_string(&mut restored)
            .unwrap();
        assert_eq!(restored, fixture);
        assert!(bytes.len() < fixture.len() / 2);
        eprintln!(
            "synthetic remote JSON: {} -> {} bytes",
            fixture.len(),
            bytes.len()
        );
        assert!(
            !routes
                .oneshot(req("/events"))
                .await
                .unwrap()
                .headers()
                .contains_key(header::CONTENT_ENCODING)
        );
    }
}
