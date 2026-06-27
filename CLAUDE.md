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

# Docker Compose 启动（app + PostgreSQL + Valkey）
docker compose --env-file .env.compose up -d

# Docker Compose 查看日志
docker compose logs -f app

# Docker Compose 停止（保留数据卷）
docker compose down
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
| 认证 | `argon2` 0.5, `jsonwebtoken` 10.4 | ⚠ 必须 `features = ["aws_lc_rs"]`，否则编译通过但 JWT 操作 panic |
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
├── errors.rs        # AppError 枚举 + IntoResponse + From<sqlx::Error> (23505→中文消息)
├── response.rs      # 统一 API 响应封装 ApiResponse<T>
├── auth/
│   ├── mod.rs       # 认证模块声明
│   ├── jwt.rs       # JWT 签发/验证 (HS256)
│   └── password.rs  # argon2 密码哈希/校验
├── middleware/
│   ├── mod.rs       # 中间件模块声明
│   └── auth.rs      # AuthUser FromRequestParts 提取器
├── models/
│   ├── mod.rs       # 模型模块声明
│   ├── auth.rs      # JwtClaims, RegisterPayload, LoginPayload, TokenResponse
│   ├── user.rs      # UserRow, UserResponse, UserListItem, 分页类型, 请求 DTO
│   └── role.rs      # RoleRow, PermissionRow, RoleWithPermissions
├── config/
│   ├── mod.rs       # AppConfig 聚合结构体 + SharedConfig 类型别名
│   └── loader.rs    # 多源配置加载（default.toml → {APP_ENV}.toml → env）
├── shutdown/
│   ├── mod.rs       # init_tracing()、GracefulShutdownConfig、两级关闭的 run()
│   ├── registry.rs  # ShutdownRegistry — 资源按 LIFO 顺序清理，错误隔离
│   └── signals.rs   # 跨平台信号监听（SIGINT、SIGTERM、SIGHUP）
└── routes/
    ├── mod.rs       # 路由模块声明
    ├── health.rs    # GET /api/v1/health — Docker 兼容健康检查（验证 DB + Valkey 连通性）
    ├── auth.rs      # POST register, POST login
    ├── users.rs     # CRUD /users, /users/me, 分页, check_manage_scope
    ├── roles.rs     # GET /roles, GET /roles/:id
    └── permissions.rs # GET /permissions

tests/
└── integration_test.rs  # 使用 tower::ServiceExt::oneshot() 的无服务器路由测试

config/
├── default.toml      # 默认配置（不含 secrets）
└── development.toml  # 可选环境覆盖（由 APP_ENV 激活）

