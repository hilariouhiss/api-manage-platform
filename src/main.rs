mod config;
mod db;
mod response;
mod routes;
mod state;
mod valkey;

use anyhow::Context;

use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化 tracing（在加载配置前输出日志）
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // 1. 加载配置
    let app_config = config::load().context("failed to load config")?;
    let shared_config = config::SharedConfig::new(app_config);

    // 2. 初始化数据库连接池
    let db_pool = db::init_pool(&shared_config.database)
        .await
        .inspect_err(|e| tracing::error!("failed to connect to database: {:#}", e))
        .context("failed to connect to database")?;
    tracing::info!(
        "database connected (pool: {}..={})",
        shared_config.database.min_connections,
        shared_config.database.max_connections
    );

    // 3. 初始化 Valkey 连接池
    let valkey_pool = valkey::init_pool(&shared_config.valkey)
        .await
        .inspect_err(|e| tracing::error!("failed to connect to valkey: {:#}", e))
        .context("failed to connect to valkey")?;
    tracing::info!("valkey connected (pool size: {})", shared_config.valkey.pool_size);

    // 4. 提取绑定地址（在构建 state 之前，避免后续 move 问题）
    let addr = format!(
        "{}:{}",
        shared_config.server.host,
        shared_config.server.port
    );

    // 5. 构建 State
    let state = AppState {
        config: shared_config,
        db: db_pool,
        valkey: valkey_pool,
    };

    // 6. 路由
    let app = axum::Router::new()
        .route("/api/v1/hello", axum::routing::get(routes::hello::hello))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind to {}", addr))?;
    tracing::info!("server starting at http://{}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}
