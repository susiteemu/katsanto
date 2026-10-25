mod admin;
pub(crate) mod api;
mod community;
mod feeds;
mod me;
mod playlists;
mod proxy;
mod subscriptions;
pub(crate) mod watch;
mod web;

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, header},
    middleware::{self, Next},
    response::Response,
    routing::get,
};
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

use crate::{
    auth::{self, session::require_user},
    client_ip::ClientIpKey,
    error::{AppError, AppResult},
    state::AppState,
};

#[derive(OpenApi)]
#[openapi(
    info(title = "Katsanto API", description = "Katsanto's REST API, used by its web app."),
    tags(
        (name = "auth", description = "Registration, login, recovery, re-authentication"),
        (name = "me", description = "The logged-in user's account"),
        (name = "admin", description = "Instance administration (admins only)"),
        (name = "youtube", description = "Search, video and channel data"),
        (name = "watch", description = "Watch progress"),
        (name = "subscriptions", description = "Subscriptions and the feed"),
        (name = "feeds", description = "Tags and saved feeds"),
        (name = "lists", description = "Watch lists"),
        (name = "community", description = "SponsorBlock and DeArrow"),
    )
)]
struct ApiDoc;

pub struct Options {
    /// Per-client rate limiting of the auth endpoints (off in tests, which have no peer address).
    pub rate_limit: bool,
}

pub fn router(state: AppState, opts: Options) -> Router {
    let mut auth = auth::router();
    if opts.rate_limit {
        // Bursts for normal use (a few page loads, a typo), then one request per 2 s per IP.
        let trusted = state
            .config
            .trusted_proxy_nets()
            .expect("trusted_proxies are checked when loading the config");
        let config = GovernorConfigBuilder::default()
            .key_extractor(ClientIpKey::new(trusted))
            .per_second(2)
            .burst_size(20)
            .finish()
            .expect("rate limit config");
        auth = auth.layer(GovernorLayer::new(Arc::new(config)));
    }
    let youtube =
        api::router().route_layer(middleware::from_fn_with_state(state.clone(), require_user));

    let (api, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest("/api/v1/auth", auth)
        .nest("/api/v1/me", me::router())
        .nest("/api/v1/admin", admin::router())
        .nest("/api/v1", youtube)
        .nest("/api/v1", watch::router())
        .nest("/api/v1", subscriptions::router())
        .nest("/api/v1", feeds::router())
        .nest("/api/v1", playlists::router())
        .nest("/api/v1", community::router())
        .split_for_parts();
    let openapi = Arc::new(openapi);

    let api = api
        .route(
            "/api/v1/openapi.json",
            get(move || {
                let doc = openapi.clone();
                async move { Json(doc) }
            }),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            same_origin_only,
        ))
        // Media bodies are already compressed; only compress API responses.
        .layer(CompressionLayer::new());

    let proxy = Router::new()
        .route("/proxy/video/{video_id}/{stream_id}", get(proxy::video))
        .route("/proxy/img", get(proxy::image))
        .route(
            "/proxy/dearrow/{video_id}",
            get(community::dearrow_thumbnail),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), require_user));

    // Called by YouTube's hub: no session, and not same-origin.
    let websub = Router::new().route(
        "/websub/{channel_id}",
        get(crate::websub::verify).post(crate::websub::notify),
    );

    let proxy_video = state.config.proxy_video;
    let csp = content_security_policy(proxy_video);
    Router::new()
        .merge(api)
        .merge(proxy)
        .merge(websub)
        .fallback(web::serve)
        .layer(middleware::from_fn(move |req, next| {
            // Debug builds read the web app from disk, which a rebuild can change under a
            // running server: recompute the inline script hashes for every response.
            let csp = if cfg!(debug_assertions) {
                content_security_policy(proxy_video)
            } else {
                csp.clone()
            };
            security_headers(csp, req, next)
        }))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// What the web app may load: only our own origin (images and video go through the
