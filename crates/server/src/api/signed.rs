//! Signed download links (SEC-23, SEC-34): `<payload>.<mac>` where the
//! payload names what may be downloaded and until when. Stateless, so any
//! API replica can serve the link; it cannot be altered or extended.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{KeyInit, Mac};

type HmacSha256 = hmac::Hmac<sha2::Sha256>;

pub const LINK_TTL_S: u64 = 600;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn mac(secret: &str, payload: &str) -> HmacSha256 {
    let mut m = HmacSha256::new_from_slice(secret.as_bytes()).expect("any key length");
    m.update(b"myquiz-link-v1:");
    m.update(payload.as_bytes());
    m
}

/// A link token for `subject` (e.g. "quiz:<uuid>"), valid `LINK_TTL_S`.
/// Returns the token and its expiry (Unix seconds).
pub fn sign(secret: &str, subject: &str) -> (String, u64) {
    let exp = now() + LINK_TTL_S;
    let payload = URL_SAFE_NO_PAD.encode(format!("{subject}|{exp}"));
    let tag = hex::encode(mac(secret, &payload).finalize().into_bytes());
    (format!("{payload}.{tag}"), exp)
}

/// The subject, if the token is genuine and not expired.
pub fn verify(secret: &str, token: &str) -> Option<String> {
    let (payload, tag) = token.split_once('.')?;
    let tag = hex::decode(tag).ok()?;
    mac(secret, payload).verify_slice(&tag).ok()?;
    let decoded = String::from_utf8(URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    let (subject, exp) = decoded.rsplit_once('|')?;
    (exp.parse::<u64>().ok()? >= now()).then(|| subject.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_tampering() {
        let (t, _) = sign("k", "quiz:abc");
        assert_eq!(verify("k", &t).as_deref(), Some("quiz:abc"));
        assert_eq!(verify("other", &t), None);
        let (payload, tag) = t.split_once('.').unwrap();
        let forged = URL_SAFE_NO_PAD.encode("quiz:xyz|99999999999");
        assert_eq!(verify("k", &format!("{forged}.{tag}")), None);
        assert_eq!(verify("k", &format!("{payload}.00")), None);
        assert_eq!(verify("k", "garbage"), None);
    }

    #[test]
    fn expired_links_fail() {
        let payload = URL_SAFE_NO_PAD.encode("quiz:abc|1");
        let tag = hex::encode(mac("k", &payload).finalize().into_bytes());
        assert_eq!(verify("k", &format!("{payload}.{tag}")), None);
    }
}
