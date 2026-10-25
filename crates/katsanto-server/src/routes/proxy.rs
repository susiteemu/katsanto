//! Stream and image proxies.
//!
//! googlevideo URLs are bound to the server's IP, and proxying also keeps the
//! viewer's IP away from Google. Bodies are streamed through without buffering.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::{
    error::{AppError, AppResult},
    state::AppState,
};

/// googlevideo throttles large range requests, so never ask for more than this
/// at once. Players simply request the next range.
const MAX_CHUNK: u64 = 10 * 1024 * 1024;

const IMAGE_HOSTS: &[&str] = &["i.ytimg.com", "yt3.ggpht.com", "yt3.googleusercontent.com"];

/// YouTube's image hosts, including the numbered `i1`–`i9.ytimg.com` used by RSS feeds.
fn is_image_host(host: &str) -> bool {
    IMAGE_HOSTS.contains(&host)
        || host
            .strip_suffix(".ytimg.com")
            .and_then(|h| h.strip_prefix('i'))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// URL under which the client loads a YouTube image.
pub fn image_url(upstream: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(upstream.as_bytes()).collect();
    format!("/proxy/img?u={encoded}")
}

pub fn stream_url(video_id: &str, stream_id: &str) -> String {
    format!("/proxy/video/{video_id}/{stream_id}")
}

pub fn is_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub async fn video(
    State(st): State<AppState>,
    Path((video_id, stream_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> AppResult<Response> {
    if !is_video_id(&video_id) {
        return Err(AppError::BadRequest("invalid video id".into()));
    }
    let requested = match headers.get(header::RANGE) {
        Some(v) => Some(
            v.to_str()
                .ok()
                .and_then(parse_range)
                .ok_or_else(|| AppError::BadRequest("unsupported Range header".into()))?,
        ),
        None => None,
    };

    // Up to two retries: with fresh player data, then with a new YouTube session.
    for attempt in 0..3 {
        let playback = st.playback(&video_id).await?;
        let stream = playback
            .stream(&stream_id)
            .ok_or_else(|| AppError::NotFound(format!("stream {stream_id}")))?;

        let (start, end) = requested.unwrap_or((0, None));
        if stream.size.is_some_and(|size| start >= size) {
            let mut res = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
            if let Some(size) = stream.size {
                res.headers_mut().insert(
                    header::CONTENT_RANGE,
                    HeaderValue::from_str(&format!("bytes */{size}")).unwrap(),
                );
            }
            return Ok(res);
        }
        let end = clamp_end(start, end, stream.size);

        let upstream = st
            .http
            .get(&stream.url)
            .header(header::USER_AGENT, &playback.user_agent)
            .header(header::RANGE, format!("bytes={start}-{end}"))
            .send()
            .await
            .map_err(|e| AppError::Upstream(e.to_string()))?;

        let status = upstream.status();
        if matches!(status.as_u16(), 403 | 410) && attempt < 2 {
            if attempt == 0 {
                // Expired or otherwise stale URL: fresh player data usually fixes it.
                tracing::info!(%video_id, %status, "stream URL rejected, refreshing player data");
            } else {
                // Fresh URLs rejected too: the session's PO token is no longer accepted.
                tracing::warn!(%video_id, %status, "fresh stream URL rejected, renewing YouTube session");
                st.yt.reset_session().await;
            }
            st.invalidate_playback(&video_id).await;
            continue;
        }
        if !status.is_success() {
            return Err(AppError::Upstream(format!("googlevideo returned {status}")));
        }

        let mut res = Response::builder().status(status.as_u16());
        for name in [
            header::CONTENT_TYPE,
            header::CONTENT_LENGTH,
            header::CONTENT_RANGE,
        ] {
            if let Some(v) = upstream.headers().get(&name) {
                res = res.header(name, v.as_bytes());
            }
        }
        let res = res
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CACHE_CONTROL, "private, max-age=3600")
            .body(Body::from_stream(upstream.bytes_stream()))
            .map_err(|e| AppError::Upstream(e.to_string()))?;
        return Ok(res);
    }
    Err(AppError::Upstream(
        "googlevideo rejected refreshed stream URL".into(),
    ))
}

/// Parse a single `bytes=start-[end]` range. Suffix and multi-ranges are not
/// used by media players and are rejected.
fn parse_range(v: &str) -> Option<(u64, Option<u64>)> {
    let (start, end) = v.trim().strip_prefix("bytes=")?.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end = match end.trim() {
        "" => None,
        e => Some(e.parse().ok().filter(|&e| e >= start)?),
    };
    Some((start, end))
}

fn clamp_end(start: u64, end: Option<u64>, size: Option<u64>) -> u64 {
    let mut last = start.saturating_add(MAX_CHUNK - 1);
    if let Some(end) = end {
        last = last.min(end);
    }
    if let Some(size) = size {
        last = last.min(size - 1);
    }
    last
}

#[derive(Deserialize)]
pub struct ImageQuery {
    u: String,
}

pub async fn image(State(st): State<AppState>, Query(q): Query<ImageQuery>) -> AppResult<Response> {
    let url = url::Url::parse(&q.u).map_err(|_| AppError::BadRequest("invalid url".into()))?;
    let allowed = url.scheme() == "https" && url.host_str().is_some_and(is_image_host);
    if !allowed {
        return Err(AppError::BadRequest("host not allowed".into()));
    }

    let upstream = st
        .http
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Upstream(e.to_string()))?;
    let status = upstream.status();
    if !status.is_success() {
        return Err(if status == StatusCode::NOT_FOUND {
            AppError::NotFound("image".into())
        } else {
            AppError::Upstream(format!("image host returned {status}"))
        });
    }

    image_response(upstream)
}

/// Pass an upstream image through. It is served from our own origin, so only raster
/// images get through (HTML or SVG could run script here), and the browser is told
/// not to sniff or run anything in it.
pub(crate) fn image_response(upstream: reqwest::Response) -> AppResult<Response> {
    let content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(raster_image_type)
        .ok_or_else(|| AppError::Upstream("upstream did not return an image".into()))?;
    let mut res = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; sandbox",
        )
        .header(header::CACHE_CONTROL, "private, max-age=86400");
    if let Some(v) = upstream.headers().get(header::CONTENT_LENGTH) {
        res = res.header(header::CONTENT_LENGTH, v.as_bytes());
    }
    res.body(Body::from_stream(upstream.bytes_stream()))
        .map_err(|e| AppError::Upstream(e.to_string()))
}

/// `image/jpeg; charset=…` → `image/jpeg`; `None` for anything but a raster image.
fn raster_image_type(content_type: &str) -> Option<String> {
    let essence = content_type.split(';').next()?.trim().to_ascii_lowercase();
    let subtype = essence.strip_prefix("image/")?;
    let ok = !subtype.is_empty()
        && subtype != "svg+xml"
        && subtype
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'));
    ok.then_some(essence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ranges() {
        assert_eq!(parse_range("bytes=0-"), Some((0, None)));
        assert_eq!(parse_range("bytes=100-199"), Some((100, Some(199))));
        assert_eq!(parse_range("bytes=200-100"), None);
        assert_eq!(parse_range("bytes=-500"), None);
        assert_eq!(parse_range("bytes=0-1,5-9"), None);
        assert_eq!(parse_range("items=0-1"), None);
    }

    #[test]
    fn clamps_to_chunk_and_size() {
        assert_eq!(clamp_end(0, None, None), MAX_CHUNK - 1);
        assert_eq!(clamp_end(0, Some(99), None), 99);
        assert_eq!(clamp_end(0, None, Some(1000)), 999);
        assert_eq!(
            clamp_end(500, Some(20_000_000), Some(30_000_000)),
            500 + MAX_CHUNK - 1
        );
    }

    #[test]
    fn clamp_does_not_overflow() {
        assert_eq!(clamp_end(u64::MAX - 1, None, None), u64::MAX);
    }

    #[test]
    fn only_raster_images_pass() {
        assert_eq!(
            raster_image_type("image/jpeg").as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(
            raster_image_type("Image/WebP; charset=binary").as_deref(),
            Some("image/webp")
        );
        assert_eq!(raster_image_type("image/svg+xml"), None);
        assert_eq!(raster_image_type("text/html"), None);
        assert_eq!(raster_image_type("image/"), None);
        assert_eq!(raster_image_type("image/x\r\nset-cookie: a"), None);
    }

    #[test]
    fn validates_video_ids() {
        assert!(is_video_id("dQw4w9WgXcQ"));
        assert!(is_video_id("a-b_c-d_e-f"));
        assert!(!is_video_id("../../etc/p"));
        assert!(!is_video_id("short"));
    }

    #[test]
    fn image_hosts() {
        assert!(is_image_host("i.ytimg.com"));
        assert!(is_image_host("i4.ytimg.com"));
        assert!(is_image_host("yt3.ggpht.com"));
        assert!(!is_image_host("ix.ytimg.com"));
        assert!(!is_image_host("i.ytimg.com.evil.example"));
        assert!(!is_image_host("evil.example"));
    }

    #[test]
    fn image_urls_are_encoded() {
        assert_eq!(
            image_url("https://i.ytimg.com/vi/x/hq.jpg?a=1&b=2"),
            "/proxy/img?u=https%3A%2F%2Fi.ytimg.com%2Fvi%2Fx%2Fhq.jpg%3Fa%3D1%26b%3D2"
        );
    }
}
