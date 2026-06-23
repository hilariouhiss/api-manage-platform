use api_manage_platform::config;
use api_manage_platform::db;
use api_manage_platform::routes;
use api_manage_platform::shutdown;
use api_manage_platform::valkey;

use anyhow::Context;

use api_manage_platform::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // --- Configuration ---
    let app_config = config::load().context("failed to load config")?;
    let shared_config = config::SharedConfig::new(app_config);

    // --- Tracing ---
    let _tracing_guard = shutdown::init_tracing()?;

    // --- Database ---
    let db_pool = db::init_pool(&shared_config.database)
        .await
        .inspect_err(|e| tracing::error!("failed to connect to database: {:#}", e))
        .context("failed to connect to database")?;
    tracing::info!(
        "database connected (pool: {}..={})",
        shared_config.database.min_connections,
        shared_config.database.max_connections
    );

    // --- Valkey ---
    let valkey_pool = valkey::init_pool(&shared_config.valkey)
        .await
        .inspect_err(|e| tracing::error!("failed to connect to valkey: {:#}", e))
        .context("failed to connect to valkey")?;
    tracing::info!(
        "valkey connected (pool size: {})",
        shared_config.valkey.pool_size
    );

    // --- Bind address ---
    let addr = format!(
        "{}:{}",
        shared_config.server.host, shared_config.server.port
    );

    // --- Shutdown registry ---
    let db_cleanup = db_pool.clone();
    let valkey_cleanup = valkey_pool.clone();

    let mut registry = shutdown::ShutdownRegistry::new();
    // Register in reverse shutdown order: tracing first → cleaned up last
    registry.register("tracing", Box::new(move || {
        Box::pin(async move {
            drop(_tracing_guard);
            Ok(())
        })
    }));
    registry.register("database", Box::new(move || {
        Box::pin(async move {
            db::close_pool(&db_cleanup).await;
            Ok(())
        })
    }));
    registry.register("valkey", Box::new(move || {
        Box::pin(async move { valkey::close_pool(&valkey_cleanup).await })
    }));

    // --- Application state ---
    let state = AppState {
        config: shared_config,
        db: db_pool,
        valkey: valkey_pool,
    };

    // --- Routes ---
    let app = axum::Router::new()
        .route("/api/v1/hello", axum::routing::get(routes::hello::hello))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind to {}", addr))?;
    tracing::info!("server starting at http://{}", addr);

    // --- Start server (with graceful shutdown) ---
    shutdown::run(
        listener,
        app,
        registry,
        shutdown::GracefulShutdownConfig::default(),
    )
    .await
}
