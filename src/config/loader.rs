//! Configuration loader.
//!
//! Implements multi-source config loading: default TOML →
//! environment-specific TOML → environment variables.

use anyhow::{Context, bail};
use config::{Config, Environment, File};
use std::path::PathBuf;
use validator::Validate;

use super::AppConfig;

/// Load configuration.
///
/// Loading order (later sources override earlier ones):
/// 1. `{CONFIG_DIR}/default.toml` — defaults (required)
/// 2. `{CONFIG_DIR}/{APP_ENV}.toml` — environment-specific overrides (optional)
/// 3. Environment variables (`APP__` prefix) — highest priority
///
/// Security constraints:
/// - `APP_ENV` must be set to a whitelisted value
/// - `APP__JWT__SECRET` environment variable must be present (TOML-only
///   secrets are rejected)
/// - `jwt.secret` length >= 32 characters (enforced by validator)
/// - In production, the secret must come from an environment variable
///   (not from a TOML file)
pub fn load() -> anyhow::Result<AppConfig> {
    // 1. Load .env file (local dev only; skipped in tests via SKIP_DOTENV)
    if std::env::var("SKIP_DOTENV").is_err() {
        let _ = dotenvy::dotenv().ok();
    }

    // 2. APP_ENV must be set and in the whitelist
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

    // 3. CONFIG_DIR override (default ./config)
    let config_dir = std::env::var("CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("config"));

    let default_path = config_dir.join("default.toml");
    let env_path = config_dir.join(format!("{app_env}.toml"));

    // 4. Build config sources
    let config = Config::builder()
        .add_source(File::from(default_path).required(true))
        .add_source(File::from(env_path).required(false))
        .add_source(Environment::with_prefix("APP").separator("__"))
        .build()?;

    // 5. Deserialize
    let app_config: AppConfig = config.try_deserialize()?;

    // 6. Validator checks (incl. jwt.secret length >= 32)
    app_config.validate()?;

    // 7. Security: JWT secret must come from an env var, never from a TOML file
    if std::env::var("APP__JWT__SECRET").is_err() {
        bail!(
            "APP__JWT__SECRET environment variable must be set; \
             do not put JWT secrets in TOML configuration files"
        );
    }

    // 8. Production-specific checks
    if app_env == "production" {
        // Already verified in step 7 that secret comes from an env var
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
    // Test helpers
    // ============================================================

    /// Create config files in a temporary directory.
    /// Returns `(dir_path, cleanup_guard)`. Uses nanosecond timestamps
    /// to ensure unique directory names per invocation.
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

    /// Set an environment variable, returning a guard that restores the
    /// original value on drop.
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

    /// Helper: batch-set the base environment variables required by every
    /// test, returning all guards (must be kept alive for the test duration).
    fn setup_base_env(dir: &PathBuf) -> Vec<EnvGuard> {
        // Snapshot and clear all pre-existing APP__* vars first, so that
        // environment-specific overrides from the developer's shell or .env
        // file don't interfere with tests.
        let mut guards: Vec<EnvGuard> = std::env::vars()
            .filter(|(k, _)| k.starts_with("APP__"))
            .map(|(k, _)| {
                let prev = std::env::var(&k).ok();
                unsafe { std::env::remove_var(&k) };
                EnvGuard { key: k, prev }
            })
            .collect();

        guards.push(set_env("SKIP_DOTENV", "1"));
        guards.push(set_env("CONFIG_DIR", &dir.to_string_lossy()));
        guards.push(set_env("APP_ENV", "development"));
        guards.push(set_env(
            "APP__JWT__SECRET",
            "a-strong-secret-at-least-32-chars!!",
        ));
        guards.push(set_env("APP__SERVER__HOST", "127.0.0.1"));
        guards
    }

    const DEFAULT_TOML: &str = r#"
[server]
host = "127.0.0.1"
port = 3000

[database]
url = "postgres://localhost/db"
max_connections = 10
min_connections = 1
acquire_timeout_seconds = 30
idle_timeout_minutes = 30

[valkey]
url = "redis://localhost:6379"
pool_size = 8
connect_timeout_seconds = 10
internal_command_timeout_seconds = 10

[jwt]
expiry_hours = 24

[logging]
level = "info"
format = "json"
"#;

    // ============================================================
    // Test cases
    // ============================================================

    // NOTE: These tests manipulate environment variables and global state.
    // Run serially: cargo test -- --test-threads=1

    /// Test 1: Load valid default.toml (with jwt.secret from env var)
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

    /// Test 2: Environment variables override default.toml fields
    #[test]
    fn test_env_overrides_default_toml() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _envs = setup_base_env(&dir);
        // Override the defaults set by setup_base_env
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
        // Fields not overridden keep their defaults
        assert_eq!(cfg.server.host, "127.0.0.1");
    }

    /// Test 3: APP_ENV-specific file loads and overrides default
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

    /// Test 4: Missing env file doesn't cause an error (required=false)
    #[test]
    fn test_missing_env_file_no_error() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _envs = setup_base_env(&dir);
        // Override APP_ENV to staging
        let _e = set_env("APP_ENV", "staging");

        let result = load();
        assert!(
            result.is_ok(),
            "Missing env-specific file should not cause error"
        );
    }

    /// Test 5: Missing default.toml returns an error
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

    /// Test 6: Missing APP_ENV returns an error
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

    /// Test 6b: Invalid APP_ENV value returns an error
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

    /// Test 7: Missing APP__JWT__SECRET returns an error
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

    /// Test 8: APP__JWT__SECRET shorter than 32 chars returns an error
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

    /// Test 8b: TOML has jwt.secret but APP__JWT__SECRET env var is missing → error
    #[test]
    fn test_jwt_secret_in_toml_but_no_env_var_errors() {
        let toml_with_secret = r#"
[server]
host = "127.0.0.1"
port = 3000

[database]
url = "postgres://localhost/db"
max_connections = 10
min_connections = 1
acquire_timeout_seconds = 30
idle_timeout_minutes = 30

[valkey]
url = "redis://localhost:6379"
pool_size = 8
connect_timeout_seconds = 10
internal_command_timeout_seconds = 10

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

    /// Test 9: Validator rejects port=0
    #[test]
    fn test_invalid_port_errors() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _envs = setup_base_env(&dir);
        let _p = set_env("APP__SERVER__PORT", "0");

        let result = load();
        assert!(result.is_err(), "Port 0 should cause validation error");
    }

    /// Test 10: Priority: env var > env file > default.toml
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

    /// Test 11: Environment variable type conversion — string "20" correctly
    /// deserializes to u32
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

    /// Test 12: Missing nested config section returns an error (caught at deserialization)
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

    /// Test 13: logging.level only accepts valid values
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

    /// Test 14: logging.format only accepts valid values
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

    /// Test 14b: logging.log_dir and logging.log_rotation receive serde defaults
    /// when absent from TOML and not overridden by environment variables.
    #[test]
    fn test_logging_default_fields() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _envs = setup_base_env(&dir);

        let cfg = load().unwrap();
        assert_eq!(cfg.logging.log_dir, "./logs");
        assert_eq!(cfg.logging.log_rotation, "daily");
    }

    /// Test 14c: APP__LOGGING__LOG_DIR override works.
    #[test]
    fn test_logging_log_dir_env_override() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _envs = setup_base_env(&dir);
        let _ld = set_env("APP__LOGGING__LOG_DIR", "/var/log/myapp");

        let cfg = load().unwrap();
        assert_eq!(cfg.logging.log_dir, "/var/log/myapp");
        // log_rotation should still be the default
        assert_eq!(cfg.logging.log_rotation, "daily");
    }

    /// Test 14d: APP__LOGGING__LOG_ROTATION with valid values (hourly, never).
    #[test]
    fn test_logging_log_rotation_env_override() {
        for (value, expected) in &[("hourly", "hourly"), ("never", "never")] {
            let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
            let _envs = setup_base_env(&dir);
            let _lr = set_env("APP__LOGGING__LOG_ROTATION", value);

            let cfg = load().unwrap();
            assert_eq!(
                cfg.logging.log_rotation, *expected,
                "log_rotation should be '{}' when env var is '{}'",
                expected, value
            );
            // log_dir should still be the default
            assert_eq!(cfg.logging.log_dir, "./logs");
        }
    }

    /// Test 14e: logging.log_rotation rejects invalid values via env var.
    #[test]
    fn test_logging_log_rotation_invalid_env_value_errors() {
        let (dir, _guard) = setup_config_dir(&[("default.toml", DEFAULT_TOML)]);
        let _envs = setup_base_env(&dir);
        let _lr = set_env("APP__LOGGING__LOG_ROTATION", "weekly");

        let result = load();
        assert!(
            result.is_err(),
            "Invalid log rotation should cause validation error"
        );
    }

    /// Test 14f: logging fields set via TOML are respected (not just defaults).
    #[test]
    fn test_logging_fields_from_toml() {
        let toml_with_logging = r#"
[server]
host = "127.0.0.1"
port = 3000

[database]
url = "postgres://localhost/db"
max_connections = 10
min_connections = 1
acquire_timeout_seconds = 30
idle_timeout_minutes = 30

[valkey]
url = "redis://localhost:6379"
pool_size = 8
connect_timeout_seconds = 10
internal_command_timeout_seconds = 10

[jwt]
expiry_hours = 24

[logging]
level = "debug"
format = "pretty"
log_dir = "/custom/logs"
log_rotation = "hourly"
"#;
        let (dir, _guard) = setup_config_dir(&[("default.toml", toml_with_logging)]);
        // Use base env but clear any APP__LOGGING__* overrides
        let _envs = setup_base_env(&dir);
        let _cleanup: Vec<EnvGuard> = std::env::vars()
            .filter(|(k, _)| k.starts_with("APP__LOGGING__"))
            .map(|(k, _)| {
                let prev = std::env::var(&k).ok();
                unsafe { std::env::remove_var(&k) };
                EnvGuard { key: k, prev }
            })
            .collect();

        let cfg = load().unwrap();
        assert_eq!(cfg.logging.level, "debug");
        assert_eq!(cfg.logging.format, "pretty");
        assert_eq!(cfg.logging.log_dir, "/custom/logs");
        assert_eq!(cfg.logging.log_rotation, "hourly");
    }

    /// Test 15: CONFIG_DIR override changes the config directory
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
min_connections = 1
acquire_timeout_seconds = 30
idle_timeout_minutes = 30

