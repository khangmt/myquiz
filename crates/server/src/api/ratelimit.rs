//! Fixed-window rate limits in Redis, shared by every API replica.

use fred::prelude::*;

use super::error::{ApiResult, AppError};
use axum::http::StatusCode;

/// Counts one hit on `bucket` and refuses with 429 RATE_LIMITED once more
/// than `limit` hits fall in the current `window_s` window.
pub async fn hit(redis: &Pool, bucket: &str, limit: u64, window_s: u64) -> ApiResult<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let key = format!("rl:{bucket}:{}", now / window_s);
    let pipe = redis.next().pipeline();
    let _: () = pipe.incr(&key).await?;
    let _: () = pipe.expire(&key, window_s as i64 + 1, None).await?;
    let (count, _): (u64, i64) = pipe.all().await?;
    if count > limit {
        return Err(AppError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "RATE_LIMITED",
            "too many requests, try again shortly",
        ));
    }
    Ok(())
}
