mod config;
mod response;
mod routes;

use anyhow::Context;
use axum::{Router, routing::get};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 加载配置
    let app_config = config::load().context("failed to load config")?;
    let shared_config = config::SharedConfig::new(app_config);

    // 提前取出绑定地址（with_state 会 move shared_config）
    let addr = format!(
        "{}:{}",
        shared_config.server.host, shared_config.server.port
    );

    // 构建路由并传入配置
    let app = Router::new()
        .route("/api/v1/hello", get(routes::hello::hello))
        .with_state(shared_config);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind to {}", addr))?;
    println!("🚀 Server running at http://{}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}
