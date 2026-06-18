use anyhow::{Context, bail};
use config::{Config, Environment, File};
use std::path::PathBuf;
use validator::Validate;

use super::AppConfig;

/// 加载配置
///
/// 加载顺序（后面的覆盖前面的）：
/// 1. `{CONFIG_DIR}/default.toml` — 默认值（必需）
/// 2. `{CONFIG_DIR}/{APP_ENV}.toml` — 环境特定覆盖（可选）
/// 3. 环境变量（`APP__` 前缀）— 最高优先级
///
/// 安全约束：
/// - `APP_ENV` 必须显式设置为白名单值之一
/// - `APP__JWT__SECRET` 环境变量必须存在（不允许仅在 TOML 中设置 secret）
/// - `jwt.secret` 长度 >= 32（validator 保证）
/// - production 环境额外要求 secret 必须来自环境变量（不可来自 TOML 文件）
pub fn load() -> anyhow::Result<AppConfig> {
    // 1. 加载 .env 文件（本地开发用；测试环境下通过 SKIP_DOTENV 跳过）
    if std::env::var("SKIP_DOTENV").is_err() {
        let _ = dotenvy::dotenv().ok();
    }

    // 2. APP_ENV 必须显式设置且为白名单值
    let app_env = std::env::var("APP_ENV").with_context(|| {
        "APP_ENV environment variable must be set (e.g., development, production, test, staging)"
    })?;

    const VALID_APP_ENVS: &[&str] = &["development", "production", "test", "staging"];
    if !VALID_APP_ENVS.contains(&app_env.as_str()) {
        bail!(
            "APP_ENV '{}' is invalid; must be one of: {}",
            app_env,
            VALID_APP_ENVS.join(", ")
        );
    }

    // 3. CONFIG_DIR 可选覆盖（默认 ./config）
    let config_dir = std::env::var("CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("config"));

    let default_path = config_dir.join("default.toml");
    let env_path = config_dir.join(format!("{app_env}.toml"));

    // 4. 构建配置源
    let config = Config::builder()
        .add_source(File::from(default_path).required(true))
        .add_source(File::from(env_path).required(false))
        .add_source(Environment::with_prefix("APP").separator("__"))
        .build()?;

    // 5. 反序列化
    let app_config: AppConfig = config.try_deserialize()?;

    // 6. validator 校验（含 jwt.secret length >= 32）
    app_config.validate()?;

    // 7. 安全校验：JWT secret 必须由环境变量注入，不允许仅依赖 TOML 文件
    if std::env::var("APP__JWT__SECRET").is_err() {
        bail!(
            "APP__JWT__SECRET environment variable must be set; \
             do not put JWT secrets in TOML configuration files"
        );
    }

    // 8. 生产环境额外要求
    if app_env == "production" {
        // 已在步骤 7 中验证 secret 来自环境变量，此处无需重复
        tracing::info!("Running in production mode");
    }

    Ok(app_config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    // ============================================================
    // 测试辅助工具
    // ============================================================

    /// 在临时目录中创建配置文件，返回 (目录路径, 清理句柄)。
    /// 使用纳秒时间戳确保每次调用的目录唯一，避免同一测试内多次调用互相覆盖。
    fn setup_config_dir(files: &[(&str, &str)]) -> (PathBuf, TempDirGuard) {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cfg-test-{}-{}", std::process::id(), nanos));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            fs::write(dir.join(name), content).unwrap();
        }
        let guard = TempDirGuard(dir.clone());
        (dir, guard)
    }

    struct TempDirGuard(PathBuf);
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 设置环境变量并返回恢复句柄（Drop 时恢复原值）
    fn set_env(key: &str, value: &str) -> EnvGuard {
        let prev = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value) };
        EnvGuard {
            key: key.to_string(),
            prev,
        }
    }

    struct EnvGuard {
        key: String,
        prev: Option<String>,
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => unsafe { std::env::set_var(&self.key, v) },
                None => unsafe { std::env::remove_var(&self.key) },
            }
        }
    }

    /// 辅助函数：批量设置测试必需的环境变量，返回所有 guard（保持存活直到测试结束）
    fn setup_base_env(dir: &PathBuf) -> Vec<EnvGuard> {
        vec![
            set_env("SKIP_DOTENV", "1"),
            set_env("CONFIG_DIR", &dir.to_string_lossy()),
            set_env("APP_ENV", "development"),
            set_env("APP__JWT__SECRET", "a-strong-secret-at-least-32-chars!!"),
        ]
    }

    const DEFAULT_TOML: &str = r#"