[valkey]
url = "redis://localhost:6379"
pool_size = 8
connect_timeout_seconds = 10
internal_command_timeout_seconds = 10

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
min_connections = 1
acquire_timeout_seconds = 30
idle_timeout_minutes = 30

[valkey]
url = "redis://other:6380"
pool_size = 16
connect_timeout_seconds = 5
internal_command_timeout_seconds = 5

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
        // Clear any externally-sourced APP__* vars that would override the TOML.
        let _app_cleanup: Vec<EnvGuard> = std::env::vars()
            .filter(|(k, _)| k.starts_with("APP__") && k != "APP__JWT__SECRET" && k != "APP_ENV")
            .map(|(k, _)| {
                let prev = std::env::var(&k).ok();
                unsafe { std::env::remove_var(&k) };
                EnvGuard { key: k, prev }
            })
            .collect();

        // Point to directory 1
        let _cd1 = set_env("CONFIG_DIR", &dir1.to_string_lossy());
        let cfg1 = load().unwrap();
        assert_eq!(cfg1.server.port, 1111);
        drop(_cd1);

        // Point to directory 2
        let _cd2 = set_env("CONFIG_DIR", &dir2.to_string_lossy());
        let cfg2 = load().unwrap();
        assert_eq!(cfg2.server.port, 2222);
    }
}
