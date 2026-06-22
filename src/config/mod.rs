use serde::Deserialize;
use validator::Validate;

mod loader;

pub use loader::load;

/// 共享配置类型，用于 axum State 注入
pub type SharedConfig = std::sync::Arc<AppConfig>;

/// 应用配置聚合结构体
#[derive(Debug, Clone, Deserialize, Validate)]
pub struct AppConfig {
    #[validate(nested)]
    pub server: ServerConfig,

    #[validate(nested)]
    pub database: DatabaseConfig,

    #[validate(nested)]
    pub valkey: ValkeyConfig,

    #[validate(nested)]
    pub jwt: JwtConfig,

    #[validate(nested)]
    pub logging: LoggingConfig,
}

/// 服务器配置
#[derive(Debug, Clone, Deserialize, Validate)]
pub struct ServerConfig {
    #[validate(length(min = 1, message = "server.host must not be empty"))]
    pub host: String,

    #[validate(range(
        min = 1,
        max = 65535,
        message = "server.port must be between 1 and 65535"
    ))]
    pub port: u16,
}

/// 数据库配置
#[derive(Debug, Clone, Deserialize, Validate)]
pub struct DatabaseConfig {
    #[validate(length(min = 1, message = "database.url must not be empty"))]
    pub url: String,

    #[validate(range(
        min = 1,
        max = 100,
        message = "database.max_connections must be between 1 and 100"
    ))]
    pub max_connections: u32,

    #[validate(range(
        min = 0,
        max = 100,
        message = "database.min_connections must be between 0 and 100"
    ))]
    pub min_connections: u32,

    #[validate(range(
        min = 1,
        max = 300,
        message = "database.acquire_timeout_seconds must be between 1 and 300"
    ))]
    pub acquire_timeout_seconds: u64,

    #[validate(range(
        min = 1,
        max = 1440,
        message = "database.idle_timeout_minutes must be between 1 and 1440"
    ))]
    pub idle_timeout_minutes: u64,
}

/// Valkey 缓存配置
#[derive(Debug, Clone, Deserialize, Validate)]
pub struct ValkeyConfig {
    #[validate(length(min = 1, message = "valkey.url must not be empty"))]
    pub url: String,

    #[validate(range(
        min = 1,
        max = 256,
        message = "valkey.pool_size must be between 1 and 256"
    ))]
    pub pool_size: u32,

    #[validate(range(
        min = 1,
        max = 60,
        message = "valkey.connect_timeout_seconds must be between 1 and 60"
    ))]
    pub connect_timeout_seconds: u64,

    #[validate(range(
        min = 1,
        max = 60,
        message = "valkey.internal_command_timeout_seconds must be between 1 and 60"
    ))]
    pub internal_command_timeout_seconds: u64,
}

/// JWT 认证配置
///
/// `secret` 字段使用 `#[serde(default)]` 以允许 TOML 中缺失该字段。
/// 实际约束由 validator（length >= 32）保证——无论是漏配还是 TOML 中写了短值都会被拒绝。
/// 生产环境额外在 loader.rs 中做硬性检查。
#[derive(Debug, Clone, Deserialize, Validate)]
pub struct JwtConfig {
    #[serde(default)]
    #[validate(length(
        min = 32,
        message = "JWT secret must be at least 32 characters; set via APP__JWT__SECRET environment variable"
    ))]
    pub secret: String,

    #[validate(range(
        min = 1,
        max = 720,
        message = "jwt.expiry_hours must be between 1 and 720"
    ))]
    pub expiry_hours: u32,
}

/// 日志配置
#[derive(Debug, Clone, Deserialize, Validate)]
pub struct LoggingConfig {
    #[validate(custom(function = "validate_log_level"))]
    pub level: String,

    #[validate(custom(function = "validate_log_format"))]
    pub format: String,
}

fn validate_log_level(level: &str) -> Result<(), validator::ValidationError> {
    match level {
        "trace" | "debug" | "info" | "warn" | "error" => Ok(()),
        _ => {
            let mut err = validator::ValidationError::new("invalid_log_level");
            err.message = Some(
                format!(
                    "logging.level must be one of: trace, debug, info, warn, error; got '{}'",
                    level
                )
                .into(),
            );
            Err(err)
        }
    }
}

