use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use api_manage_platform::config;
use api_manage_platform::db;
use api_manage_platform::health_check;
use api_manage_platform::state::AppState;
use api_manage_platform::valkey;

/// Build a test router for the health check endpoint with real state.
async fn test_app() -> axum::Router {
    let app_config = config::load().expect("failed to load config");
    let shared_config = config::SharedConfig::new(app_config);

    let db_pool = db::init_pool(&shared_config.database)
        .await
        .expect("failed to init db pool");

    let valkey_pool = valkey::init_pool(&shared_config.valkey)
        .await
        .expect("failed to init valkey pool");

    let state = AppState {
        config: shared_config,
        db: db_pool,
        valkey: valkey_pool,
    };

    axum::Router::new()
        .route("/api/v1/health", axum::routing::get(health_check))
        .with_state(state)
}

#[tokio::test]
async fn test_health_returns_200() {
    let app = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_health_has_json_content_type() {
    let app = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
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
async fn test_health_response_body() {
    let app = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
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
    assert_eq!(body["message"], "health check passed");

    let data = &body["data"];
    assert_eq!(data["status"], "healthy");
    assert_eq!(data["database"], "connected");
    assert_eq!(data["valkey"], "connected");
}

#[tokio::test]
async fn test_health_post_returns_405() {
    let app = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn test_unknown_route_returns_404() {
    let app = test_app().await;

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
    let app = test_app().await;

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
