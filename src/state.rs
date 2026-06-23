//! Application state module.
//!
//! Defines [`AppState`], the shared application state injected into
//! axum handlers via [`axum::extract::FromRef`].

use axum::extract::FromRef;
use sqlx::PgPool;

use crate::config::SharedConfig;
use crate::valkey::ValkeyPool;

/// Shared application state.
///
/// Derives [`FromRef`] so handlers can extract sub-state on demand:
///
/// ```rust,ignore
/// async fn handler(State(db): State<PgPool>) { ... }
/// async fn other(State(config): State<SharedConfig>) { ... }
/// ```
#[derive(Clone, FromRef)]
pub struct AppState {
    pub config: SharedConfig,
    pub db: PgPool,
    pub valkey: ValkeyPool,
}
