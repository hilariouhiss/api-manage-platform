//! Shutdown cleanup registry.
//!
//! Resources are cleaned up in reverse registration order (LIFO).
//! Errors from individual entries are logged but never interrupt
//! subsequent cleanup tasks.

use std::future::Future;
use std::pin::Pin;

type CleanupFn =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> + Send>;

struct ShutdownEntry {
    name: &'static str,
    task: CleanupFn,
}

/// Ordered resource cleanup registry.
///
/// Executes registered cleanup tasks in LIFO order during shutdown.
/// Errors from individual tasks are logged independently and never
/// prevent subsequent tasks from running.
pub struct ShutdownRegistry {
    entries: Vec<ShutdownEntry>,
}

impl ShutdownRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Register a cleanup task.
    ///
    /// `name` identifies the resource in logs; `task` is the async
    /// cleanup function. Tasks run in reverse registration order
    /// (last registered runs first).
    pub fn register(&mut self, name: &'static str, task: CleanupFn) {
        self.entries.push(ShutdownEntry { name, task });
    }

    /// Consume the registry and run all cleanup tasks in reverse order.
    ///
    /// Errors are logged via `tracing::error!` individually; they are
    /// never propagated and never interrupt remaining tasks.
    pub async fn cleanup(self) {
        for entry in self.entries.into_iter().rev() {
            tracing::info!(resource = entry.name, "shutting down");
            let start = std::time::Instant::now();
            match (entry.task)().await {
                Ok(()) => {
                    tracing::info!(
                        resource = entry.name,
                        elapsed_ms = start.elapsed().as_millis(),
                        "shutdown complete"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        resource = entry.name,
                        elapsed_ms = start.elapsed().as_millis(),
                        error = %e,
                        "shutdown failed"
                    );
                }
            }
        }
        tracing::info!("all cleanup tasks completed");
    }
}

impl Default for ShutdownRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn cleanup_runs_in_reverse_order() {
        let order = Arc::new(Mutex::new(Vec::new()));

        let mut registry = ShutdownRegistry::new();

        for name in ["first", "second", "third"] {
            let order = order.clone();
            registry.register(
                name,
                Box::new(move || {
                    Box::pin(async move {
                        order.lock().unwrap().push(name);
                        Ok(())
                    })
                }),
            );
        }

        registry.cleanup().await;

        let executed = order.lock().unwrap();
        assert_eq!(*executed, vec!["third", "second", "first"]);
    }

    #[tokio::test]
    async fn cleanup_continues_on_error() {
        let order = Arc::new(Mutex::new(Vec::new()));

        let mut registry = ShutdownRegistry::new();

        // Register a task that fails
        let order_clone = order.clone();
        registry.register(
            "failing",
            Box::new(move || {
                Box::pin(async move {
                    order_clone.lock().unwrap().push("failing");
                    anyhow::bail!("simulated failure")
                })
            }),
        );

        // Register a task that succeeds (registered later, runs first)
        let order_clone = order.clone();
        registry.register(
            "succeeding",
            Box::new(move || {
                Box::pin(async move {
                    order_clone.lock().unwrap().push("succeeding");
                    Ok(())
                })
            }),
        );

        registry.cleanup().await;

        let executed = order.lock().unwrap();
        // Both ran; succeeding ran first (later registration)
        assert_eq!(*executed, vec!["succeeding", "failing"]);
    }

    #[tokio::test]
    async fn empty_registry_cleanup_is_noop() {
        let registry = ShutdownRegistry::new();
        registry.cleanup().await; // must not panic
    }

    #[tokio::test]
    async fn default_creates_empty_registry() {
        let registry = ShutdownRegistry::default();
        registry.cleanup().await; // must not panic
    }

    #[tokio::test]
    async fn single_entry_registers_and_cleans_up() {
        let mut registry = ShutdownRegistry::new();
        registry.register("only", Box::new(|| Box::pin(async { Ok(()) })));
        registry.cleanup().await; // must not panic
    }
}
