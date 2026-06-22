use std::time::Duration;

use anyhow::{Context, ensure};
use fred::prelude::*;

use crate::config::ValkeyConfig;

/// Valkey 连接池类型别名
pub type ValkeyPool = fred::clients::Pool;

/// 初始化 Valkey 连接池
///
/// 从 URL 解析 `Config`，使用 `Builder::from_config()` 创建 Builder，
/// 通过 `with_connection_config` 设置超时参数，然后 `build_pool` 构建连接池。
/// `pool.init()` 立即初始化所有连接，随后 `PING` 做连通性校验。
/// 失败时返回错误，由调用方记录日志并退出。
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

    // 连通性校验
    let _: String = pool.ping(None).await.context("failed to ping valkey")?;

    Ok(pool)
}
