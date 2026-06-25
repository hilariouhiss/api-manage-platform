use api_manage_platform::config;
use api_manage_platform::db;
use api_manage_platform::routes;
use api_manage_platform::shutdown;
use api_manage_platform::valkey;

use anyhow::Context;
use axum::routing::{delete, get, post, put};
use sqlx::PgPool;

use api_manage_platform::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // --- Configuration ---
    let app_config = config::load().context("failed to load config")?;
    let shared_config = config::SharedConfig::new(app_config);

    // --- Tracing ---
    let _tracing_guard = shutdown::init_tracing(&shared_config.logging)?;

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

    // --- Run migrations ---
    run_migrations(&db_pool).await?;

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
        // Health check
        .route("/api/v1/hello", get(routes::hello::hello))
        // Auth
        .route("/api/v1/auth/register", post(routes::auth::register))
        .route("/api/v1/auth/login", post(routes::auth::login))
        // Current user
        .route("/api/v1/users/me", get(routes::users::me))
        .route("/api/v1/users/me", put(routes::users::update_me))
        // User CRUD
        .route("/api/v1/users", get(routes::users::list_users))
        .route("/api/v1/users", post(routes::users::create_user))
        .route("/api/v1/users/{id}", get(routes::users::get_user))
        .route("/api/v1/users/{id}", put(routes::users::update_user))
        .route("/api/v1/users/{id}", delete(routes::users::delete_user))
        // Roles & Permissions
        .route("/api/v1/roles", get(routes::roles::list_roles))
        .route("/api/v1/roles/{id}", get(routes::roles::get_role))
        .route("/api/v1/permissions", get(routes::permissions::list_permissions))
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

/// Run SQL migrations at startup.
async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    tracing::info!("running database migrations");
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("failed to run database migrations")?;
    tracing::info!("database migrations complete");
    Ok(())
}
