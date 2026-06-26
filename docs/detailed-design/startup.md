# 应用入口与启动流程

## 启动序列

[src/main.rs](../../src/main.rs) 按以下顺序编排启动：

```
config::load()                    → 多层配置加载
shutdown::init_tracing(&cfg)      → 日志系统初始化（双输出：控制台 + 文件）
db::init_pool(&cfg)               → PostgreSQL 连接池
run_migrations(&pool)             → sqlx::migrate! 执行迁移
valkey::init_pool(&cfg)           → Valkey 连接池
ShutdownRegistry::new()           → 注册资源清理（LIFO 逆序：tracing → db → valkey）
AppState { config, db, valkey }   → 聚合状态
axum::Router::new()               → 路由挂载
tokio::TcpListener::bind()        → 端口绑定
shutdown::run(listener, app, registry, config) → 启动 + 优雅关闭
```

**连接失败策略**：`.inspect_err(|e| tracing::error!(...))` 记录日志后 `?` 传播错误退出，fail-fast。

## 状态注入

`AppState`（[src/state.rs](../../src/state.rs)）聚合三类共享状态：

```rust
#[derive(Clone, FromRef)]
pub struct AppState {
    pub config: SharedConfig,   // Arc<AppConfig>
    pub db: PgPool,             // PostgreSQL 连接池
    pub valkey: ValkeyPool,     // fred::clients::Pool
}
```

`#[derive(FromRef)]` 使 handler 可按需提取子状态，无需持有整个 `AppState`：

```rust
async fn handler(
    State(db): State<PgPool>,
    State(config): State<SharedConfig>,
    AuthUser(claims): AuthUser,
) -> ...
```

顶层 `Router::with_state(state)` 注入一次即可。

## 公共导出

[src/lib.rs](../../src/lib.rs) 通过 `pub mod` 重新导出所有模块。集成测试通过 `use api_manage_platform::*` 访问所有公共类型，使用 `tower::ServiceExt::oneshot()` 无需启动服务器即可测试路由。

## 数据库迁移

`run_migrations()` 在启动时调用 `sqlx::migrate!("./migrations")` 自动执行所有未应用的 SQL 迁移文件，确保数据库 schema 与代码同步。
