# 错误处理与统一响应

## 统一响应格式

[src/response.rs](../../src/response.rs) 定义：

```rust
#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub code: u16,        // HTTP 状态码
    pub message: String,  // 人类可读消息
    pub data: Option<T>,  // 可选载荷
}
```

实现 `IntoResponse`，自动序列化为 JSON。工厂方法：

| 方法 | 用途 | code |
| --- | --- | --- |
| `ApiResponse::ok()` | 无数据成功 | 200 |
| `ApiResponse::success(msg, data)` | 带数据/消息成功 | 200 |
| `ApiResponse::failure(status, msg)` | 指定状态码失败 | 自定义 |

## AppError 枚举

[src/errors.rs](../../src/errors.rs) 定义统一错误类型，Handler 返回 `Result<ApiResponse<T>, AppError>`：

| 变体 | HTTP 状态码 | 响应消息 | 触发场景 |
| --- | --- | --- | --- |
| `BadRequest(String)` | 400 | 自定义（如"无效的分页游标"） | 参数格式错误 |
| `Unauthorized` | 401 | "未认证或 token 无效" | 缺失/无效/过期 token |
| `Forbidden` | 403 | "无权限执行此操作" | 权限不足、越权操作 |
| `NotFound(String)` | 404 | "{资源} 不存在" | 用户/角色不存在 |
| `Conflict(String)` | 409 | 自定义（如"用户名已存在"） | 唯一约束冲突 |
| `Validation(ValidationErrors)` | 422 | validator 自动生成 | 请求体校验失败 |
| `Internal(anyhow::Error)` | 500 | "服务器内部错误" | 数据库故障等未知错误 |

`Internal` 变体日志原始错误后，仅向客户端返回固定消息，不泄露内部细节。

**便捷构造器**：

```rust
impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self;
    pub fn not_found(resource: impl Into<String>) -> Self;   // → "{resource} 不存在"
    pub fn conflict(msg: impl Into<String>) -> Self;
}
```

## 数据库错误转换

`From<sqlx::Error> for AppError` 实现自动转换：

**PostgreSQL 23505（unique_violation）**：解析约束名映射为用户友好消息——

| 约束名 | 映射消息 |
| --- | --- |
| `idx_users_username` | "用户名已被占用" |
| `idx_users_email` | "邮箱已被注册" |
| `idx_users_phone` | "手机号已被注册" |
| `idx_roles_name` | "角色名称已存在" |
| `idx_permissions_resource_action` | "该资源的此操作权限已存在" |
| 未知约束 | "资源已存在，请检查唯一字段" |

**其他错误** → `Internal(e.into())`，不暴露表名/列名。

## 其余 From 实现

| 来源 | 目标 | 说明 |
| --- | --- | --- |
| `anyhow::Error` → `AppError` | `Internal` | 通用错误包装 |
| `sqlx::Error` → `AppError` | 23505→Conflict, 其他→Internal | 上述映射 |

## 错误响应示例

```json
// 401
{ "code": 401, "message": "未认证或 token 无效", "data": null }

// 403
{ "code": 403, "message": "无权限执行此操作", "data": null }

// 404
{ "code": 404, "message": "用户 不存在", "data": null }

// 409
{ "code": 409, "message": "用户名已被占用", "data": null }

// 422
{ "code": 422, "message": "密码长度不能少于 8 位", "data": null }

// 500
{ "code": 500, "message": "服务器内部错误", "data": null }
```
