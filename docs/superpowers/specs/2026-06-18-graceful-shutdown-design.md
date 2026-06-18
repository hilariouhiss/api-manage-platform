# Graceful Shutdown System — Design Spec

**Date**: 2026-06-18  
**Status**: Approved  
**Scope**: Implement graceful shutdown covering SIGINT, SIGTERM, and SIGHUP with resource cleanup and configurable drain timeout

---

## Overview

The platform currently has no graceful shutdown — `main.rs` calls
`axum::serve(...).await.unwrap()`, which terminates abruptly on any signal
(Ctrl+C, SIGTERM, etc.). This spec defines a production-grade shutdown
system with proper HTTP drain, resource cleanup, and signal handling.

## Signal Handling

| Signal   | Behavior                                          |
|----------|---------------------------------------------------|
| SIGINT   | Trigger graceful shutdown (Ctrl+C)                |
| SIGTERM  | Trigger graceful shutdown (Docker, k8s, process managers) |
| SIGHUP   | Log event, no-op. Reserved for future reload hooks. Process continues. |

Signal watching runs in a loop inside `shutdown::run()`. SIGHUP is
intercepted and logged; SIGINT/SIGTERM trigger the shutdown sequence.

## Module Structure

```
src/shutdown/
  mod.rs       — GracefulShutdownConfig, init_tracing(), run()
  signals.rs   — watch_signals() → anyhow::Result<SignalEvent>
  registry.rs  — ShutdownRegistry with ordered cleanup entries
```

No changes to `src/response.rs` or `src/routes/`.

## Key Types

### `GracefulShutdownConfig`

```rust
pub struct GracefulShutdownConfig {
    pub drain_timeout: Duration, // default 10 seconds
}
```

Default constructed via `Default::default()`.

### `SignalEvent` (in signals.rs)

```rust
pub(crate) enum SignalEvent {
    Terminate,  // SIGINT or SIGTERM
    Reload,     // SIGHUP
}
```

`watch_signals()` returns `anyhow::Result<SignalEvent>`. Uses
`tokio::signal::ctrl_c()` for SIGINT, and `unix::signal(SignalKind::terminate())`
/ `SignalKind::hangup()` for the other two. SIGHUP returns `Reload`;
the caller logs and loops.

### `ShutdownRegistry`

```rust
type CleanupFn = Box<dyn FnOnce() -> BoxFuture<'static, anyhow::Result<()>> + Send>;

pub struct ShutdownRegistry {
    entries: Vec<ShutdownEntry>,
}

struct ShutdownEntry {
    name: &'static str,
    task: CleanupFn,
}

impl ShutdownRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, name: &'static str, task: CleanupFn);
    pub async fn cleanup(self); // consumes self, runs entries in reverse, logs errors, never aborts
}
```

`cleanup()` runs entries in reverse registration order. Each entry's error
is logged individually; no failure blocks subsequent entries.

### `TracingGuard`

```rust
pub struct TracingGuard;

/// Initialize tracing subscriber with JSON output, env-filter support.
/// Returns a guard that flushes logs on drop.
pub fn init_tracing() -> anyhow::Result<TracingGuard>;
```

Currently a no-op drop guard. Future: hold `tracing_appender::non_blocking::WorkerGuard`.

Tracing defaults to `info` level, overridable via `RUST_LOG` env var.
JSON format enabled (uses `tracing-subscriber` `json` and `env-filter` features,
both already declared in Cargo.toml).

## `shutdown::run()` — the Orchestrator

```rust
pub async fn run(
    listener: TcpListener,
    app: Router,
    registry: ShutdownRegistry,
    config: GracefulShutdownConfig,
) -> anyhow::Result<()>
```

**Concurrency model (CORRECTED):**

1. Server starts serving immediately.
2. A shutdown-signal future runs concurrently inside
   `axum::serve(...).with_graceful_shutdown(shutdown_signal)`.
3. The shutdown-signal future loops on `watch_signals()`:
   - SIGHUP → log, continue
   - SIGINT / SIGTERM → break (trigger axum's graceful shutdown)
4. Once the shutdown signal fires, axum stops accepting new connections and
   drains in-flight requests.
5. A `tokio::time::timeout` wraps the server future to enforce `drain_timeout`.
   That timeout begins ONLY after the signal is received (not during normal serving),
   because axum's graceful shutdown semantics start the drain when the signal
   future resolves.
6. After the server future completes (or times out), `registry.cleanup()` ALWAYS
   runs — even on error or timeout.
7. Returns `Ok(())` on clean exit, or an error on timeout / server error.

The timeout constraint is:
```rust
// The drain timeout starts when the shutdown signal fires.
// axum's with_graceful_shutdown future resolves once the signal triggers,
// then the actual drain begins. So wrapping the whole serve() in timeout
// is correct: normal serving time is unbounded, drain is bounded by timeout.
```

## `main.rs` Integration

```rust
mod shutdown;
mod response;
mod routes;

use axum::{routing::get, Router};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let tracing_guard = shutdown::init_tracing()?;

    let config = shutdown::GracefulShutdownConfig::default();

    let mut registry = shutdown::ShutdownRegistry::new();
    // Future: register postgres pool, redis pool here
    registry.register("tracing", move || {
        Box::pin(async move { drop(tracing_guard); Ok(()) })
    });

    let app = Router::new()
        .route("/api/v1/hello", get(routes::hello::hello));

    let listener = TcpListener::bind("127.0.0.1:3000").await?;
    tracing::info!("Server running at http://127.0.0.1:3000");

    shutdown::run(listener, app, registry, config).await
}
```

Key decisions:
- Listener binding stays in `main.rs` (binding failure is a startup error, not a shutdown concern)
- `main()` returns `anyhow::Result<()>` — no `unwrap()` anywhere
- Tracing is initialized before everything else
- Cleanup always runs regardless of server exit path

## Error Handling

- `anyhow::Result` throughout the shutdown module
- `init_tracing()` returns a Result (fails if subscriber already set)
- `watch_signals()` returns a Result (fails on signal hook registration)
- `cleanup()` is infallible — errors are logged internally, never propagated
- `run()` returns `anyhow::Result<()>` — Ok on clean shutdown, Err on timeout or server error

## Unix-Only Considerations

Signal handling (SIGTERM, SIGHUP) uses `tokio::signal::unix`. This is
Unix-only. On Windows, alternative methods exist but are not in scope.
If cross-platform is needed later, `cfg(unix)` gates can be added around
the signal module.

## Spec Self-Review

- No TBDs or placeholders
- Signal flow is internally consistent: SIGHUP → Reload → log+loop; SIGINT/SIGTERM → Terminate → drain+cleanup+exit
- Scope is focused: one new module, minimal main.rs changes, no refactoring of existing code
- Ambiguities resolved: drain timeout starts after signal; cleanup runs unconditionally; errors are logged not swallowed
