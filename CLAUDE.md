# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

一个基于 Rust 的 API 管理平台，使用 axum 作为 Web 框架，tokio 作为异步运行时。项目处于早期开发阶段，基础设施和依赖已就绪。

## 构建与开发命令

```bash
# 构建项目
cargo build

# 构建（release 模式）
cargo build --release

# 运行开发服务器（绑定 127.0.0.1:3000）
cargo run

# 运行测试
cargo test

# 运行单个测试
cargo test <test_name>

# 检查代码（编译但不生成二进制文件，速度快）
cargo check

# 代码格式化
cargo fmt

# Lint 检查
cargo clippy

# 运行数据库迁移（使用 sqlx-cli）
sqlx migrate run --database-url <DATABASE_URL>
```

## 技术栈

| 用途 | 库 | 关键版本/特性 |
| ------ | ----- | -------------- |
| Web 框架 | `axum` 0.8 | |
| 异步运行时 | `tokio` 1.52 | `features = ["full"]` |
| 中间件 | `tower` 0.5, `tower-http` 0.7 | CORS, trace |
| 序列化 | `serde` 1.0, `serde_json` 1.0 | derive macro |
| HTTP 客户端 | `reqwest` 0.13 | json feature |
| 数据库 | `sqlx` 0.9 | PostgreSQL, rustls, uuid, chrono, migrate |
| 缓存/Redis | `fred` 10.1 | rustls。⚠ 初始化池必须用 `Config::from_url(&url)` + `Builder::from_config(config)` 传入地址；`Builder::default_centralized()` 会忽略 URL |
| 认证 | `argon2` 0.5, `jsonwebtoken` 10.4 | |
| 校验 | `validator` 0.20 | derive macro |
| UUID | `uuid` 1.23 | v7, serde |
| 日期时间 | `chrono` 0.4 | serde |
| 配置管理 | `config` 0.15, `dotenvy` 0.15 | |
| 日志 | `tracing` 0.1, `tracing-subscriber` 0.3 | env-filter, json |
| 错误处理 | `thiserror` 2.0, `anyhow` 1.0 | |

## 代码架构

```text
src/
├── main.rs          # 入口点：配置加载、tracing、连接池、关闭注册、路由、shutdown::run() 启动
├── lib.rs           # 库 crate 根（api_manage_platform），重新导出所有模块供集成测试使用
├── state.rs         # AppState 聚合状态（FromRef derive）
├── db.rs            # PostgreSQL 连接池初始化（init_pool / close_pool）
├── valkey.rs        # Valkey 连接池初始化（init_pool / close_pool）+ ValkeyPool 类型别名
├── response.rs      # 统一 API 响应封装 ApiResponse<T>
├── config/
│   ├── mod.rs       # AppConfig 聚合结构体 + SharedConfig 类型别名
│   └── loader.rs    # 多源配置加载（default.toml → {APP_ENV}.toml → env）
├── shutdown/
│   ├── mod.rs       # init_tracing()、GracefulShutdownConfig、两级关闭的 run()
│   ├── registry.rs  # ShutdownRegistry — 资源按 LIFO 顺序清理，错误隔离
│   └── signals.rs   # 跨平台信号监听（SIGINT、SIGTERM、SIGHUP）
└── routes/
    ├── mod.rs       # 路由模块声明
    └── hello.rs     # 示例端点 GET /api/v1/hello

tests/
└── integration_test.rs  # 使用 tower::ServiceExt::oneshot() 的无服务器路由测试

config/
├── default.toml      # 默认配置（不含 secrets）
└── development.toml  # 可选环境覆盖（由 APP_ENV 激活）

.env.example          # 环境变量模板（含注释）
.env                  # 本地开发（gitignored）
```

### 核心模式

- **状态注入**：使用 `AppState` + `#[derive(FromRef)]` 作为 axum 单一顶层 State。Handler 可按需提取子状态：`State<PgPool>`、`State<SharedConfig>`、`State<ValkeyPool>`。`axum` 需要启用 `macros` feature。
- **优雅关闭（两级）**：信号到达后，axum 开始排空进行中的请求（有 `drain_timeout` 保护，默认 10 秒）。资源按 LIFO 顺序通过 `ShutdownRegistry` 清理。无论服务器如何退出，清理始终运行。
- **配置注入**：`config::load()` → `SharedConfig`（`Arc<AppConfig>`）→ 存入 `AppState.config`。
- **集成测试**：使用 `tower::ServiceExt::oneshot()` 测试 axum 路由——无需运行服务器。导入来自 `api_manage_platform` crate（通过 `lib.rs` 公开）。
- **统一响应格式**：所有 API 端点返回 `ApiResponse<T>`（定义在 `src/response.rs`），包含 `code`、`message`、`data` 字段。提供了 `success()`、`ok()`、`failure()`、`message()` 工厂方法。`ApiResponse` 实现了 `IntoResponse`，自动序列化为 JSON。
- **环境变量格式**：`APP__` 前缀 + `__` 分隔符 → 嵌套结构体。如 `APP__SERVER__PORT=8080` → `server.port`。
- **配置加载顺序**：`default.toml` → `{APP_ENV}.toml`（可选）→ 环境变量 `APP__*`（最高优先级）。
- **Rust edition 2024**：项目使用 Rust 2024 edition。`std::env::set_var` / `remove_var` 在此 edition 中为 `unsafe`。

### 依赖规划

依赖列表表明项目规划支持：

- **数据库操作**（sqlx + PostgreSQL，含迁移支持）
- **Redis 缓存**（fred）
- **用户认证**（argon2 密码哈希 + JWT token）
- **配置管理**（config + dotenvy）
- **请求校验**（validator）
- **分布式追踪/日志**（tracing + tracing-subscriber，JSON 格式输出）
- **OpenAPI 文档**（utoipa / utoipa-swagger-ui 已注释，后续启用）

## 开发约定

- **启动流程**：config::load() → shutdown::init_tracing() → db::init_pool() → valkey::init_pool() → ShutdownRegistry → AppState → 路由 → shutdown::run(listener, app, registry, config)。连接失败使用 `.inspect_err(\|e\| tracing::error!(…))` 记录日志后退出，fail-fast 不延迟连接。
- **首次运行**：`cp .env.example .env` → 设置 `APP_ENV` 和 `APP__JWT__SECRET` → `cargo run`
- **环境变量测试**：`cargo test -- --test-threads=1`（`set_var`/`remove_var` 在 Rust 2024 中为 `unsafe`，测试必须串行）
- **Secrets**：`jwt.secret` 不写入 TOML，必须通过 `APP__JWT__SECRET` 注入。`#[serde(default)]` + validator `length(min=32)` + loader 显式检查三重保证
