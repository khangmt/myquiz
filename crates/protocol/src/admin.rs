//! Administration (FR-AD-01, FR-AD-04).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One row of `GET /admin/users?q=&page=`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct AdminUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
    /// "user" or "admin".
    pub role: String,
    /// "active" or "suspended".
    pub status: String,
    /// RFC 3339.
    pub created_at: String,
    pub quiz_count: u32,
    pub game_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct AdminUserPage {
    pub items: Vec<AdminUser>,
    pub page: u32,
    pub page_size: u32,
    #[ts(type = "number")]
    pub total: i64,
}

/// One row of `GET /admin/audit`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct AuditRow {
    #[ts(type = "number")]
    pub id: i64,
    /// RFC 3339.
    pub at: String,
    pub actor_email: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    /// Hashed, never the raw address (SEC-12).
    pub ip_hash: Option<String>,
    #[ts(type = "Record<string, unknown>")]
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct AuditPage {
    pub items: Vec<AuditRow>,
    pub page: u32,
    pub page_size: u32,
    #[ts(type = "number")]
    pub total: i64,
}
