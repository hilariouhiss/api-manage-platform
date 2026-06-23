//! Database module.
//!
//! Provides PostgreSQL connection pool initialization and shutdown
//! via [`init_pool`] and [`close_pool`].

use std::time::Duration;

use anyhow::{Context, ensure};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::config::DatabaseConfig;

/// Initialize a PostgreSQL connection pool.
///
/// Validates that `min_connections <= max_connections`, then immediately
/// attempts to establish at least one connection. Returns an error on
/// failure; the caller is responsible for logging and exiting.
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

/// Close a PostgreSQL connection pool.
///
/// `PgPool::close()` is infallible (returns `()`). It immediately wakes
/// all tasks waiting for a connection; subsequent `acquire` calls return
/// `PoolClosed`.
pub async fn close_pool(pool: &PgPool) {
    pool.close().await;
}
