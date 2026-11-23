//! The audit log (SEC-12): logins, failed logins, role changes,
//! suspensions, exports and deletions. IPs are stored only as a keyed hash.

use axum::http::HeaderMap;
use hmac::{KeyInit, Mac};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

type HmacSha256 = hmac::Hmac<sha2::Sha256>;

/// The client IP as Caddy reports it: the last X-Forwarded-For entry, which
/// Caddy appends itself, so a client cannot forge it.
pub fn client_ip(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn hash_ip(secret: &str, ip: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("any key length");
    mac.update(ip.as_bytes());
    hex::encode(&mac.finalize().into_bytes()[..16])
}

pub struct AuditEntry<'a> {
    pub actor: Option<Uuid>,
    pub action: &'a str,
    pub target_type: Option<&'a str>,
    pub target_id: Option<String>,
    pub details: Value,
}

/// Writes one entry. Failures are logged, never shown to the user: a
/// sign-in should not fail because the audit insert did.
pub async fn audit(db: &PgPool, secret: &str, headers: &HeaderMap, e: AuditEntry<'_>) {
    let ip_hash = client_ip(headers).map(|ip| hash_ip(secret, &ip));
    let res = sqlx::query(
        "INSERT INTO audit_log (actor_user_id, action, target_type, target_id, ip_hash, details) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(e.actor)
    .bind(e.action)
    .bind(e.target_type)
    .bind(e.target_id)
    .bind(ip_hash)
    .bind(e.details)
    .execute(db)
    .await;
    if let Err(err) = res {
        tracing::error!(error = %err, action = e.action, "audit insert failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_forwarded_hop_wins() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "6.6.6.6, 10.0.0.9".parse().unwrap());
        assert_eq!(client_ip(&h).as_deref(), Some("10.0.0.9"));
        assert_eq!(client_ip(&HeaderMap::new()), None);
    }

    #[test]
    fn ip_hash_is_keyed_and_stable() {
        assert_eq!(hash_ip("k", "1.2.3.4"), hash_ip("k", "1.2.3.4"));
        assert_ne!(hash_ip("k", "1.2.3.4"), hash_ip("other", "1.2.3.4"));
        assert!(!hash_ip("k", "1.2.3.4").contains("1.2.3.4"));
    }
}
