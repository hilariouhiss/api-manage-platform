use axum::response::IntoResponse;
use serde::Serialize;

/// 统一 API 响应结构体
#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub code: u16,
    pub message: String,
    pub data: Option<T>,
}

impl<T: Serialize> ApiResponse<T> {
    /// 创建一个成功的响应（带数据）
    pub fn success(message: impl Into<String>, data: T) -> Self {
        Self {
            code: 200,
            message: message.into(),
            data: Some(data),
        }
    }

    /// 创建一个仅含消息的成功响应（code=200, data=None）
    /// 适用于 health check、简单确认等场景
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            code: 200,
            message: message.into(),
            data: None,
        }
    }

    /// 创建一个失败的响应
    pub fn failure(code: u16, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// 创建一个仅包含消息的响应（data 为 None）
    pub fn message(code: u16, message: impl Into<String>) -> Self {
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
