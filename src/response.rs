//! Response module.
//!
//! Provides a unified [`ApiResponse<T>`] struct for API responses,
//! together with convenience constructors and an [`axum::response::IntoResponse`]
//! implementation.

use axum::response::IntoResponse;
use serde::Serialize;

/// Unified API response struct.
#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub code: u16,
    pub message: String,
    pub data: Option<T>,
}

impl<T: Serialize> ApiResponse<T> {
    /// Create a success response without data.
    pub fn ok() -> Self {
        Self {
            code: 200,
            message: "success".to_string(),
            data: None,
        }
    }

    /// Create a success response with optional data.
    pub fn success(message: impl Into<String>, data: Option<T>) -> Self {
        Self {
            code: 200,
            message: message.into(),
            data,
        }
    }

    /// Create a failure response without data.
    pub fn failure(code: u16, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> axum::response::Response {
        axum::Json(self).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    // --- ok() ---

    #[test]
    fn test_ok_returns_200_with_success_message() {
        let resp: ApiResponse<()> = ApiResponse::ok();
        assert_eq!(resp.code, 200);
        assert_eq!(resp.message, "success");
        assert!(resp.data.is_none());
    }

    // --- success() ---

    #[test]
    fn test_success_with_data() {
        let resp = ApiResponse::success("Operation complete", Some(42));
        assert_eq!(resp.code, 200);
        assert_eq!(resp.message, "Operation complete");
        assert_eq!(resp.data, Some(42));
    }

    #[test]
    fn test_success_without_data() {
        let resp: ApiResponse<i32> = ApiResponse::success("OK", None);
        assert_eq!(resp.code, 200);
        assert_eq!(resp.message, "OK");
        assert!(resp.data.is_none());
    }

    #[test]
    fn test_success_message_string_conversion() {
        // &str → String conversion via impl Into<String>
        let resp: ApiResponse<()> = ApiResponse::success("static str", None);
        assert_eq!(resp.message, "static str");
    }

    // --- failure() ---

    #[test]
    fn test_failure_returns_custom_code_and_message() {
        let resp: ApiResponse<()> = ApiResponse::failure(404, "Not Found");
        assert_eq!(resp.code, 404);
        assert_eq!(resp.message, "Not Found");
        assert!(resp.data.is_none());
    }

    #[test]
    fn test_failure_with_500() {
        let resp: ApiResponse<()> = ApiResponse::failure(500, "Internal Server Error");
        assert_eq!(resp.code, 500);
        assert_eq!(resp.message, "Internal Server Error");
    }

    // --- IntoResponse ---

    #[test]
    fn test_into_response_returns_json_content_type() {
        let resp = ApiResponse::<()>::ok();
        let axum_resp = resp.into_response();
        let content_type = axum_resp
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

    #[test]
    fn test_into_response_serializes_correctly() {
        let body = ApiResponse::success("Hi", Some("there"));
        let axum_resp = body.into_response();

        // axum::Json serializes the body; verify status code
        assert_eq!(axum_resp.status(), 200);
    }
}
