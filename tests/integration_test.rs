use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use api_manage_platform::hello;

/// Build a minimal test router containing only the hello endpoint.
/// No database or Valkey state is required — the handler is stateless.
fn test_app() -> axum::Router {
    axum::Router::new().route("/api/v1/hello", axum::routing::get(hello))
}

#[tokio::test]
async fn test_hello_returns_200() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_hello_has_json_content_type() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let content_type = response
        .headers()
        .get("content-type")
        .expect("content-type header missing")
        .to_str()
        .unwrap();

    assert!(
        content_type.starts_with("application/json"),
        "expected application/json, got: {}",
        content_type
    );
}

#[tokio::test]
async fn test_hello_response_body() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body["code"], 200);
    assert_eq!(body["message"], "Hello World!");
    assert_eq!(body["data"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_hello_post_returns_405() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn test_unknown_route_returns_404() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/unknown")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_root_returns_404() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
