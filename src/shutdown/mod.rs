//! Graceful shutdown module.
//!
//! Provides signal watching, HTTP drain, and resource cleanup in a
//! complete graceful shutdown flow.
//!
//! # Shutdown flow
//!
//! 1. Server begins accepting connections immediately
//! 2. Concurrently listen for shutdown signals (SIGINT / SIGTERM / Ctrl+C)
//! 3. On signal, axum stops accepting new connections and begins draining
//!    in-flight requests
//! 4. The drain phase has a `drain_timeout` guard
//! 5. Regardless of outcome, [`ShutdownRegistry::cleanup()`] always runs

mod registry;
mod signals;

use std::future::IntoFuture;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail};
use tokio::net::TcpListener;
use tracing_subscriber::prelude::*;

use crate::config::LoggingConfig;

pub use registry::ShutdownRegistry;
use signals::SignalEvent;

/// Graceful shutdown configuration.
pub struct GracefulShutdownConfig {
    /// HTTP drain timeout.
    ///
    /// After the shutdown signal fires, this is the maximum time allowed
    /// for in-flight requests to complete. If exceeded, shutdown is forced.
    pub drain_timeout: Duration,
}

impl Default for GracefulShutdownConfig {
    fn default() -> Self {
        Self {
            drain_timeout: Duration::from_secs(10),
        }
    }
}

/// Guard for the tracing subscriber.
///
/// Holds a [`tracing_appender::non_blocking::WorkerGuard`] that flushes
/// buffered log messages to disk when dropped. This ensures no log data
/// is lost during graceful shutdown.
#[derive(Debug)]
pub struct TracingGuard {
    _worker_guard: tracing_appender::non_blocking::WorkerGuard,
}

/// Initialize the tracing subscriber with dual output.
///
/// # Output
///
/// - **Console** (stdout): Formatted according to `config.format` (`json`
///   or `pretty`), with ANSI colors.
/// - **File**: Always JSON format, written to a rolling log file under
///   `config.log_dir`. The rotation strategy (`daily`, `hourly`, or
///   `never`) is controlled by `config.log_rotation`.
///
/// Log level is overridable via the `RUST_LOG` environment variable.
/// Defaults to `config.level` when `RUST_LOG` is not set.
///
/// Returns a [`TracingGuard`] that must be kept alive for the lifetime of
/// the process — dropping it flushes and shuts down the file writer.
pub fn init_tracing(config: &LoggingConfig) -> anyhow::Result<TracingGuard> {
    // Ensure the log directory exists
    std::fs::create_dir_all(&config.log_dir)
        .with_context(|| format!("failed to create log directory: {}", config.log_dir))?;

    // Rolling file appender
    let file_appender = match config.log_rotation.as_str() {
        "daily" => tracing_appender::rolling::daily(&config.log_dir, "app.log"),
        "hourly" => tracing_appender::rolling::hourly(&config.log_dir, "app.log"),
        "never" => tracing_appender::rolling::never(&config.log_dir, "app.log"),
        other => bail!(
            "invalid log_rotation '{}': must be daily, hourly, or never",
            other
        ),
    };

    // Non-blocking writer: logs are written on a background thread
    let (non_blocking, worker_guard) = tracing_appender::non_blocking(file_appender);

    // Env filter: RUST_LOG env var or config.level default
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| config.level.as_str().into());

    // Console layer: respects the configured format (json or pretty)
    let console_layer = {
        let layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stdout)
            .with_ansi(true);
        if config.format == "json" {
            layer.json().boxed()
        } else {
            layer.boxed()
        }
    };

    // File layer: always JSON for machine readability, no ANSI
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .json()
        .boxed();

    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .with(env_filter)
        .try_init()
        .map_err(|e| anyhow::anyhow!("failed to initialize tracing subscriber: {e}"))?;

    Ok(TracingGuard {
        _worker_guard: worker_guard,
    })
}

