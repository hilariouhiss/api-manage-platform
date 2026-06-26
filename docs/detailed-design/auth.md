# 认证与鉴权

## 密码处理

[src/auth/password.rs](../../src/auth/password.rs) 提供：

```rust
pub fn hash_password(password: &str) -> Result<String, anyhow::Error>
pub fn verify_password(password: &str, hash: &str) -> Result<bool, anyhow::Error>
```

- **哈希**：随机盐（`SaltString::generate(&mut OsRng)`）+ `Argon2::default()` → PHC 格式字符串（`$argon2id$v=19$...`）
- **校验**：`Argon2::default().verify_password()` 恒定时间比较，防时序攻击

**密码规则**（[src/models/auth.rs](../../src/models/auth.rs) `validate_password`）：

- 最少 8 字符
- 必须包含数字 + 英文字母 + 特殊字符
- 校验失败消息：中文提示

## JWT Token

[src/auth/jwt.rs](../../src/auth/jwt.rs) 基于 `jsonwebtoken` + HS256 算法。

### Claims 结构

[src/models/auth.rs](../../src/models/auth.rs)：

```rust
pub struct JwtClaims {
    pub sub: Uuid,                // 用户 ID
    pub username: String,         // 登录名
    pub roles: Vec<String>,       // 角色名列表，如 ["user"]
    pub permissions: Vec<String>, // 权限列表，如 ["user:read", "user:update"]
    pub exp: usize,               // Unix 过期时间戳
}
```

权限格式为 `"{resource}:{action}"`（如 `user:read`、`user:list`）。roles 和 permissions 嵌入 Claims 后，运行时鉴权仅需内存查找，零数据库开销。

### Token 签发

```rust
pub fn create_token(
    config: &SharedConfig,
    user_id: Uuid,
    username: &str,
    roles: Vec<String>,
    permissions: Vec<String>,
) -> Result<String, anyhow::Error>
```

流程：`exp = now + config.jwt.expiry_hours` → 构造 `JwtClaims` → `jsonwebtoken::encode()` 用 HS256 签名 → 返回编码后的 token 字符串。

### Token 验证

```rust
pub fn verify_token(config: &SharedConfig, token: &str) -> Result<JwtClaims, anyhow::Error>
```

`jsonwebtoken::decode::<JwtClaims>()` 自动验证签名和 `exp` 过期时间，返回 Claims。

## AuthUser 提取器

[src/middleware/auth.rs](../../src/middleware/auth.rs) 实现 `FromRequestParts<S>`，Handler 无需手动处理认证：

```rust
pub struct AuthUser(pub JwtClaims);
```

**提取流程**：

1. 从 state 提取 `SharedConfig`
2. 读取 `Authorization` header → 剥离 `"Bearer "` 前缀
3. 调用 `verify_token()` 解码验证
4. 成功返回 `AuthUser(claims)`，失败返回 401 JSON

**Handler 中的用法**：

```rust
async fn handler(
    AuthUser(claims): AuthUser,
    State(db): State<PgPool>,
) -> Result<ApiResponse<T>, AppError> {
    claims.require_permission("user:list")?;
    // ...
}
```

## 权限检查

`JwtClaims` 提供三个方法：

| 方法 | 返回值 | 说明 |
| --- | --- | --- |
| `has_permission("user:read")` | `bool` | `Vec::contains`，O(n) |
| `require_permission("user:manage")` | `Result<(), AppError>` | 无权限返回 Forbidden |
| `has_role("admin")` | `bool` | 角色检查 |

## 三级管理范围

[src/routes/users.rs](../../src/routes/users.rs) `check_manage_scope` 实现逐级管控：

| 操作者角色 | 可管理的目标用户 | 逻辑 |
| --- | --- | --- |
| system_admin | 所有用户 | 直接放行 |
| admin | 仅拥有 `user` 角色的用户 | 查目标是否含 admin/system_admin 角色，有则拒绝 |
| user | 仅自身 | 不能调用管理类端点（返回 Forbidden） |

管理范围检查与后续数据操作在**同一个数据库事务**内执行，`check_manage_scope` 接受 `impl sqlx::Executor` 参数（`&PgPool` 或 `&mut Transaction`），防 TOCTOU 条件竞争。

## 注册与登录流程

**注册**（`POST /api/v1/auth/register`）：

1. `validator::Validate` 校验请求体（含密码强度规则）
2. argon2 哈希密码
3. 数据库事务：CASE WHEN 三合一唯一性检查（username/email/phone） → INSERT users → INSERT user_roles（分配默认 `user` 角色） → COMMIT
4. 事务外查询 roles + permissions（联表 user_roles + role_permissions）
5. 签发 JWT → 返回 `{ token, expires_at }`

**登录**（`POST /api/v1/auth/login`）：

1. 校验请求体
2. 查用户（`WHERE username = $1 AND deleted_at IS NULL`）
3. 验证密码（argon2 恒定时间比较）
4. 查询 roles + permissions
5. 签发 JWT → 返回 `{ token, expires_at }`

## 请求/响应类型

[src/models/auth.rs](../../src/models/auth.rs)：

- `RegisterPayload` — 注册请求体（`display_name`, `username`, `password`, `email`, `phone`, `avatar_url?`, `self_intro?`）
- `LoginPayload` — 登录请求体（`username`, `password`）
- `TokenResponse` — Token 响应（`token: String`, `expires_at: DateTime<Utc>`）
