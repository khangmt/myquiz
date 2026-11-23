//! Wire types shared between the server and the React app.
//!
//! Every type here derives `ts_rs::TS`; `cargo test -p myquiz-protocol`
//! writes the TypeScript definitions to `web/src/generated/`, so the frontend
//! cannot drift from the server (design section 3).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The one error shape every REST endpoint returns (design section 6).
/// The client translates `code`; `message` is for logs and developers.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct ApiError {
    pub error: ApiErrorBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
}

/// Response of `GET /healthz`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Health {
    pub status: String,
    pub role: String,
    pub version: String,
}
/// The signed-in user, from `GET /api/v1/me`. `csrfToken` goes back in the
/// `X-CSRF-Token` header of every request that changes something (SEC-05).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Me {
    pub id: String,
    pub email: String,
    pub display_name: String,
    /// "user" or "admin".
    pub role: String,
    /// "vi" or "en".
    pub locale: String,
    pub csrf_token: String,
}

/// Body of `PATCH /api/v1/me`. Missing fields stay as they are.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct UpdateMe {
    #[ts(optional)]
    pub display_name: Option<String>,
    #[ts(optional)]
    pub locale: Option<String>,
}

pub mod admin;
pub mod quiz;
pub mod report;
pub mod ws;
