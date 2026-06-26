# 优雅关闭与日志

## 信号处理

[src/shutdown/signals.rs](../../src/shutdown/signals.rs) 跨平台监听系统信号：

```rust
pub(crate) enum SignalEvent {
    Terminate,  // SIGINT / SIGTERM / Ctrl+C
    Reload,     // SIGHUP（Unix only）
}

pub(crate) async fn watch_signals() -> anyhow::Result<SignalEvent>
```

| 信号 | 平台 | 行为 |
| --- | --- | --- |
| SIGINT (Ctrl+C) | 全平台 | 返回 Terminate |
| SIGTERM | Unix | 返回 Terminate |
| SIGHUP | Unix | 返回 Reload（调用方记录日志后继续循环） |

实现：Unix 使用 `tokio::signal::unix::signal()` 注册 SIGTERM 和 SIGHUP handler，与 `ctrl_c()` 通过 `tokio::select!` 并发等待。Windows 仅监听 `ctrl_c()`。

## 关闭流程

[src/shutdown/mod.rs](../../src/shutdown/mod.rs) `run()` 编排两级关闭：

```
tokio::spawn(signal_task)      ─→ 循环 watch_signals()
    ├─ Terminate               ─→  notify.notify_waiters()
    └─ Reload                  ─→  tracing::info!() + 继续

axum::serve.with_graceful_shutdown(notify.notified())
    → 信号触发后 axum 停止接受新连接，开始排空进行中请求

tokio::spawn(drain_deadline)   ─→ notify.notified().await → sleep(drain_timeout) → warn

tokio::select! {
    server_handle  → 正常排空完成（含服务端错误）
    drain_deadline → 超时，强制关闭
}

signal_task.abort()
registry.cleanup()  // ⚠ 始终执行，无论 Server 如何退出
```

关键设计决策：

- **信号触发后超时才启动**：`drain_deadline` 先 `await notify.notified()` 再 `sleep(drain_timeout)`，正常服务期间无超时限制
- **Cleanup 始终运行**：即使 server 错误或超时，`registry.cleanup()` 在 `tokio::select!` 之后无条件执行
- **超时不 panic**：drain_timeout 到达后 `run()` 返回 `Ok(())`，视为正常退出

## ShutdownRegistry

[src/shutdown/registry.rs](../../src/shutdown/registry.rs) 管理资源清理：

```rust
pub struct ShutdownRegistry { entries: Vec<ShutdownEntry> }

impl ShutdownRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, name: &'static str, task: CleanupFn);
    pub async fn cleanup(self);  // 消耗 self，LIFO 逆序执行
}
```

**Cleanup 特性**：

- LIFO 逆序执行（后注册先清理）
- 每个任务的错误通过 `tracing::error!` 独立记录（含耗时 `elapsed_ms`）
- 单任务失败不中断后续清理（错误隔离）

**当前注册顺序**（[src/main.rs](../../src/main.rs)）：

| 注册序 | 资源 | 清理操作 | 执行序 |
| --- | --- | --- | --- |
| 1 | tracing | drop TracingGuard → 刷写 WorkerGuard | 3（最后） |
| 2 | database | `PgPool::close()` | 2 |
| 3 | valkey | `fred::Pool::quit()` | 1（最先） |

## GracefulShutdownConfig

```rust
pub struct GracefulShutdownConfig {
    pub drain_timeout: Duration,  // 默认 10 秒
}
```

`Default::default()` 提供默认值。可覆盖以适配不同部署环境的排空时间需求。

## 日志系统

[src/shutdown/mod.rs](../../src/shutdown/mod.rs) `init_tracing()` 初始化 `tracing-subscriber`，双输出：

| 输出 | 格式 | ANSI | 用途 |
| --- | --- | --- | --- |
| stdout | 由 `logging.format` 控制（json / pretty） | 是 | 开发调试 |
| 文件 | **固定 JSON** | 否 | 生产日志采集 |

**文件滚动**（基于 `tracing_appender`）：

| logging.log_rotation | 文件命名 | 适用场景 |
| --- | --- | --- |
| daily | `app.log.YYYY-MM-DD` | 常规生产环境 |
| hourly | `app.log.YYYY-MM-DD-HH` | 高频日志 |
| never | `app.log`（单文件追加） | 开发/简单部署 |

**非阻塞写入**：文件输出通过 `tracing_appender::non_blocking` 在后台线程写入，不阻塞业务逻辑。

**TracingGuard**：

```rust
pub struct TracingGuard {
    _worker_guard: tracing_appender::non_blocking::WorkerGuard,
}
```

持有 `WorkerGuard`，drop 时刷写所有缓冲日志到磁盘。在 `ShutdownRegistry` 中最后清理，确保关闭过程的所有日志不丢失。

**日志级别优先级**：`RUST_LOG` 环境变量 > `logging.level` 配置。使用 `tracing_subscriber::EnvFilter::try_from_default_env()`。

**日志目录**：`init_tracing` 内部 `create_dir_all` 确保目录存在，失败则返回 `anyhow::Error` 中止启动。
