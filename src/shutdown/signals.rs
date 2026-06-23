//! Cross-platform signal handling.
//!
//! - All platforms: `ctrl_c()` (SIGINT / Windows Ctrl+C)
//! - Unix extras: SIGTERM (→ Terminate), SIGHUP (→ Reload)

use anyhow::Context;

/// Signal event type.
#[derive(Debug)]
pub(crate) enum SignalEvent {
    /// Termination signal received (SIGINT / SIGTERM / Ctrl+C).
    Terminate,
    /// Reload signal received (SIGHUP, Unix only).
    Reload,
}

/// Watch for shutdown signals.
///
/// Returns the first signal event received. On Unix, listens for
/// SIGINT, SIGTERM, and SIGHUP concurrently; on non-Unix platforms
/// (Windows), only Ctrl+C is monitored.
pub(crate) async fn watch_signals() -> anyhow::Result<SignalEvent> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm =
            signal(SignalKind::terminate()).context("failed to register SIGTERM handler")?;
        let mut sighup =
            signal(SignalKind::hangup()).context("failed to register SIGHUP handler")?;

        tokio::select! {
            _ = tokio::signal::ctrl_c() => Ok(SignalEvent::Terminate),
            _ = sigterm.recv() => Ok(SignalEvent::Terminate),
            _ = sighup.recv() => Ok(SignalEvent::Reload),
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .context("failed to listen for ctrl+c")?;
        Ok(SignalEvent::Terminate)
    }
}
