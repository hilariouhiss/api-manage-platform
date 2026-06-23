//! Hello route module.
//!
//! A simple health-check / hello-world endpoint.

use crate::response::ApiResponse;

/// `GET /api/v1/hello`
///
/// Returns a plain success response with "Hello World!".
pub async fn hello() -> ApiResponse<()> {
    ApiResponse::success("Hello World!", None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hello_returns_hello_world() {
        let resp = hello().await;
        assert_eq!(resp.code, 200);
        assert_eq!(resp.message, "Hello World!");
        assert!(resp.data.is_none());
    }
}
