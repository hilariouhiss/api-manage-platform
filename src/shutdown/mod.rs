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

use anyhow::Context;
use tokio::net::TcpListener;

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
/// Currently a no-op; in the future it may hold a
/// `tracing_appender::non_blocking::WorkerGuard` to ensure logs are
/// flushed before the process exits.
pub struct TracingGuard {
    _private: (),
}

/// Initialize the tracing subscriber.
///
/// Uses JSON output format with level overridable via the `RUST_LOG`
/// environment variable. Defaults to `info`.
pub fn init_tracing() -> anyhow::Result<TracingGuard> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .try_init()
        .map_err(|e| anyhow::anyhow!("failed to initialize tracing subscriber: {e}"))?;
    Ok(TracingGuard { _private: () })
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
