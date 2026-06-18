mod config;
mod response;
mod routes;

use axum::{routing::get, Router};

#[tokio::main]
async fn main() {
    // 构建路由
    let app = Router::new().route("/api/v1/hello", get(routes::hello::hello));

    // 启动服务
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("🚀 Server running at http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}
