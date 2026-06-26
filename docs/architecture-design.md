# 架构设计文档

## 系统概述

基于 Rust 的 API 管理平台后端，采用 axum Web 框架 + PostgreSQL + Valkey（Redis 兼容）三层架构，面向内部团队提供用户管理与 RBAC 权限控制能力。

## 架构分层

```
┌─────────────────────────────────────────┐
│              HTTP Layer                  │
│  axum Router → Middleware → Handler      │
├─────────────────────────────────────────┤
│            Application Layer             │
│  认证 (JWT + argon2)                     │
│  鉴权 (RBAC: roles + permissions)        │
│  校验 (validator derive)                 │
├─────────────────────────────────────────┤
│             Data Layer                   │
│  sqlx PgPool (PostgreSQL)                │
│  fred Pool   (Valkey/Redis)              │
└─────────────────────────────────────────┘
```

## 技术选型

| 关注点 | 选型 | 理由 |
| --- | --- | --- |
| Web 框架 | axum 0.8 | 类型安全、与 tokio 生态深度集成、FromRequestParts 可组合提取器 |
| 异步运行时 | tokio 1.52 (full) | Rust 异步事实标准 |
| 数据库 | PostgreSQL + sqlx 0.9 | 编译期 SQL 校验、原生异步、迁移支持 |
| 缓存 | Valkey/Redis + fred 10.1 | 高性能连接池、rustls TLS |
| 密码哈希 | argon2 0.5 | OWASP 推荐的抗 GPU 暴力破解算法 |
| Token 认证 | jsonwebtoken 10.4 + HS256 | 无状态验证，权限嵌入 Claims 零查库开销 |
| 请求校验 | validator 0.20 (derive) | 声明式校验规则，编译期生成 |
| 配置管理 | config 0.15 + dotenvy 0.15 | 多层源叠加、环境变量覆盖 |
| 日志 | tracing 0.1 + tracing-subscriber 0.3 | 结构化日志、双输出（控制台 + 文件滚动） |
| 序列化 | serde 1.0 + serde_json 1.0 | Rust 序列化标准 |

## 横切关注点

### 统一响应格式

所有 API 端点返回 `ApiResponse<T>` —— 包含 HTTP 状态码 `code`、人类可读 `message`、可选 `data` 载荷。Handler 返回 `Result<ApiResponse<T>, AppError>`，由 axum 自动将错误转换为标准 JSON 响应，客户端始终获得一致的结构。

### 统一错误处理

`AppError` 枚举覆盖七种错误类型（BadRequest / Unauthorized / Forbidden / NotFound / Conflict / Validation / Internal），每种映射到对应的 HTTP 状态码与中文消息。`sqlx::Error` 通过 `From` trait 自动转换，对 PostgreSQL 唯一约束冲突（23505）按约束名映射为用户友好提示，防止数据库内部信息泄露。

### 认证鉴权

**认证**：JWT Bearer Token。`AuthUser` 实现 `FromRequestParts`，自动从 `Authorization: Bearer <token>` 头提取并验证 token，无需 handler 手动处理。密码以 argon2（PHC 格式）存储，验证通过 `argon2::PasswordVerifier` 恒定时间比较。

**鉴权**：RBAC 模型。Token 签发时将用户角色与权限扁平化嵌入 Claims，运行时 `require_permission("resource:action")` 仅需内存 `Vec::contains`，零数据库开销。权限格式为 `resource:action` 二元组。

### 配置管理

多层配置源：`default.toml`（必需）→ `{APP_ENV}.toml`（可选）→ `APP__*` 环境变量（最高优先级，双下划线映射嵌套字段）。JWT 密钥强制从环境变量注入，带 `#[serde(default)]` + `length(min=32)` validator + loader 显式检查三重保护，确保不写入 TOML 文件。

### 优雅关闭

两级关闭：信号到达 → axum 排空进行中请求（默认 10 秒 drain_timeout）→ `ShutdownRegistry` LIFO 逆序清理资源。各清理任务错误隔离，单点失败不中断后续清理。日志借助 `TracingGuard`（持有 `WorkerGuard`）在最后时刻刷写，防止丢失。

## 模块职责

| 模块 | 路径 | 职责 |
| --- | --- | --- |
| 入口 | [src/main.rs](src/main.rs) | 编排启动流程：配置 → 日志 → 数据库 → 缓存 → 迁移 → 路由 → 服务器 |
| 库根 | [src/lib.rs](src/lib.rs) | 重新导出所有公共模块供集成测试 |
| 状态 | [src/state.rs](src/state.rs) | `AppState` 聚合配置、PgPool、ValkeyPool，`FromRef` 派生支持子状态提取 |
| 配置 | [src/config/](src/config/) | 结构体定义 + 多层加载器 |
| 数据库 | [src/db.rs](src/db.rs) | PostgreSQL 连接池初始化与关闭 |
| 缓存 | [src/valkey.rs](src/valkey.rs) | Valkey 连接池初始化与关闭 |
| 认证 | [src/auth/](src/auth/) | JWT 签发/验证、argon2 密码哈希/校验 |
| 中间件 | [src/middleware/](src/middleware/) | `AuthUser` 提取器 |
| 路由 | [src/routes/](src/routes/) | auth（注册/登录）、users（CRUD + 个人信息）、roles（查询）、permissions（查询） |
| 模型 | [src/models/](src/models/) | 数据库行类型、请求/响应 DTO、分页辅助 |
| 错误 | [src/errors.rs](src/errors.rs) | `AppError` 枚举 + IntoResponse + From 转换 |
| 响应 | [src/response.rs](src/response.rs) | `ApiResponse<T>` 统一格式 |
| 关闭 | [src/shutdown/](src/shutdown/) | 信号监听、排空超时、资源清理注册表、日志初始化 |

## 数据流

**注册/登录 → Token 签发**：

1. 客户端发送凭证 → `POST /api/v1/auth/register` 或 `/login`
2. Handler 校验请求体（validator derive）
3. 注册：事务内检查唯一性 → 插入用户 → 分配默认角色 → 提交 → 查询角色与权限
4. 登录：查用户 → argon2 验证密码 → 查询角色与权限
5. 将 user_id、username、roles、permissions 嵌入 JWT Claims → 签名 → 返回 token

**认证请求 → 业务处理**：

1. 客户端在 Authorization 头携带 Bearer token
2. `AuthUser` 提取器拦截 → 解码验证 token → 注入 JwtClaims
3. Handler 调用 `claims.require_permission("resource:action")` 进行授权
4. 执行业务逻辑（事务内完成权限检查与数据操作）
5. 返回 `ApiResponse<T>`（JSON）

## 安全设计要点

- 密码以 argon2 PHC 格式存储，不可逆
- JWT 密钥最小 32 字符，强制环境变量注入
- Token 权限即时生效（嵌入 Claims），无状态验证
- 管理操作在同一数据库事务内完成权限范围检查，防 TOCTOU
- 数据库唯一约束冲突不暴露表名/列名，映射为中文消息
- 软删除（deleted_at）保留数据审计追踪
- `updates_by` 记录操作者 ID
