# 数据库设计

## PostgreSQL 连接池

[src/db.rs](../../src/db.rs) 封装 `sqlx::PgPool`：

```rust
pub async fn init_pool(cfg: &DatabaseConfig) -> anyhow::Result<PgPool> {
    ensure!(cfg.min_connections <= cfg.max_connections, ...);
    PgPoolOptions::new()
        .max_connections(cfg.max_connections)
        .min_connections(cfg.min_connections)
        .acquire_timeout(Duration::from_secs(cfg.acquire_timeout_seconds))
        .idle_timeout(Duration::from_secs(cfg.idle_timeout_minutes * 60))
        .connect(&cfg.url).await
}

pub async fn close_pool(pool: &PgPool) {
    pool.close().await; // 唤醒所有等待任务，后续 acquire 返回 PoolClosed
}
```

启动时 `run_migrations()` 调用 `sqlx::migrate!("./migrations")` 自动执行所有未应用迁移。

## Valkey 缓存连接池

[src/valkey.rs](../../src/valkey.rs) 封装 `fred::clients::Pool`：

```rust
pub type ValkeyPool = fred::clients::Pool;

pub async fn init_pool(cfg: &ValkeyConfig) -> anyhow::Result<ValkeyPool> {
    ensure!(cfg.pool_size > 0, ...);
    let config = Config::from_url(&cfg.url)?;
    let mut builder = Builder::from_config(config);
    builder.with_connection_config(|conn| {
        conn.connection_timeout = ...;
        conn.internal_command_timeout = ...;
    });
    let pool = builder.build_pool(cfg.pool_size as usize)?;
    pool.init().await?;
    pool.ping(None).await?;  // 连通性验证
    Ok(pool)
}
```

关键点：必须通过 `Config::from_url(&url)` + `Builder::from_config(config)` 传地址，`Builder::default_centralized()` 会忽略 URL。

## 表结构

迁移文件位于 [migrations/](../../migrations/)。所有主键使用 PostgreSQL `uuidv7()` 生成。

### users — 用户表（软删除）

| 列 | 类型 | 约束 |
| --- | --- | --- |
| id | UUID | PK, DEFAULT uuidv7() |
| display_name | VARCHAR(100) | NOT NULL |
| username | VARCHAR(50) | NOT NULL |
| password_hash | VARCHAR(255) | NOT NULL |
| email | VARCHAR(255) | NOT NULL |
| phone | VARCHAR(30) | NOT NULL |
| avatar_url | TEXT | NULL |
| self_intro | TEXT | NULL |
| created_at | TIMESTAMPTZ | NOT NULL, DEFAULT now() |
| created_by | UUID | NULL |
| updated_at | TIMESTAMPTZ | NULL |
| updated_by | UUID | NULL |
| deleted_at | TIMESTAMPTZ | NULL |
| deleted_by | UUID | NULL |

部分唯一索引（仅对未删除行生效，允许已删除行与活跃行同名）：

- `idx_users_username ON users(username) WHERE deleted_at IS NULL`
- `idx_users_email ON users(email) WHERE deleted_at IS NULL`
- `idx_users_phone ON users(phone) WHERE deleted_at IS NULL`
- `idx_users_cursor ON users(created_at DESC, id DESC) WHERE deleted_at IS NULL`

### roles — 角色表

| 列 | 类型 | 约束 |
| --- | --- | --- |
| id | UUID | PK, DEFAULT uuidv7() |
| name | VARCHAR(50) | NOT NULL |
| description | TEXT | NULL |
| created_at | TIMESTAMPTZ | NOT NULL |
| created_by | UUID | NULL |

唯一索引：`idx_roles_name ON roles(name)`。

### permissions — 权限表

| 列 | 类型 | 约束 |
| --- | --- | --- |
| id | UUID | PK, DEFAULT uuidv7() |
| name | VARCHAR(100) | NOT NULL |
| resource | VARCHAR(50) | NOT NULL |
| action | VARCHAR(50) | NOT NULL |
| description | TEXT | NULL |

唯一索引：`idx_permissions_resource_action ON permissions(resource, action)`。

### role_permissions — 角色-权限关联

| 列 | 类型 | 约束 |
| --- | --- | --- |
| role_id | UUID | FK → roles(id) ON DELETE CASCADE |
| permission_id | UUID | FK → permissions(id) ON DELETE CASCADE |

主键：`(role_id, permission_id)`。

### user_roles — 用户-角色关联

| 列 | 类型 | 约束 |
| --- | --- | --- |
| user_id | UUID | FK → users(id) ON DELETE CASCADE |
| role_id | UUID | FK → roles(id) ON DELETE CASCADE |

主键：`(user_id, role_id)`。

## ER 关系

```
users ──< user_roles >── roles ──< role_permissions >── permissions
```

- 一个用户可有多个角色
- 一个角色可有多个权限
- 权限通过角色间接授予用户

## 种子数据

[migrations/002_seed_data.sql](../../migrations/002_seed_data.sql) 预设：

**三个角色**：`system_admin`（系统管理员）、`admin`（管理员）、`user`（普通用户）。

**五个权限**（resource=user）：

| action | name | 说明 |
| --- | --- | --- |
| read | 读取个人信息 | 查看自己的用户信息 |
| update | 更新个人信息 | 修改自己的用户信息 |
| list | 查询用户列表 | 查看所有用户列表及详情 |
| create | 创建用户 | 创建新用户 |
| manage | 管理用户 | 更新和删除其他用户 |

**角色-权限分配**：

| 角色 | 拥有的 action |
| --- | --- |
| system_admin | read, update, list, create, manage（全部） |
| admin | read, update, list, create, manage（全部） |
| user | read, update |

**种子管理员账号**：用户名 `admin`，密码 `admin123!@#`（argon2 预哈希），角色 `system_admin`。
