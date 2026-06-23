use axum::extract::FromRef;
use sqlx::PgPool;

use crate::config::SharedConfig;
use crate::valkey::ValkeyPool;

/// 应用共享状态
///
/// 通过 `#[derive(FromRef)]` 支持 handler 按需提取子状态：
///
/// ```rust,ignore
/// async fn handler(State(db): State<PgPool>) { ... }
/// async fn other(State(config): State<SharedConfig>) { ... }
/// ```
#[derive(Clone, FromRef)]
pub struct AppState {
    pub config: SharedConfig,
    pub db: PgPool,
    pub valkey: ValkeyPool,
}
