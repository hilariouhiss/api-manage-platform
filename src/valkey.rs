//! Valkey (Redis) module.
//!
//! Provides Valkey connection pool initialization and shutdown
//! via [`init_pool`] and [`close_pool`].

use std::time::Duration;

use anyhow::{Context, ensure};
use fred::prelude::*;

use crate::config::ValkeyConfig;

/// Valkey connection pool type alias.
pub type ValkeyPool = fred::clients::Pool;

/// Initialize a Valkey connection pool.
///
/// Builds a [`Config`] from the URL, creates a [`Builder`] via
/// `Builder::from_config()`, applies timeout settings through
/// `with_connection_config`, then calls `build_pool`. After
/// `pool.init()` initializes all connections, a `PING` command
/// verifies connectivity. Returns an error on failure; the caller
/// is responsible for logging and exiting.
pub async fn init_pool(cfg: &ValkeyConfig) -> anyhow::Result<ValkeyPool> {
    ensure!(
        cfg.pool_size > 0,
        "valkey.pool_size must be greater than 0, got {}",
        cfg.pool_size
    );

    let config = Config::from_url(&cfg.url).context("invalid valkey url")?;
    let mut builder = Builder::from_config(config);

    builder.with_connection_config(|conn| {
        conn.connection_timeout = Duration::from_secs(cfg.connect_timeout_seconds);
        conn.internal_command_timeout = Duration::from_secs(cfg.internal_command_timeout_seconds);
    });

    let pool = builder
        .build_pool(cfg.pool_size as usize)
        .context("failed to build valkey pool")?;

    pool.init()
        .await
        .context("failed to initialize valkey pool")?;

    // Connectivity check
    let _: String = pool.ping(None).await.context("failed to ping valkey")?;

    Ok(pool)
}

/// Close a Valkey connection pool.
///
/// Sends a `QUIT` command to notify the server. The future returned
/// by `fred::Pool::quit()` resolves once all connections have written
/// their commands to the socket.
pub async fn close_pool(pool: &ValkeyPool) -> anyhow::Result<()> {
    pool.quit().await.context("failed to quit valkey pool")?;
    Ok(())
}