/// Start the server and manage the graceful shutdown lifecycle.
///
/// # Shutdown flow (two-phase)
///
/// - **Phase 1**: Server serves normally while waiting for a shutdown
///   signal. No timeout.
/// - **Phase 2**: On signal, axum begins draining in-flight requests while
///   a `drain_timeout` timer runs concurrently. If the timer expires first,
///   shutdown is forced.
///
/// Cleanup always runs, regardless of how the server exits.
pub async fn run(
    listener: TcpListener,
    app: axum::Router,
    registry: ShutdownRegistry,
    config: GracefulShutdownConfig,
) -> anyhow::Result<()> {
    let shutdown_notify = Arc::new(tokio::sync::Notify::new());

    // Signal-watching task: on Terminate, call notify_waiters()
    let signal_task = tokio::spawn({
        let notify = shutdown_notify.clone();
        async move {
            loop {
                match signals::watch_signals().await {
                    Ok(SignalEvent::Terminate) => {
                        tracing::info!("termination signal received");
                        notify.notify_waiters();
                        break;
                    }
                    Ok(SignalEvent::Reload) => {
                        tracing::info!("SIGHUP received, reload not implemented — continuing");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "signal watch error, initiating shutdown");
                        notify.notify_waiters();
                        break;
                    }
                }
            }
        }
    });

    // axum graceful shutdown signal: resolves when Notify fires → axum begins drain
    let axum_signal = {
        let notify = shutdown_notify.clone();
        async move { notify.notified().await }
    };

    // Spawn the server future on a dedicated task.
    // WithGracefulShutdown only implements IntoFuture, so an explicit conversion is needed.
    let server_handle = tokio::spawn(
        axum::serve(listener, app)
            .with_graceful_shutdown(axum_signal)
            .into_future(),
    );

    // Drain deadline task: starts counting only after the signal fires
    let drain_deadline_handle = tokio::spawn({
        let notify = shutdown_notify.clone();
        let drain_timeout = config.drain_timeout;
        async move {
            notify.notified().await;
            tracing::info!(
                timeout_secs = drain_timeout.as_secs(),
                "drain phase started"
            );
            tokio::time::sleep(drain_timeout).await;
            tracing::warn!("drain timeout reached");
        }
    });

    tracing::info!("server is ready to accept connections");

    // Wait for the server to finish (normal drain or timeout)
    let result = tokio::select! {
        server_result = server_handle => {
            server_result
                .context("server task panicked")?
                .context("server error during drain")
        }
        _ = drain_deadline_handle => {
            tracing::warn!(
                timeout_secs = config.drain_timeout.as_secs(),
                "drain timeout exceeded, forcing shutdown"
            );
            Ok(())
        }
    };

    // Clean up auxiliary tasks
    signal_task.abort();

    // Resource cleanup (always runs)
    registry.cleanup().await;

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LoggingConfig;
    use std::fs;

    /// Guard that removes a temporary directory on drop.
    struct TempDirGuard(std::path::PathBuf);
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Helper: build a `LoggingConfig` for tests.
    fn test_logging_config(log_dir: &str, rotation: &str) -> LoggingConfig {
        LoggingConfig {
            level: "info".into(),
            format: "json".into(),
            log_dir: log_dir.into(),
            log_rotation: rotation.into(),
        }
    }

    /// Helper: find the first log file under `dir` whose name starts with
    /// `app.log` and return its contents.
    fn read_first_log_file(dir: &str) -> Option<String> {
        let entries = fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("app.log"))
            {
                return fs::read_to_string(&path).ok();
            }
        }
        None
    }

    // ═══════════════════════════════════════════════════════════════
    // init_tracing / TracingGuard tests
    // ═══════════════════════════════════════════════════════════════
    //
    // IMPORTANT: `init_tracing()` calls `try_init()` which sets the
    // global tracing subscriber exactly once per process.  Tests that
    // need a live subscriber are therefore grouped into a SINGLE test
    // function.  Tests that fail *before* `try_init()` (e.g. invalid
    // config) can live in separate functions.

    /// Integration test: full init_tracing → log → drop → verify pipeline.
    ///
    /// Covers:
    /// - Log directory creation
    /// - Successful guard return
    /// - `tracing::info!` is written to file
    /// - `TracingGuard::drop` flushes the worker (file has content after drop)
    /// - `daily` rotation strategy
    #[test]
    fn init_tracing_full_pipeline() {
        let tmp = std::env::temp_dir()
            .join(format!("tracing-test-{}", std::process::id()));
        let _cleanup = TempDirGuard(tmp.clone());

        // ── Step 1: init with "daily" rotation ──────────────────
        let log_dir = tmp.join("daily-logs");
        let cfg = test_logging_config(&log_dir.to_string_lossy(), "daily");

        let guard = init_tracing(&cfg).expect("init_tracing(daily) should succeed");
        assert!(
            log_dir.exists(),
            "log directory should be created by init_tracing"
        );

        // ── Step 2: emit a log event ────────────────────────────
        tracing::info!("integration test message");

        // ── Step 3: drop the guard → flush + shutdown writer ────
        drop(guard);

        // ── Step 4: verify log file exists + has our message ────
        let content = read_first_log_file(&cfg.log_dir)
            .expect("log file should exist after TracingGuard is dropped");
        assert!(
            content.contains("integration test message"),
            "log file should contain the test message"
        );
    }

    /// Error path: invalid rotation value is rejected *before* the
    /// global subscriber is set, so this test is safe to run alongside
    /// the full-pipeline test.
    #[test]
    fn init_tracing_rejects_invalid_rotation() {
        let tmp = std::env::temp_dir()
            .join(format!("tracing-test-rot-{}", std::process::id()));
        let _cleanup = TempDirGuard(tmp.clone());

        let cfg = test_logging_config(&tmp.to_string_lossy(), "weekly");
        let result = init_tracing(&cfg);

        assert!(result.is_err(), "invalid rotation should be rejected");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("log_rotation"),
            "error message should mention log_rotation, got: {err}"
        );
    }

    /// Error path: log directory cannot be created (e.g. path is a file).
    #[test]
    fn init_tracing_fails_when_log_dir_is_inaccessible() {
        let tmp = std::env::temp_dir()
            .join(format!("tracing-test-dir-{}", std::process::id()));
        let _cleanup = TempDirGuard(tmp.clone());

        // Create the temporary parent directory first
        fs::create_dir_all(&tmp).unwrap();

        // Now create a regular *file* where the log subdirectory should go
        let log_dir = tmp.join("not-a-dir");
        fs::write(&log_dir, "block").unwrap();

        let cfg = test_logging_config(&log_dir.to_string_lossy(), "daily");
        let result = init_tracing(&cfg);

        assert!(
            result.is_err(),
            "creating directory where a file exists should fail"
        );
    }

    /// Test that the `init_tracing_rejects_invalid_rotation` error path
    /// also works for other garbage rotation values.
    #[test]
    fn init_tracing_rejects_garbage_rotation_values() {
        let tmp = std::env::temp_dir()
            .join(format!("tracing-test-gbg-{}", std::process::id()));
        let _cleanup = TempDirGuard(tmp.clone());

        for garbage in &["", "DAILY", "Daily", "monthly", "minutely"] {
            let cfg = test_logging_config(&tmp.to_string_lossy(), garbage);
            let result = init_tracing(&cfg);
            assert!(
                result.is_err(),
                "rotation value '{garbage}' should be rejected"
            );
        }
    }
}
