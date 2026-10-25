//! Optional WebSub (PubSubHubbub) push from YouTube's hub: new uploads arrive within
//! a minute instead of at the next RSS poll. The hub must be able to reach
//! `public_url`, so this is off by default.
//!
//! A notification only says "this channel changed"; the channel is then refreshed
//! through the normal RSS path, which also handles edits and deletions. Polling
//! continues at a slower pace as a safety net.

use std::time::Duration;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use hmac::{Hmac, KeyInit, Mac};
use katsanto_db::{feed, now, websub};
use rand::{Rng, distr::Alphanumeric};
use serde::Deserialize;
use sha1::Sha1;

use crate::{error::AppResult, state::AppState};

/// Lease asked from the hub (it may grant less).
const LEASE_S: i64 = 10 * 86400;
/// Renew this long before a lease runs out.
const RENEW_BEFORE_S: i64 = 2 * 86400;
/// Ask again if the hub hasn't verified a request after this long.
const RETRY_AFTER_S: i64 = 6 * 3600;
/// Requests to the hub per maintenance round.
const BATCH: i64 = 50;
const ROUND: Duration = Duration::from_secs(15 * 60);
/// RSS polling interval for channels the hub pushes.
pub const PUSHED_POLL_S: i64 = 6 * 3600;

fn topic(channel_id: &str) -> String {
    format!("https://www.youtube.com/xml/feeds/videos.xml?channel_id={channel_id}")
}

fn callback(st: &AppState, channel_id: &str) -> String {
    st.public_link(&format!("/websub/{channel_id}"))
}

/// Why WebSub can't work with this configuration, if it can't.
pub fn unavailable_reason(st: &AppState) -> Option<&'static str> {
    let host = st.config.public_url.host_str().unwrap_or_default();
    (host == "localhost" || host.starts_with("127.") || host.ends_with(".local"))
        .then_some("public_url is not reachable from the internet")
}

/// Keep push subscriptions in step with what users subscribe to.
pub fn spawn(st: AppState) -> Option<tokio::task::JoinHandle<()>> {
    if !st.config.websub {
        return None;
    }
    if let Some(reason) = unavailable_reason(&st) {
        tracing::warn!("WebSub is enabled but {reason}; not subscribing at the hub");
        return None;
    }
    tracing::info!(hub = %st.config.websub_hub, "WebSub push enabled");
    Some(tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            if let Err(e) = maintain(&st).await {
                tracing::warn!(error = %e, "WebSub maintenance failed");
            }
            tokio::time::sleep(ROUND).await;
        }
    }))
}

async fn maintain(st: &AppState) -> AppResult<()> {
    for id in
        websub::needing_subscription(st.db.pool(), RENEW_BEFORE_S, RETRY_AFTER_S, BATCH).await?
    {
        let fresh: String = rand::rng()
            .sample_iter(Alphanumeric)
            .take(40)
            .map(char::from)
            .collect();
        let secret = websub::requested(st.db.pool(), &id, &fresh).await?;
        if let Err(e) = hub_request(st, &id, "subscribe", Some(&secret)).await {
            tracing::info!(channel = id, error = %e, "WebSub subscribe failed");
        }
    }
    for id in websub::orphaned(st.db.pool(), BATCH).await? {
        // Whatever the hub says, nobody needs the subscription anymore.
        if let Err(e) = hub_request(st, &id, "unsubscribe", None).await {
            tracing::info!(channel = id, error = %e, "WebSub unsubscribe failed");
        }
        websub::delete(st.db.pool(), &id).await?;
    }
    Ok(())
}

async fn hub_request(
    st: &AppState,
    channel_id: &str,
    mode: &str,
    secret: Option<&str>,
) -> Result<(), String> {
    let topic = topic(channel_id);
    let callback = callback(st, channel_id);
    let lease = LEASE_S.to_string();
    let mut form = vec![
        ("hub.callback", callback.as_str()),
        ("hub.topic", topic.as_str()),
        ("hub.mode", mode),
        ("hub.verify", "async"),
        ("hub.lease_seconds", lease.as_str()),
    ];
    if let Some(secret) = secret {
        form.push(("hub.secret", secret));
    }
    let body: String = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form)
        .finish();
    let res = st
        .http
        .post(&st.config.websub_hub)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "hub returned {}: {}",
            res.status(),
            res.text().await.unwrap_or_default().trim()
        ))
    }
}

#[derive(Deserialize)]
pub struct Verification {
    #[serde(rename = "hub.mode")]
    mode: String,
    #[serde(rename = "hub.topic")]
    topic: String,
    #[serde(rename = "hub.challenge")]
    challenge: String,
    #[serde(rename = "hub.lease_seconds")]
    lease_seconds: Option<i64>,
}

/// The hub confirming a (un)subscription: echo the challenge if we asked for it.
pub async fn verify(
    State(st): State<AppState>,
    Path(channel_id): Path<String>,
    Query(q): Query<Verification>,
) -> AppResult<impl IntoResponse> {
    let not_ours = (StatusCode::NOT_FOUND, String::new());
    if q.topic != topic(&channel_id) {
        return Ok(not_ours);
    }
    let sub = websub::get(st.db.pool(), &channel_id).await?;
    let confirm = match (q.mode.as_str(), &sub) {
        ("subscribe", Some(s)) if s.wanted => {
            let lease = q.lease_seconds.unwrap_or(LEASE_S).clamp(60, 365 * 86400);
            websub::verified(st.db.pool(), &channel_id, now() + lease).await?;
            true
        }
        ("unsubscribe", s) => s.as_ref().is_none_or(|s| !s.wanted),
        _ => false,
    };
    Ok(if confirm {
        (StatusCode::OK, q.challenge)
    } else {
        not_ours
    })
}

/// A notification: if it's signed with the channel's secret, refresh the channel now.
/// Always 2xx for a known channel, as the spec asks (a bad signature is just ignored).
pub async fn notify(
    State(st): State<AppState>,
    Path(channel_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<StatusCode> {
    let Some(sub) = websub::get(st.db.pool(), &channel_id).await? else {
        return Ok(StatusCode::GONE);
    };
    let signature = headers
        .get("x-hub-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !signature_ok(&sub.secret, &body, signature) {
        tracing::warn!(
            channel = channel_id,
            "WebSub notification with a bad signature"
        );
        return Ok(StatusCode::ACCEPTED);
    }
    if sub.wanted {
        feed::schedule_now(st.db.pool(), &channel_id).await?;
        st.refresh_wakeup.notify_one();
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `X-Hub-Signature: sha1=<hex HMAC-SHA1 of the body>`.
fn signature_ok(secret: &str, body: &[u8], header: &str) -> bool {
    let Some(hex) = header.strip_prefix("sha1=") else {
        return false;
    };
    let Some(expected) = decode_hex(hex) else {
        return false;
    };
    let mut mac = Hmac::<Sha1>::new_from_slice(secret.as_bytes()).expect("any key length");
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_hmac_sha1_signatures() {
        // HMAC-SHA1("key", "The quick brown fox jumps over the lazy dog")
        let body = b"The quick brown fox jumps over the lazy dog";
        let good = "sha1=de7c9b85b8b78aa6bc8a7a36f70a90701c9db4d9";
        assert!(signature_ok("key", body, good));
        assert!(!signature_ok("other", body, good));
        assert!(!signature_ok("key", b"tampered", good));
        assert!(!signature_ok(
            "key",
            body,
            "de7c9b85b8b78aa6bc8a7a36f70a90701c9db4d9"
        ));
        assert!(!signature_ok("key", body, "sha1=zz"));
    }
}