[server]
host = "127.0.0.1"
port = 3000

[database]
url = "postgres://localhost/db"
max_connections = 10

[valkey]
url = "redis://localhost:6379"

[jwt]
expiry_hours = 24

[logging]
level = "info"
format = "json"
"#;

    // ============================================================
    // 测试用例
    // ============================================================

    // 注意：由于测试操作环境变量和全局状态，需要串行运行。
    // 运行方式：cargo test -- --test-threads=1

    /// 测试 1: 加载有效的 default.toml（提供 jwt.secret 环境变量）
    #[test]
    fn test_load_with_default_toml_and_env_secret() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _envs = setup_base_env(&dir);

        let result = load();
        assert!(result.is_ok(), "Expected Ok but got: {:?}", result);

        let cfg = result.unwrap();
        assert_eq!(cfg.server.port, 3000);
        assert_eq!(cfg.database.max_connections, 10);
        assert_eq!(cfg.valkey.url, "redis://localhost:6379");
        assert_eq!(cfg.jwt.secret, "a-strong-secret-at-least-32-chars!!");
        assert_eq!(cfg.jwt.expiry_hours, 24);
        assert_eq!(cfg.logging.level, "info");
        assert_eq!(cfg.logging.format, "json");
    }

    /// 测试 2: 环境变量覆盖 default.toml 中的字段
    #[test]
    fn test_env_overrides_default_toml() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _envs = setup_base_env(&dir);
        // 覆盖 base 中设置的默认值
        let _p = set_env("APP__SERVER__PORT", "8080");
        let _l = set_env("APP__LOGGING__LEVEL", "debug");

        let cfg = load().unwrap();
        assert_eq!(
            cfg.server.port, 8080,
            "env var should override default port"
        );
        assert_eq!(
            cfg.logging.level, "debug",
            "env var should override log level"
        );
        // 未被覆盖的字段保持默认值
        assert_eq!(cfg.server.host, "127.0.0.1");
    }

    /// 测试 3: APP_ENV 指定的环境文件成功加载并覆盖 default
    #[test]
    fn test_env_specific_file_overrides_default() {
        let (dir, _guard) = setup_config_dir(&[
            ("default.toml", DEFAULT_TOML),
            (
                "development.toml",
                r#"
[server]
port = 9999

[logging]
level = "debug"
"#,
            ),
        ]);
        let _envs = setup_base_env(&dir);

        let cfg = load().unwrap();
        assert_eq!(
            cfg.server.port, 9999,
            "development.toml should override default port"
        );
        assert_eq!(
            cfg.logging.level, "debug",
            "development.toml should override log level"
        );
        assert_eq!(cfg.server.host, "127.0.0.1");
    }

    /// 测试 4: 环境文件不存在时不报错（required=false）
    #[test]
    fn test_missing_env_file_no_error() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _envs = setup_base_env(&dir);
        // 覆盖 APP_ENV 为 staging
        let _e = set_env("APP_ENV", "staging");

        let result = load();
        assert!(
            result.is_ok(),
            "Missing env-specific file should not cause error"
        );
    }

    /// 测试 5: default.toml 缺失时返回错误
    #[test]
    fn test_missing_default_toml_errors() {
        let dir = std::env::temp_dir().join(format!("cfg-test-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dir_guard = TempDirGuard(dir.clone());

        let _envs = setup_base_env(&dir);

        let result = load();
        assert!(result.is_err(), "Missing default.toml should cause error");
        drop(dir_guard);
    }

    /// 测试 6: 未设置 APP_ENV 时返回错误
    #[test]
    fn test_missing_app_env_errors() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _skip = set_env("SKIP_DOTENV", "1");
        let _cd = set_env("CONFIG_DIR", &dir.to_string_lossy());
        let _sec = set_env("APP__JWT__SECRET", "a-strong-secret-at-least-32-chars!!");
        unsafe { std::env::remove_var("APP_ENV") };

        let result = load();
        assert!(result.is_err(), "Missing APP_ENV should cause error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("APP_ENV"),
            "Error should mention APP_ENV: {}",
            err
        );
    }

    /// 测试 6b: APP_ENV 值不在白名单中时返回错误
    #[test]
    fn test_invalid_app_env_errors() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _skip = set_env("SKIP_DOTENV", "1");
        let _cd = set_env("CONFIG_DIR", &dir.to_string_lossy());
        let _sec = set_env("APP__JWT__SECRET", "a-strong-secret-at-least-32-chars!!");
        let _e = set_env("APP_ENV", "prodution"); // typo

        let result = load();
        assert!(result.is_err(), "Invalid APP_ENV value should cause error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("APP_ENV"),
            "Error should mention APP_ENV: {}",
            err
        );
    }

    /// 测试 7: 未设置 APP__JWT__SECRET 时返回错误
    #[test]
    fn test_missing_jwt_secret_errors() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _skip = set_env("SKIP_DOTENV", "1");
        let _cd = set_env("CONFIG_DIR", &dir.to_string_lossy());
        let _env = set_env("APP_ENV", "development");
        unsafe { std::env::remove_var("APP__JWT__SECRET") };

        let result = load();
        assert!(
            result.is_err(),
            "Missing JWT secret should cause validation error"
        );
    }

    /// 测试 8: APP__JWT__SECRET 长度不足 32 字符时返回错误
    #[test]
    fn test_jwt_secret_too_short_errors() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _skip = set_env("SKIP_DOTENV", "1");
        let _cd = set_env("CONFIG_DIR", &dir.to_string_lossy());
        let _env = set_env("APP_ENV", "development");
        let _sec = set_env("APP__JWT__SECRET", "too-short");

        let result = load();
        assert!(
            result.is_err(),
            "Short JWT secret should cause validation error"
        );
    }

    /// 测试 8b: TOML 中有 jwt.secret 但没有 APP__JWT__SECRET 环境变量 → 失败
    #[test]
    fn test_jwt_secret_in_toml_but_no_env_var_errors() {
        let toml_with_secret = r#"
[server]
host = "127.0.0.1"
port = 3000

[database]
url = "postgres://localhost/db"
max_connections = 10

[valkey]
url = "redis://localhost:6379"

[jwt]
secret = "a-valid-secret-in-toml-at-least-32!!"
expiry_hours = 24

[logging]
level = "info"
format = "json"
"#;
        let (dir, _guard) = setup_config_dir(&[("default.toml", toml_with_secret)]);
        let _skip = set_env("SKIP_DOTENV", "1");
        let _cd = set_env("CONFIG_DIR", &dir.to_string_lossy());
        let _env = set_env("APP_ENV", "development");
        unsafe { std::env::remove_var("APP__JWT__SECRET") };

        let result = load();
        assert!(
            result.is_err(),
            "TOML having jwt.secret without APP__JWT__SECRET env var should fail"
        );
    }

    /// 测试 9: validator 校验失败（port=0）时返回错误
    #[test]
    fn test_invalid_port_errors() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _envs = setup_base_env(&dir);
        let _p = set_env("APP__SERVER__PORT", "0");

        let result = load();
        assert!(result.is_err(), "Port 0 should cause validation error");
    }

    /// 测试 10: 验证优先级 — 环境变量 > 环境文件 > default.toml
    #[test]
    fn test_priority_env_beats_file_beats_default() {
        let (dir, _guard) = setup_config_dir(&[
            ("default.toml", DEFAULT_TOML),
            (
                "development.toml",
                r#"
[server]
port = 4000
"#,
            ),
        ]);
        let _envs = setup_base_env(&dir);
        let _p = set_env("APP__SERVER__PORT", "5000"); // env beats both

        let cfg = load().unwrap();
        assert_eq!(
            cfg.server.port, 5000,
            "env var should have highest priority"
        );
    }

    /// 测试 11: 环境变量类型转换 — 字符串 "20" 正确反序列化为 u32
    #[test]
    fn test_env_var_type_conversion() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _envs = setup_base_env(&dir);
        let _c = set_env("APP__DATABASE__MAX_CONNECTIONS", "20");
        let _e = set_env("APP__JWT__EXPIRY_HOURS", "48");

        let cfg = load().unwrap();
        assert_eq!(cfg.database.max_connections, 20);
        assert_eq!(cfg.jwt.expiry_hours, 48);
    }

    /// 测试 12: 缺失嵌套配置段时返回错误（反序列化层捕获）
    #[test]
    fn test_missing_nested_section_errors() {
        let (dir, _guard) = setup_config_dir(&[(
            "default.toml",
            r#"
[server]
host = "127.0.0.1"
port = 3000

# database section intentionally missing

[valkey]
url = "redis://localhost:6379"

[jwt]
expiry_hours = 24

[logging]
level = "info"
format = "json"
"#,
        )]);
        let _envs = setup_base_env(&dir);

        let result = load();
        assert!(
            result.is_err(),
            "Missing database section should cause deserialization error"
        );
    }

    /// 测试 13: logging.level 只接受有效值
    #[test]
    fn test_logging_level_invalid_value_errors() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _envs = setup_base_env(&dir);
        let _l = set_env("APP__LOGGING__LEVEL", "verbose");

        let result = load();
        assert!(
            result.is_err(),
            "Invalid log level should cause validation error"
        );
    }

    /// 测试 14: logging.format 只接受有效值
    #[test]
    fn test_logging_format_invalid_value_errors() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _envs = setup_base_env(&dir);
        let _f = set_env("APP__LOGGING__FORMAT", "text");

        let result = load();
        assert!(
            result.is_err(),
            "Invalid log format should cause validation error"
        );
    }

    /// 测试 15: CONFIG_DIR 可覆盖默认配置目录
    #[test]
    fn test_config_dir_override() {
        let (dir1, _guard1) = setup_config_dir(&[(
            "default.toml",
            r#"
[server]
host = "127.0.0.1"
port = 1111

[database]
url = "postgres://localhost/db"
max_connections = 10

[valkey]
url = "redis://localhost:6379"

[jwt]
expiry_hours = 24

[logging]
level = "info"
format = "json"
"#,
        )]);

        let (dir2, _guard2) = setup_config_dir(&[(
            "default.toml",
            r#"
[server]
host = "0.0.0.0"
port = 2222

[database]
url = "postgres://other/db"
max_connections = 5

[valkey]
url = "redis://other:6380"

[jwt]
expiry_hours = 12

[logging]
level = "debug"
format = "pretty"
"#,
        )]);

        let _skip = set_env("SKIP_DOTENV", "1");
        let _env = set_env("APP_ENV", "development");
        let _sec = set_env("APP__JWT__SECRET", "a-strong-secret-at-least-32-chars!!");

        // 指向目录 1
        let _cd1 = set_env("CONFIG_DIR", &dir1.to_string_lossy());
        let cfg1 = load().unwrap();
        assert_eq!(cfg1.server.port, 1111);
        drop(_cd1);

        // 指向目录 2
        let _cd2 = set_env("CONFIG_DIR", &dir2.to_string_lossy());
        let cfg2 = load().unwrap();
        assert_eq!(cfg2.server.port, 2222);
    }
}