.env.example          # 环境变量模板（含注释）
.env                  # 本地开发（gitignored）
.env.compose          # Docker Compose 密钥（gitignored）
```

### 核心模式

- **RBAC 鉴权**：roles + permissions 扁平化嵌入 JWT Claims，运行时 `require_permission("resource:action")` 零查库。权限格式 `"{resource}:{action}"`。三级管理范围：system_admin（全部）> admin（仅 user 角色用户）> user（仅自身）。
- **防 TOCTOU**：管理范围检查 (`check_manage_scope`) + 角色分配验证 + 唯一性检查全在同一事务内执行。
- **数据库错误映射**：`From<sqlx::Error> for AppError` 对 PG 23505 按约束名映射中文消息，不泄露表/列名。
- **软删除**：用户删除设 `deleted_at` + `deleted_by`；唯一索引带 `WHERE deleted_at IS NULL`；所有查询过滤软删除行。
- **游标分页**：keyset `(created_at DESC, id DESC)`，游标 base64url 编码，取 limit+1 行判断 has_more。
- **状态注入**：使用 `AppState` + `#[derive(FromRef)]` 作为 axum 单一顶层 State。Handler 可按需提取子状态：`State<PgPool>`、`State<SharedConfig>`、`State<ValkeyPool>`。`axum` 需要启用 `macros` feature。
- **优雅关闭（两级）**：信号到达后，axum 开始排空进行中的请求（有 `drain_timeout` 保护，默认 10 秒）。资源按 LIFO 顺序通过 `ShutdownRegistry` 清理。无论服务器如何退出，清理始终运行。
- **配置注入**：`config::load()` → `SharedConfig`（`Arc<AppConfig>`）→ 存入 `AppState.config`。
- **集成测试**：使用 `tower::ServiceExt::oneshot()` 测试 axum 路由——无需运行服务器。导入来自 `api_manage_platform` crate（通过 `lib.rs` 公开）。
- **统一响应格式**：所有 API 端点返回 `ApiResponse<T>`（定义在 `src/response.rs`），包含 `code`、`message`、`data` 字段。提供了 `success()`、`ok()`、`failure()`、`message()` 工厂方法。健康检查等需要自定义 HTTP 状态码的端点可直接构造 `ApiResponse`，手动设置 `code`（如 `StatusCode::SERVICE_UNAVAILABLE`）。`ApiResponse` 实现了 `IntoResponse`，自动序列化为 JSON。
- **环境变量格式**：`APP__` 前缀 + `__` 分隔符 → 嵌套结构体。如 `APP__SERVER__PORT=8080` → `server.port`。
- **配置加载顺序**：`default.toml` → `{APP_ENV}.toml`（可选）→ 环境变量 `APP__*`（最高优先级）。
- **Rust edition 2024**：项目使用 Rust 2024 edition。`std::env::set_var` / `remove_var` 在此 edition 中为 `unsafe`。
- **容器环境**：容器内 `APP__SERVER__HOST` 必须为 `0.0.0.0`。项目使用 rustls，运行时仅需 `ca-certificates`（无需 libssl）。迁移脚本 `sqlx::migrate!` 在编译期嵌入，运行时不需要 `migrations/` 目录。

### 文档

- `docs/architecture-design.md` — 架构设计（技术选型、横切关注点、模块职责、数据流）
- `docs/detailed-design.md` — 详细设计索引，链接到 `docs/detailed-design/` 下 per-module 子文档

## 开发约定

- **修改代码后格式化**：每次修改 `.rs` 文件后，运行 `cargo fmt` 确保代码风格一致。
- **查看仓库代码**：优先使用 `codegraph` MCP 工具（`codegraph_explore`、`codegraph_node`、`codegraph_search`）理解代码结构和调用关系，避免逐文件 Read。
- **查询 API/库文档**：需要查阅 `axum`、`sqlx`、`fred`、`tokio` 等依赖的官方文档时，使用 context7（`resolve-library-id` → `query-docs`）获取最新文档和示例。
- **SQL 索引命名**：`pk_` 主键 | `uk_` 唯一 | `idx_` 普通 | `fk_` 外键（如 `uk_users_email`、`idx_users_cursor`、`fk_user_roles_user_id`）
- **启动流程**：config::load() → shutdown::init_tracing() → db::init_pool() → run_migrations()（`sqlx::migrate!("./migrations").run(&pool)` 自动执行）→ valkey::init_pool() → ShutdownRegistry → AppState → 路由 → shutdown::run(listener, app, registry, config)。连接失败使用 `.inspect_err(\|e\| tracing::error!(…))` 记录日志后退出，fail-fast 不延迟连接。
- **首次运行**：`cp .env.example .env` → 设置 `APP_ENV` 和 `APP__JWT__SECRET` → `cargo run`
- **环境变量测试**：`cargo test -- --test-threads=1`（`set_var`/`remove_var` 在 Rust 2024 中为 `unsafe`，测试必须串行）
- **Secrets**：`jwt.secret` 不写入 TOML，必须通过 `APP__JWT__SECRET` 注入。`#[serde(default)]` + validator `length(min=32)` + loader 显式检查三重保证