fn validate_log_format(format: &str) -> Result<(), validator::ValidationError> {
    match format {
        "json" | "pretty" => Ok(()),
        _ => {
            let mut err = validator::ValidationError::new("invalid_log_format");
            err.message = Some(
                format!(
                    "logging.format must be one of: json, pretty; got '{}'",
                    format
                )
                .into(),
            );
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ServerConfig 校验 ---

    #[test]
    fn test_server_config_valid() {
        let cfg = ServerConfig {
            host: "0.0.0.0".into(),
            port: 3000,
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_server_config_port_zero_invalid() {
        let cfg = ServerConfig {
            host: "0.0.0.0".into(),
            port: 0,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_server_config_empty_host_invalid() {
        let cfg = ServerConfig {
            host: "".into(),
            port: 3000,
        };
        assert!(cfg.validate().is_err());
    }

    // --- DatabaseConfig 校验 ---

    #[test]
    fn test_database_config_valid() {
        let cfg = DatabaseConfig {
            url: "postgres://localhost/db".into(),
            max_connections: 10,
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_database_config_max_connections_zero_invalid() {
        let cfg = DatabaseConfig {
            url: "postgres://localhost/db".into(),
            max_connections: 0,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_database_config_empty_url_invalid() {
        let cfg = DatabaseConfig {
            url: "".into(),
            max_connections: 10,
        };
        assert!(cfg.validate().is_err());
    }

    // --- ValkeyConfig 校验 ---

    #[test]
    fn test_valkey_config_valid() {
        let cfg = ValkeyConfig {
            url: "redis://localhost:6379".into(),
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_valkey_config_empty_url_invalid() {
        let cfg = ValkeyConfig { url: "".into() };
        assert!(cfg.validate().is_err());
    }

    // --- JwtConfig 校验 ---

    #[test]
    fn test_jwt_config_valid() {
        let cfg = JwtConfig {
            secret: "a-strong-secret-at-least-32-chars!".into(),
            expiry_hours: 24,
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_jwt_config_secret_too_short_invalid() {
        let cfg = JwtConfig {
            secret: "short".into(),
            expiry_hours: 24,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_jwt_config_secret_empty_invalid() {
        let cfg = JwtConfig {
            secret: "".into(),
            expiry_hours: 24,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_jwt_config_expiry_zero_invalid() {
        let cfg = JwtConfig {
            secret: "a-strong-secret-at-least-32-chars!".into(),
            expiry_hours: 0,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_jwt_config_expiry_above_720_invalid() {
        let cfg = JwtConfig {
            secret: "a-strong-secret-at-least-32-chars!".into(),
            expiry_hours: 721,
        };
        assert!(cfg.validate().is_err());
    }

    // --- LoggingConfig 校验 ---

    #[test]
    fn test_logging_config_valid_json() {
        let cfg = LoggingConfig {
            level: "info".into(),
            format: "json".into(),
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_logging_config_valid_pretty() {
        let cfg = LoggingConfig {
            level: "debug".into(),
            format: "pretty".into(),
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_logging_config_invalid_level() {
        let cfg = LoggingConfig {
            level: "verbose".into(),
            format: "json".into(),
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_logging_config_invalid_format() {
        let cfg = LoggingConfig {
            level: "info".into(),
            format: "text".into(),
        };
        assert!(cfg.validate().is_err());
    }

    // --- AppConfig 嵌套校验 ---

    #[test]
    fn test_app_config_nested_validation_propagates() {
        let cfg = AppConfig {
            server: ServerConfig {
                host: "0.0.0.0".into(),
                port: 0,
            }, // invalid port
            database: DatabaseConfig {
                url: "postgres://localhost/db".into(),
                max_connections: 10,
            },
            valkey: ValkeyConfig {
                url: "redis://localhost:6379".into(),
            },
            jwt: JwtConfig {
                secret: "a-strong-secret-at-least-32-chars!".into(),
                expiry_hours: 24,
            },
            logging: LoggingConfig {
                level: "info".into(),
                format: "json".into(),
            },
        };
        assert!(cfg.validate().is_err());
    }
}