/// proxy), plus googlevideo when video isn't proxied. Inline styles are allowed for
/// the user's custom CSS and the player; inline scripts only by hash.
fn content_security_policy(proxy_video: bool) -> HeaderValue {
    let scripts = web::inline_script_hashes().join(" ");
    let video = if proxy_video {
        ""
    } else {
        " https://*.googlevideo.com"
    };
    let policy = format!(
        "default-src 'self'; script-src 'self' {scripts}; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; media-src 'self' blob:{video}; connect-src 'self'{video}; \
         worker-src 'self' blob:; object-src 'none'; base-uri 'self'; form-action 'self'; \
         frame-ancestors 'none'"
    );
    HeaderValue::from_str(&policy).expect("valid CSP header")
}

async fn security_headers(csp: HeaderValue, req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    // Responses with a stricter policy of their own (proxied images) keep it.
    h.entry(header::CONTENT_SECURITY_POLICY).or_insert(csp);
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    res
}

/// Spec for generating the frontend's API client (`katsanto-server openapi`).
pub fn openapi_json() -> String {
    let (_, openapi) = OpenApiRouter::<AppState>::with_openapi(ApiDoc::openapi())
        .nest("/api/v1/auth", auth::router())
        .nest("/api/v1/me", me::router())
        .nest("/api/v1/admin", admin::router())
        .nest("/api/v1", api::router())
        .nest("/api/v1", watch::router())
        .nest("/api/v1", subscriptions::router())
        .nest("/api/v1", feeds::router())
        .nest("/api/v1", playlists::router())
        .nest("/api/v1", community::router())
        .split_for_parts();
    openapi.to_pretty_json().expect("openapi json")
}

/// CSRF protection: state-changing requests must come from our own origin.
/// Browsers always send `Origin` (and `Sec-Fetch-Site`) on such requests; requests
/// without either come from non-browser clients, which cookies can't be forged for.
async fn same_origin_only(
    State(st): State<AppState>,
    req: Request,
    next: Next,
) -> AppResult<Response> {
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)
        && !is_same_origin(req.headers(), &st.config.public_url)
    {
        return Err(AppError::Forbidden("Cross-site request rejected".into()));
    }
    Ok(next.run(req).await)
}

fn is_same_origin(headers: &HeaderMap, public_url: &url::Url) -> bool {
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok())
        && !matches!(site, "same-origin" | "none")
    {
        return false;
    }
    match headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        Some(origin) => origin == public_url.origin().ascii_serialization(),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The web client's generated types are keyed by operation id; duplicates
    /// silently merge two endpoints' types.
    #[test]
    fn operation_ids_are_unique() {
        let spec: serde_json::Value = serde_json::from_str(&openapi_json()).unwrap();
        let mut ids: Vec<&str> = spec["paths"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|p| p.as_object().unwrap().values())
            .filter_map(|op| op["operationId"].as_str())
            .collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate operation ids");
    }

    #[test]
    fn origin_check() {
        let url = url::Url::parse("https://k.example.com").unwrap();
        let h = |pairs: &[(&'static str, &'static str)]| {
            let mut m = HeaderMap::new();
            for (k, v) in pairs {
                m.insert(*k, v.parse().unwrap());
            }
            m
        };
        assert!(is_same_origin(&h(&[]), &url));
        assert!(is_same_origin(
            &h(&[("origin", "https://k.example.com")]),
            &url
        ));
        assert!(is_same_origin(
            &h(&[
                ("origin", "https://k.example.com"),
                ("sec-fetch-site", "same-origin")
            ]),
            &url
        ));
        assert!(!is_same_origin(
            &h(&[("origin", "https://evil.example")]),
            &url
        ));
        assert!(!is_same_origin(
            &h(&[("origin", "http://k.example.com")]),
            &url
        ));
        assert!(!is_same_origin(
            &h(&[("sec-fetch-site", "cross-site")]),
            &url
        ));
        assert!(!is_same_origin(
            &h(&[("sec-fetch-site", "same-site")]),
            &url
        ));
    }
}
