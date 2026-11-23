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
