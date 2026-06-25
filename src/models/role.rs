//! Role and permission types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// ── Database rows ─────────────────────────────────────────────

/// A row from the `roles` table.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct RoleRow {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
}

/// A row from the `permissions` table.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct PermissionRow {
    pub id: Uuid,
    pub name: String,
    pub resource: String,
    pub action: String,
    pub description: Option<String>,
}

// ── API response types ────────────────────────────────────────

/// Role detail including its assigned permissions.
#[derive(Debug, Serialize)]
pub struct RoleWithPermissions {
    #[serde(flatten)]
    pub role: RoleRow,
    pub permissions: Vec<PermissionRow>,
}

// ── Request payloads ──────────────────────────────────────────

/// Payload for assigning a role to a user.
#[derive(Debug, Deserialize)]
pub struct AssignRolePayload {
    pub role_id: Uuid,
}
