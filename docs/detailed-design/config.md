# 配置系统

## 配置结构

[src/config/mod.rs](../../src/config/mod.rs) 定义类型：

```rust
pub type SharedConfig = Arc<AppConfig>;

pub struct AppConfig {
    pub server: ServerConfig,       // host + port
    pub database: DatabaseConfig,   // url + pool + timeouts
    pub valkey: ValkeyConfig,       // url + pool + timeouts
    pub jwt: JwtConfig,             // secret + expiry_hours
    pub logging: LoggingConfig,     // level + format + log_dir + rotation
}
```

所有子结构体带 `#[derive(Deserialize, Validate)]`，启动时 `app_config.validate()` 自动执行校验规则。`SharedConfig` 为 `Arc<AppConfig>`，通过 axum State 注入。

## 加载顺序

[src/config/loader.rs](../../src/config/loader.rs) `load()` 按以下步骤加载：

1. `dotenvy::dotenv()` 加载 `.env` 文件（测试时 `SKIP_DOTENV=1` 跳过，因 Rust 2024 edition 中 `set_var` 为 `unsafe`，测试需串行 `--test-threads=1`）
2. 校验 `APP_ENV` 必须在白名单 `[development, production, test, staging]`
3. `CONFIG_DIR` 环境变量或默认 `./config` 确定配置目录
4. `Config::builder()` 叠加三个源：
   - `File(default.toml)` — 必需，默认值
   - `File({APP_ENV}.toml)` — 可选，环境特定覆盖
   - `Environment::with_prefix("APP").separator("__")` — 最高优先级，`APP__SERVER__PORT=8080` 映射为 `server.port`
5. `config.try_deserialize::<AppConfig>()` 反序列化
6. `app_config.validate()` 执行 validator 规则（端口范围、pool_size 范围、JWT secret 长度等）
7. 显式检查 `APP__JWT__SECRET` 环境变量必须存在（禁止从 TOML 文件读取密钥）

## 配置项全表

| 配置项 | 默认来源 | 校验规则 | 说明 |
| --- | --- | --- | --- |
| `server.host` | default.toml | 非空 | 监听地址 |
| `server.port` | default.toml / env | 1–65535 | 监听端口 |
| `database.url` | env（推荐） | 非空 | PostgreSQL 连接串 |
| `database.max_connections` | default.toml | — | 最大连接数 |
| `database.min_connections` | default.toml | ≤ max | 最小连接数 |
| `database.acquire_timeout_seconds` | default.toml | — | 获取连接超时 |
| `database.idle_timeout_minutes` | default.toml | — | 空闲超时 |
| `valkey.url` | env（推荐） | 非空 | Valkey 连接串 |
| `valkey.pool_size` | default.toml | 1–256 | 连接池大小 |
| `valkey.connect_timeout_seconds` | default.toml | 1–60 | 连接超时 |
| `valkey.internal_command_timeout_seconds` | default.toml | 1–60 | 命令超时 |
| `jwt.secret` | **仅 env** `APP__JWT__SECRET` | ≥32 字符 | HS256 签名密钥 |
| `jwt.expiry_hours` | default.toml | 1–720 | Token 有效期 |
| `logging.level` | default.toml / `RUST_LOG` | — | 日志级别 |
| `logging.format` | default.toml | json / pretty | 控制台输出格式 |
| `logging.log_dir` | default.toml（默认 `./logs`） | — | 日志文件目录 |
| `logging.log_rotation` | default.toml | daily / hourly / never | 日志滚动策略 |

## JWT 密钥安全

三重保护确保 JWT 密钥不写入 TOML 配置文件：

1. `JwtConfig.secret` 标记 `#[serde(default)]` — 反序列化时若缺失则使用空字符串
2. Validator `length(min = 32)` — 空字符串校验失败
3. Loader 显式 `std::env::var("APP__JWT__SECRET").is_err()` → `bail!()` — 环境变量未设置则直接拒绝启动
