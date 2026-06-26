# API 路由

## 路由表

[src/main.rs](../../src/main.rs) 中使用 axum `Router` 挂载所有端点：

| 方法 | 路径 | Handler | 认证 | 所需权限 |
| --- | --- | --- | --- | --- |
| GET | `/api/v1/hello` | `routes::hello::hello` | 无 | 无 |
| POST | `/api/v1/auth/register` | `routes::auth::register` | 无 | 无 |
| POST | `/api/v1/auth/login` | `routes::auth::login` | 无 | 无 |
| GET | `/api/v1/users/me` | `routes::users::me` | JWT | 无 |
| PUT | `/api/v1/users/me` | `routes::users::update_me` | JWT | 无 |
| GET | `/api/v1/users` | `routes::users::list_users` | JWT | `user:list` |
| POST | `/api/v1/users` | `routes::users::create_user` | JWT | `user:create` |
| GET | `/api/v1/users/{id}` | `routes::users::get_user` | JWT | `user:list` |
| PUT | `/api/v1/users/{id}` | `routes::users::update_user` | JWT | `user:manage` |
| DELETE | `/api/v1/users/{id}` | `routes::users::delete_user` | JWT | `user:manage` |
| GET | `/api/v1/roles` | `routes::roles::list_roles` | JWT | `role:list` |
| GET | `/api/v1/roles/{id}` | `routes::roles::get_role` | JWT | `role:list` |
| GET | `/api/v1/permissions` | `routes::permissions::list_permissions` | JWT | `permission:list` |

## 通用响应格式

所有端点返回 `ApiResponse<T>`：

```json
{ "code": 200, "message": "success", "data": { ... } }
{ "code": 400, "message": "无效的分页游标", "data": null }
```

## 认证端点

### POST /api/v1/auth/register

```json
// Request
{
  "display_name": "张三",
  "username": "zhangsan",
  "password": "Abc123!@",
  "email": "zhangsan@example.com",
  "phone": "13800138000",
  "avatar_url": null,
  "self_intro": null
}

// Response 200
{ "code": 200, "message": "注册成功", "data": { "token": "...", "expires_at": "2026-07-03T..." } }
```

处理流程：校验 → 哈希密码 → 事务内唯一性检查 + 写入 → 分配 user 角色 → 签发 JWT。

### POST /api/v1/auth/login

```json
// Request
{ "username": "zhangsan", "password": "Abc123!@" }

// Response 200
{ "code": 200, "message": "登录成功", "data": { "token": "...", "expires_at": "2026-07-03T..." } }
```

处理流程：查用户 → argon2 验证 → 查角色权限 → 签发 JWT。

## 用户端点

### GET /api/v1/users/me

返回当前认证用户的完整资料（不含 password_hash）。

### PUT /api/v1/users/me

更新当前用户自身信息。**禁止**通过此端点修改 `role_ids`。支持 COALESCE 部分更新（display_name, email, phone, avatar_url, self_intro）。

### GET /api/v1/users

分页用户列表，权限 `user:list`。响应类型 `PaginatedResponse<UserListItem>`，不含 password_hash、self_intro、审计字段。

**Query 参数**：

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| limit | i64 | 20 (clamp 1–100) | 每页条数 |
| cursor | String? | 无（首页） | base64url 编码游标 |

**响应**：

```json
{
  "code": 200, "message": "success",
  "data": {
    "data": [
      { "id": "...", "display_name": "张三", "username": "zhangsan", "email": "...", "phone": "...", "avatar_url": null, "created_at": "..." }
    ],
    "next_cursor": "eyJjcmVhdGVkX2F0I...",
    "has_more": true
  }
}
```

### POST /api/v1/users

创建用户，权限 `user:create`。请求体同注册（`CreateUserPayload`）。事务内执行：唯一性检查 → 写入 → 分配 `user` 角色。返回 `UserResponse`。

### GET /api/v1/users/{id}

查看单个用户详情，权限 `user:list`。返回 `UserResponse`。

### PUT /api/v1/users/{id}

更新用户，权限 `user:manage`。请求体 `UpdateUserPayload`（所有字段可选，含 `role_ids`）。

单事务内执行：

1. `check_manage_scope`（管理范围检查，防 TOCTOU）
2. 角色分配验证（admin 不能分配 system_admin/admin 角色）
3. 唯一性检查（email/phone 排除自身）
4. COALESCE 更新 profile
5. 如提供 `role_ids`：DELETE + 批量 INSERT 重建角色关联

返回 `UserResponse`。

### DELETE /api/v1/users/{id}

软删除，权限 `user:manage`。规则：

- 不能删除自身
- 事务内 `check_manage_scope` + `UPDATE users SET deleted_at = now(), deleted_by = $1`
- `rows_affected() == 0` 则返回 404

## 角色与权限端点

### GET /api/v1/roles

返回所有角色列表（`Vec<RoleRow>`），按 `created_at ASC` 排序，权限 `role:list`。

### GET /api/v1/roles/{id}

返回单个角色及其关联权限（`RoleWithPermissions`：flatten 的 RoleRow + `Vec<PermissionRow>`），权限 `role:list`。

### GET /api/v1/permissions

返回所有权限列表（`Vec<PermissionRow>`），按 `resource, action` 排序，权限 `permission:list`。

## 分页机制

采用 keyset（cursor-based）分页，基于 `(created_at DESC, id DESC)` 排序，优于传统 OFFSET 分页：

- **稳定**：插入/删除不影响已翻过的页
- **高效**：利用 `idx_users_cursor` 索引，无需全表扫描

**游标结构**：

```rust
pub struct UserCursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}
```

编码：`serde_json::to_string()` → base64url。解码失败返回 `AppError::BadRequest("无效的分页游标")`。

**查询逻辑**：

- 首页：`WHERE deleted_at IS NULL ORDER BY ... LIMIT $limit + 1`
- 翻页：增加 `AND (created_at, id) < ($cursor.created_at, $cursor.id)`
- `has_more`：结果行数超过 `limit` 则为 true
- `next_cursor`：结果集最后一行的 `(created_at, id)` 编码

## 数据保护规则

- `UserRow`（含 password_hash）永不直接返回 API
- `UserResponse`（详情用）排除 password_hash 和审计字段
- `UserListItem`（列表用）进一步排除 self_intro 和时间戳
- 软删除行在所有查询中通过 `WHERE deleted_at IS NULL` 排除
