use std::time::Duration;

use anyhow::{Context, ensure};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::DatabaseConfig;

/// 初始化 PostgreSQL 连接池
///
/// 校验 `min_connections <= max_connections`，然后立即尝试建立至少一个连接。
/// 失败时返回错误，由调用方记录日志并退出。
pub async fn init_pool(cfg: &DatabaseConfig) -> anyhow::Result<PgPool> {
    ensure!(
        cfg.min_connections <= cfg.max_connections,
        "database.min_connections ({}) must be <= database.max_connections ({})",
        cfg.min_connections,
        cfg.max_connections
    );

    PgPoolOptions::new()
        .max_connections(cfg.max_connections)
        .min_connections(cfg.min_connections)
        .acquire_timeout(Duration::from_secs(cfg.acquire_timeout_seconds))
        .idle_timeout(Duration::from_secs(cfg.idle_timeout_minutes * 60))
        .connect(&cfg.url)
        .await
        .context("failed to connect to database")
}
