use crate::response::ApiResponse;

/// GET /api/v1/hello
pub async fn hello() -> ApiResponse<()> {
    ApiResponse::ok("Hello World!")
}
