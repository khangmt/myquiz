//! PINs and player tokens, drawn from the operating system's random
//! generator (SEC-17).

use rand::{rngs::OsRng, TryRngCore};
use sha2::{Digest, Sha256};

/// A random 6-digit PIN, 100000..=999999 so it never starts with 0.
pub fn new_pin() -> String {
    loop {
        let mut b = [0u8; 4];
        OsRng.try_fill_bytes(&mut b).expect("OS random generator");
        let n = u32::from_le_bytes(b);
        // Rejection sampling keeps the distribution uniform.
        if n < u32::MAX - (u32::MAX % 900_000) {
            return (100_000 + n % 900_000).to_string();
        }
    }
}

/// A random 128-bit player token, hex encoded. The browser keeps it; Redis
/// keeps only its hash (design section 4, FR-GM-09).
pub fn new_player_token() -> String {
    let mut b = [0u8; 16];
    OsRng.try_fill_bytes(&mut b).expect("OS random generator");
    hex::encode(b)
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_six_digits() {
        for _ in 0..1000 {
            let p = new_pin();
            assert_eq!(p.len(), 6);
            assert!(p.bytes().all(|c| c.is_ascii_digit()));
            assert_ne!(&p[..1], "0");
        }
    }

    #[test]
    fn tokens_are_random_and_hashed() {
        let a = new_player_token();
        assert_eq!(a.len(), 32);
        assert_ne!(a, new_player_token());
        assert_eq!(hash_token(&a), hash_token(&a));
        assert_ne!(hash_token(&a), a);
        assert_eq!(hash_token(&a).len(), 64);
    }
}
