//! End-to-end tests of the HTTP API against an in-memory database and a fake YouTube.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use katsanto_app::{config::Config, routes, state::AppState};
use katsanto_db::Db;
use katsanto_yt::{
    YoutubeSource, YtError, YtResult,
    model::{
        ChannelFeed, ChannelInfo, ChannelPage, ChannelRef, ChannelTab, FeedVideo, Playback,
        ResolvedUrl, SearchPage, SearchParams, VideoCard, VideoDetails, VideoInfo, VideoPage,
    },
};
use serde_json::{Value, json};
use tower::ServiceExt;
use url::Url;
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};

struct FakeYoutube;

#[async_trait]
impl YoutubeSource for FakeYoutube {
    async fn search(&self, _: &str, _: &SearchParams) -> YtResult<SearchPage> {
        Ok(SearchPage {
            items: vec![],
            cursor: None,
            corrected_query: None,
        })
    }
    async fn search_more(&self, _: &str) -> YtResult<SearchPage> {
        Err(YtError::BadRequest("no".into()))
    }
    async fn search_suggestions(&self, _: &str) -> YtResult<Vec<String>> {
        Ok(vec![])
    }
    async fn resolve_url(&self, _: &str) -> YtResult<ResolvedUrl> {
        Err(YtError::BadRequest("no".into()))
    }
    async fn playback(&self, id: &str) -> YtResult<Playback> {
        Ok(Playback {
            details: VideoDetails {
                id: id.into(),
                title: format!("Video {id}"),
                description: None,
                channel: ChannelRef {
                    id: "UCaaaaaaaaaaaaaaaaaaaaaa".into(),
                    name: "Fake channel".into(),
                    avatar_url: None,
                },
                duration_s: 600,
                view_count: None,
                thumbnail_url: Some(format!("https://i.ytimg.com/vi/{id}/hq.jpg")),
                is_live: false,
            },
            streams: vec![],
            subtitles: vec![],
            hls_manifest_url: None,
            valid_until: i64::MAX,
            user_agent: String::new(),
        })
    }
    async fn video_info(&self, _: &str) -> YtResult<VideoInfo> {
        Err(YtError::Unavailable("no".into()))
    }
    async fn channel(&self, id: &str, tab: ChannelTab) -> YtResult<ChannelPage> {
        if id != FAKE_CHANNEL && id != NO_RSS_CHANNEL {
            return Err(YtError::NotFound(id.into()));
        }
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let card = |id: &str, duration: Option<u32>, short: bool| VideoCard {
            id: id.into(),
            title: id.into(),
            channel: None,
            thumbnail_url: None,
            duration_s: duration,
            // What the parser estimates from "2 hours ago".
            published_at: Some(now - 7200),
            published_text: None,
            view_count: None,
            is_live: false,
            is_short: short,
            is_upcoming: false,
        };
        let items = match tab {
            ChannelTab::Videos if id == NO_RSS_CHANNEL => {
                vec![card("norssvideo1", Some(200), false)]
            }
            ChannelTab::Videos => vec![card("video1aaaaa", Some(321), false)],
            ChannelTab::Shorts => vec![card("short1aaaaa", None, true)],
            ChannelTab::Live => vec![],
        };
        Ok(ChannelPage {
            channel: ChannelInfo {
                id: id.into(),
                name: "Fake channel".into(),
                handle: Some("@fake".into()),
                avatar_url: Some("https://yt3.ggpht.com/a".into()),
                banner_url: None,
                subscriber_count: Some(1234),
                video_count: None,
                description: String::new(),
                has_shorts: true,
                has_live: false,
            },
            videos: VideoPage {
                items,
                cursor: None,
            },
        })
    }
    async fn channel_feed(&self, id: &str) -> YtResult<ChannelFeed> {
        if id != FAKE_CHANNEL {
            return Err(YtError::NotFound(id.into()));
        }
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let v = |id: &str, age: i64, short: bool| FeedVideo {
            id: id.into(),
            title: format!("Title {id}"),
            published_at: now - age,
            thumbnail_url: Some(format!("https://i.ytimg.com/vi/{id}/hq.jpg")),
            view_count: Some(10),
            is_short: short,
        };
        Ok(ChannelFeed {
            channel_id: FAKE_CHANNEL.into(),
            channel_name: "Fake channel".into(),
            // The short isn't flagged in RSS; enrichment finds it on the Shorts tab.
            videos: vec![
                v("video2aaaaa", 10, false),
                v("short1aaaaa", 50, false),
                v("video1aaaaa", 100, false),
            ],
        })
    }
    async fn channel_more(&self, _: &str) -> YtResult<VideoPage> {
        Err(YtError::BadRequest("no".into()))
    }
}

const FAKE_CHANNEL: &str = "UCaaaaaaaaaaaaaaaaaaaaaa";
/// A channel whose RSS feed is unavailable (YouTube's feeds have such outages).
const NO_RSS_CHANNEL: &str = "UCnorssxxxxxxxxxxxxxxxxx";

struct App {
    router: Router,
    db: Db,
    state: AppState,
    origin: Url,
}

/// A browser-like client: keeps its session cookie.
#[derive(Default, Clone)]
struct Client {
    cookie: Option<String>,
}

struct Res {
    status: StatusCode,
    body: Value,
}

impl App {
    async fn new() -> Self {
        Self::with_url("http://localhost:3000").await
    }

    async fn with_url(public_url: &str) -> Self {
        let config = Config {
            public_url: Url::parse(public_url).unwrap(),
            // No network in tests.
            sponsorblock_api: String::new(),
            ..Config::default()
        };
        let db = Db::in_memory().await.unwrap();
        let state = AppState::new(config, db.clone(), Arc::new(FakeYoutube)).unwrap();
        Self {
            router: routes::router(
                state.clone(),
                routes::Options {
                    rate_limit: false,
                    assets: None,
                },
            ),
            db,
            state,
            origin: Url::parse(public_url).unwrap(),
        }
    }

    async fn call(&self, c: &mut Client, method: Method, path: &str, body: Option<Value>) -> Res {
        self.call_with(c, method, path, body, &[]).await
    }

    async fn call_with(
        &self,
        c: &mut Client,
        method: Method,
        path: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> Res {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(cookie) = &c.cookie {
            req = req.header(header::COOKIE, cookie);
        }
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let req = match body {
            Some(b) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let res = self.router.clone().oneshot(req).await.unwrap();
        if let Some(set) = res.headers().get(header::SET_COOKIE) {
            let pair = set.to_str().unwrap().split(';').next().unwrap().to_owned();
            c.cookie = (!pair.ends_with('=')).then_some(pair);
        }
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Res { status, body }
    }

    async fn get(&self, c: &mut Client, path: &str) -> Res {
        self.call(c, Method::GET, path, None).await
    }

    async fn post(&self, c: &mut Client, path: &str, body: Value) -> Res {
        self.call(c, Method::POST, path, Some(body)).await
    }

    async fn register_password(&self, username: &str, invite: Option<&str>) -> (Client, Res) {
        let mut c = Client::default();
        let res = self
            .post(
                &mut c,
                "/api/v1/auth/register/password",
                json!({ "username": username, "password": "correct horse battery", "invite": invite }),
            )
            .await;
        (c, res)
    }

    async fn invite(&self, admin: &mut Client) -> String {
        let res = self
            .post(admin, "/api/v1/admin/invites", json!({ "note": "test" }))
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
        token_from(&res.body["link"]["url"])
    }

    /// Clear the session's re-authentication time, as if logged in long ago.
    async fn expire_reauth(&self) {
        sqlx::query("UPDATE sessions SET reauth_at = NULL")
            .execute(self.db.pool())
            .await
            .unwrap();
    }
}

fn token_from(url: &Value) -> String {
    let url = Url::parse(url.as_str().unwrap()).unwrap();
    url.query_pairs().next().unwrap().1.into_owned()
}

/// A software passkey that behaves like a discoverable credential.
struct Authenticator {
    inner: WebauthnAuthenticator<SoftPasskey>,
    user_handle: Option<String>,
    credential_id: Option<String>,
}

impl Authenticator {
    fn new() -> Self {
        Self {
            inner: WebauthnAuthenticator::new(SoftPasskey::new(true)),
            user_handle: None,
            credential_id: None,
        }
    }

    fn register(&mut self, origin: &Url, options: &Value) -> Value {
        let mut options = options.clone();
        // SoftPasskey can't store resident keys; the server never checks this flag.
        options["publicKey"]["authenticatorSelection"]["requireResidentKey"] = false.into();
        options["publicKey"]["authenticatorSelection"]["residentKey"] = "discouraged".into();
        self.user_handle = options["publicKey"]["user"]["id"]
            .as_str()
            .map(str::to_owned);
        let cred = self
            .inner
            .do_registration(origin.clone(), serde_json::from_value(options).unwrap())
            .unwrap();
        let cred = serde_json::to_value(cred).unwrap();
        self.credential_id = cred["rawId"].as_str().map(str::to_owned);
        cred
    }

    fn authenticate(&mut self, origin: &Url, options: &Value) -> Value {
        let mut options = options.clone();
        // Discoverable login sends no allow-list; SoftPasskey only looks credentials up there.
        if options["publicKey"]["allowCredentials"]
            .as_array()
            .is_none_or(Vec::is_empty)
        {
            options["publicKey"]["allowCredentials"] =
                json!([{ "type": "public-key", "id": self.credential_id }]);
        }
        let cred = self
            .inner
            .do_authentication(origin.clone(), serde_json::from_value(options).unwrap())
            .unwrap();
        let mut cred = serde_json::to_value(cred).unwrap();
        // A discoverable credential returns the user handle (not covered by the signature).
        cred["response"]["userHandle"] = self.user_handle.clone().into();
        cred
    }
}

async fn signup_with_passkey(
    app: &App,
    username: &str,
    invite: Option<&str>,
) -> (Client, Authenticator, Res) {
    let mut c = Client::default();
    let mut auth = Authenticator::new();
    let start = app
        .post(
            &mut c,
            "/api/v1/auth/register/passkey/start",
            json!({ "username": username, "invite": invite }),
        )
        .await;
    assert_eq!(start.status, StatusCode::OK, "{:?}", start.body);
    assert_eq!(
        start.body["options"]["publicKey"]["authenticatorSelection"]["residentKey"],
        "required"
    );
    let cred = auth.register(&app.origin, &start.body["options"]);
    let finish = app
        .post(
            &mut c,
            "/api/v1/auth/register/passkey/finish",
            json!({ "challenge_id": start.body["challenge_id"], "credential": cred, "name": "Soft key" }),
        )
        .await;
    (c, auth, finish)
}

async fn login_with_passkey(app: &App, auth: &mut Authenticator) -> (Client, Res) {
    let mut c = Client::default();
    let start = app
        .post(&mut c, "/api/v1/auth/login/passkey/start", json!({}))
        .await;
    assert_eq!(start.status, StatusCode::OK);
    let cred = auth.authenticate(&app.origin, &start.body["options"]);
    let res = app
        .post(
            &mut c,
            "/api/v1/auth/login/passkey/finish",
            json!({ "challenge_id": start.body["challenge_id"], "credential": cred }),
        )
        .await;
    (c, res)
}

#[tokio::test]
async fn guests_are_blocked() {
    let app = App::new().await;
    let mut guest = Client::default();
    for path in [
        "/api/v1/search?q=rust",
        "/api/v1/videos/dQw4w9WgXcQ",
        "/api/v1/me",
        "/api/v1/admin/users",
        "/proxy/img?u=https%3A%2F%2Fi.ytimg.com%2Fx.jpg",
        "/proxy/video/dQw4w9WgXcQ/137",
    ] {
        let res = app.get(&mut guest, path).await;
        assert_eq!(res.status, StatusCode::UNAUTHORIZED, "{path}");
    }
    let res = app.get(&mut guest, "/api/v1/auth/config").await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["needs_setup"], true);
    assert_eq!(res.body["passkeys_available"], true);
    assert_eq!(res.body["registration_mode"], "invite");
    assert_eq!(
        app.get(&mut guest, "/api/v1/openapi.json").await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn first_user_is_admin_then_invites_are_required() {
    let app = App::new().await;
    let (mut admin, res) = app.register_password("admin", None).await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    assert_eq!(res.body["user"]["is_admin"], true);
    assert_eq!(res.body["recovery_codes"].as_array().unwrap().len(), 10);
    assert_eq!(
        app.get(&mut admin, "/api/v1/search?q=rust").await.status,
        StatusCode::OK
    );

    let (_, res) = app.register_password("mallory", None).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);

    let invite = app.invite(&mut admin).await;
    let mut guest = Client::default();
    let cfg = app
        .get(&mut guest, &format!("/api/v1/auth/config?invite={invite}"))
        .await;
    assert_eq!(cfg.body["invite_valid"], true);

    let (mut bob, res) = app.register_password("bob", Some(&invite)).await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    assert_eq!(res.body["user"]["is_admin"], false);
    assert_eq!(
        app.get(&mut bob, "/api/v1/admin/users").await.status,
        StatusCode::FORBIDDEN
    );

    let (_, res) = app.register_password("carol", Some(&invite)).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN, "invite is single-use");
    let (_, res) = app.register_password("BOB", None).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN, "still needs an invite");
}

#[tokio::test]
async fn usernames_are_unique_and_passwords_checked() {
    let app = App::new().await;
    let (mut admin, _) = app.register_password("admin", None).await;
    let invite = app.invite(&mut admin).await;
    let (_, res) = app.register_password("Admin", Some(&invite)).await;
    assert_eq!(res.status, StatusCode::CONFLICT);

    let mut c = Client::default();
    let res = app
        .post(
            &mut c,
            "/api/v1/auth/register/password",
            json!({ "username": "dave", "password": "short", "invite": invite }),
        )
        .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn password_login_and_logout() {
    let app = App::new().await;
    app.register_password("admin", None).await;
    let mut c = Client::default();
    let bad = app
        .post(
            &mut c,
            "/api/v1/auth/login/password",
            json!({ "username": "admin", "password": "wrong password!" }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
    let unknown = app
        .post(
            &mut c,
            "/api/v1/auth/login/password",
            json!({ "username": "nobody", "password": "wrong password!" }),
        )
        .await;
    assert_eq!(
        unknown.body["message"], bad.body["message"],
        "same message for unknown users"
    );

    let ok = app
        .post(
            &mut c,
            "/api/v1/auth/login/password",
            json!({ "username": "ADMIN", "password": "correct horse battery" }),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    assert_eq!(
        app.get(&mut c, "/api/v1/me").await.body["username"],
        "admin"
    );

    assert_eq!(
        app.post(&mut c, "/api/v1/auth/logout", json!({}))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert!(c.cookie.is_none());
    assert_eq!(
        app.get(&mut c, "/api/v1/me").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn cross_site_writes_are_rejected() {
    let app = App::new().await;
    let mut c = Client::default();
    let body = json!({ "username": "admin", "password": "correct horse battery" });
    let res = app
        .call_with(
            &mut c,
            Method::POST,
            "/api/v1/auth/register/password",
            Some(body.clone()),
            &[("origin", "https://evil.example")],
        )
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    let res = app
        .call_with(
            &mut c,
            Method::POST,
            "/api/v1/auth/register/password",
            Some(body),
            &[
                ("origin", "http://localhost:3000"),
                ("sec-fetch-site", "same-origin"),
            ],
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED);
}

#[tokio::test]
async fn passkey_signup_login_and_reauth() {
    let app = App::new().await;
    let (mut c, mut auth, res) = signup_with_passkey(&app, "alice", None).await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    assert_eq!(res.body["user"]["passkey_count"], 1);
    assert_eq!(res.body["user"]["has_password"], false);

    let keys = app.get(&mut c, "/api/v1/me/passkeys").await;
    assert_eq!(keys.body[0]["name"], "Soft key");

    let (mut c2, res) = login_with_passkey(&app, &mut auth).await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["user"]["username"], "alice");
    assert!(app.get(&mut c2, "/api/v1/me/passkeys").await.body[0]["last_used_at"].is_i64());

    // The only credential can't be removed, even right after logging in.
    let id = keys.body[0]["id"].as_i64().unwrap();
    let del = app
        .call(
            &mut c2,
            Method::DELETE,
            &format!("/api/v1/me/passkeys/{id}"),
            None,
        )
        .await;
    assert_eq!(del.status, StatusCode::CONFLICT);

    // Sensitive actions need a recent re-authentication.
    app.expire_reauth().await;
    let res = app
        .post(&mut c2, "/api/v1/me/recovery-codes", json!({}))
        .await;
    assert_eq!(res.body["error"], "reauth_required");
    // Adding a passkey too: otherwise a stolen session could add one and stay in.
    let res = app
        .post(&mut c2, "/api/v1/me/passkeys/start", json!({}))
        .await;
    assert_eq!(res.body["error"], "reauth_required");
    let start = app
        .post(&mut c2, "/api/v1/auth/reauth/passkey/start", json!({}))
        .await;
    let cred = auth.authenticate(&app.origin, &start.body["options"]);
    let fin = app
        .post(
            &mut c2,
            "/api/v1/auth/reauth/passkey/finish",
            json!({ "challenge_id": start.body["challenge_id"], "credential": cred }),
        )
        .await;
    assert_eq!(fin.status, StatusCode::NO_CONTENT, "{:?}", fin.body);
    let res = app
        .post(&mut c2, "/api/v1/me/recovery-codes", json!({}))
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["recovery_codes"].as_array().unwrap().len(), 10);
}

#[tokio::test]
async fn passkey_challenges_are_single_use() {
    let app = App::new().await;
    let (_, mut auth, _) = signup_with_passkey(&app, "alice", None).await;
    let mut c = Client::default();
    let start = app
        .post(&mut c, "/api/v1/auth/login/passkey/start", json!({}))
        .await;
    let cred = auth.authenticate(&app.origin, &start.body["options"]);
    let body = json!({ "challenge_id": start.body["challenge_id"], "credential": cred });
    assert_eq!(
        app.post(&mut c, "/api/v1/auth/login/passkey/finish", body.clone())
            .await
            .status,
        StatusCode::OK
    );
    let replay = app
        .post(
            &mut Client::default(),
            "/api/v1/auth/login/passkey/finish",
            body,
        )
        .await;
    assert_eq!(replay.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn recovery_codes_work_once() {
    let app = App::new().await;
    let (_, res) = app.register_password("admin", None).await;
    let code = res.body["recovery_codes"][0]
        .as_str()
        .unwrap()
        .to_lowercase();
    let mut c = Client::default();
    let body = json!({ "username": "admin", "code": code });
    let ok = app
        .post(&mut c, "/api/v1/auth/login/recovery", body.clone())
        .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    assert_eq!(ok.body["used_recovery_code"], true);
    assert_eq!(ok.body["user"]["recovery_codes_remaining"], 9);
    let again = app
        .post(&mut Client::default(), "/api/v1/auth/login/recovery", body)
        .await;
    assert_eq!(again.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_created_user_sets_up_via_reset_link() {
    let app = App::new().await;
    let (mut admin, _) = app.register_password("admin", None).await;
    let res = app
        .call(
            &mut admin,
            Method::PATCH,
            "/api/v1/admin/settings",
            Some(json!({ "registration_mode": "closed" })),
        )
        .await;
    assert_eq!(res.body["registration_mode"], "closed");

    let created = app
        .post(
            &mut admin,
            "/api/v1/admin/users",
            json!({ "username": "erin" }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let token = token_from(&created.body["setup_link"]["url"]);

    let mut guest = Client::default();
    let info = app
        .get(&mut guest, &format!("/api/v1/auth/reset/{token}"))
        .await;
    assert_eq!(info.body["username"], "erin");

    // Passkey setup through the reset link.
    let mut auth = Authenticator::new();
    let start = app
        .post(
            &mut guest,
            &format!("/api/v1/auth/reset/{token}/passkey/start"),
            json!({}),
        )
        .await;
    assert_eq!(start.status, StatusCode::OK, "{:?}", start.body);
    let cred = auth.register(&app.origin, &start.body["options"]);
    let fin = app
        .post(
            &mut guest,
            &format!("/api/v1/auth/reset/{token}/passkey/finish"),
            json!({ "challenge_id": start.body["challenge_id"], "credential": cred }),
        )
        .await;
    assert_eq!(fin.status, StatusCode::OK, "{:?}", fin.body);
    assert_eq!(fin.body["user"]["username"], "erin");
    assert_eq!(fin.body["user"]["passkey_count"], 1);

    // The link is used up; the new passkey logs in.
    assert_eq!(
        app.get(
            &mut Client::default(),
            &format!("/api/v1/auth/reset/{token}")
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let (_, res) = login_with_passkey(&app, &mut auth).await;
    assert_eq!(res.status, StatusCode::OK);
}

#[tokio::test]
async fn admin_account_changes_need_reauth() {
    let app = App::new().await;
    let (mut admin, _) = app.register_password("admin", None).await;
    let admin_id = app.get(&mut admin, "/api/v1/me").await.body["id"]
        .as_i64()
        .unwrap();
    app.expire_reauth().await;

    let reset_link = format!("/api/v1/admin/users/{admin_id}/reset-link");
    let res = app.post(&mut admin, &reset_link, json!({})).await;
    assert_eq!(res.body["error"], "reauth_required");
    let res = app
        .post(
            &mut admin,
            "/api/v1/admin/users",
            json!({ "username": "mallory", "is_admin": true }),
        )
        .await;
    assert_eq!(res.body["error"], "reauth_required");
    let res = app
        .call(
            &mut admin,
            Method::PATCH,
            "/api/v1/admin/settings",
            Some(json!({ "registration_mode": "open" })),
        )
        .await;
    assert_eq!(res.body["error"], "reauth_required");
    // Read-only admin pages and invites still work.
    assert_eq!(
        app.get(&mut admin, "/api/v1/admin/users").await.status,
        StatusCode::OK
    );
    app.invite(&mut admin).await;

    let res = app
        .post(
            &mut admin,
            "/api/v1/auth/reauth/password",
            json!({ "password": "correct horse battery" }),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT, "{:?}", res.body);
    let res = app.post(&mut admin, &reset_link, json!({})).await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
}

#[tokio::test]
async fn responses_have_security_headers() {
    let app = App::new().await;
    let req = Request::builder()
        .uri("/api/v1/auth/config")
        .body(Body::empty())
        .unwrap();
    let res = app.router.clone().oneshot(req).await.unwrap();
    let h = res.headers();
    let csp = h[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert!(!csp.contains("script-src 'self' 'unsafe-inline'"), "{csp}");
    assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(h[header::X_FRAME_OPTIONS], "DENY");
}

#[tokio::test]
async fn reset_with_password_signs_out_everywhere() {
    let app = App::new().await;
    let (mut admin, _) = app.register_password("admin", None).await;
    let invite = app.invite(&mut admin).await;
    let (mut frank, _) = app.register_password("frank", Some(&invite)).await;
    let frank_id = app.get(&mut frank, "/api/v1/me").await.body["id"]
        .as_i64()
        .unwrap();

    let link = app
        .post(
            &mut admin,
            &format!("/api/v1/admin/users/{frank_id}/reset-link"),
            json!({}),
        )
        .await;
    let token = token_from(&link.body["url"]);
    let mut guest = Client::default();
    let res = app
        .post(
            &mut guest,
            &format!("/api/v1/auth/reset/{token}/password"),
            json!({ "password": "a brand new passphrase" }),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(
        app.get(&mut frank, "/api/v1/me").await.status,
        StatusCode::UNAUTHORIZED,
        "old session revoked"
    );
    assert_eq!(
        app.get(&mut guest, "/api/v1/me").await.body["username"],
        "frank"
    );
}

#[tokio::test]
async fn disabling_a_user_ends_their_sessions() {
    let app = App::new().await;
    let (mut admin, _) = app.register_password("admin", None).await;
    let invite = app.invite(&mut admin).await;
    let (mut gina, _) = app.register_password("gina", Some(&invite)).await;
    let id = app.get(&mut gina, "/api/v1/me").await.body["id"]
        .as_i64()
        .unwrap();

    let res = app
        .call(
            &mut admin,
            Method::PATCH,
            &format!("/api/v1/admin/users/{id}"),
            Some(json!({ "disabled": true })),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.get(&mut gina, "/api/v1/me").await.status,
        StatusCode::UNAUTHORIZED
    );
    let mut c = Client::default();
    let login = app
        .post(
            &mut c,
            "/api/v1/auth/login/password",
            json!({ "username": "gina", "password": "correct horse battery" }),
        )
        .await;
    assert_eq!(login.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn last_admin_is_protected() {
    let app = App::new().await;
    let (mut admin, _) = app.register_password("admin", None).await;
    let id = app.get(&mut admin, "/api/v1/me").await.body["id"]
        .as_i64()
        .unwrap();
    let res = app
        .call(
            &mut admin,
            Method::PATCH,
            &format!("/api/v1/admin/users/{id}"),
            Some(json!({ "is_admin": false })),
        )
        .await;
    assert_eq!(res.status, StatusCode::CONFLICT);
    let res = app
        .call(&mut admin, Method::DELETE, "/api/v1/me", None)
        .await;
    assert_eq!(res.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn passwords_cannot_be_disabled_while_someone_needs_them() {
    let app = App::new().await;
    let (mut admin, _) = app.register_password("admin", None).await;
    let res = app
        .call(
            &mut admin,
            Method::PATCH,
            "/api/v1/admin/settings",
            Some(json!({ "passwords_enabled": false })),
        )
        .await;
    assert_eq!(res.status, StatusCode::CONFLICT);
    assert!(res.body["message"].as_str().unwrap().contains("admin"));
}

#[tokio::test]
async fn passwords_disabled_blocks_password_login() {
    let app = App::new().await;
    let (mut alice, _, _) = signup_with_passkey(&app, "alice", None).await;
    let res = app
        .call(
            &mut alice,
            Method::PATCH,
            "/api/v1/admin/settings",
            Some(json!({ "passwords_enabled": false })),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    let mut c = Client::default();
    let login = app
        .post(
            &mut c,
            "/api/v1/auth/login/password",
            json!({ "username": "alice", "password": "whatever whatever" }),
        )
        .await;
    assert_eq!(login.status, StatusCode::FORBIDDEN);
    assert_eq!(
        app.get(&mut c, "/api/v1/auth/config").await.body["passwords_enabled"],
        false
    );
}

#[tokio::test]
async fn without_https_domain_passwords_are_forced_on() {
    let app = App::with_url("http://192.168.1.10:3000").await;
    let mut c = Client::default();
    let cfg = app.get(&mut c, "/api/v1/auth/config").await;
    assert_eq!(cfg.body["passkeys_available"], false);
    assert_eq!(cfg.body["passwords_enabled"], true);
    let res = app
        .post(
            &mut c,
            "/api/v1/auth/register/passkey/start",
            json!({ "username": "x" }),
        )
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);

    let (mut admin, res) = app.register_password("admin", None).await;
    assert_eq!(res.status, StatusCode::CREATED);
    let res = app
        .call(
            &mut admin,
            Method::PATCH,
            "/api/v1/admin/settings",
            Some(json!({ "passwords_enabled": false })),
        )
        .await;
    assert_eq!(res.status, StatusCode::CONFLICT);
    let s = app.get(&mut admin, "/api/v1/admin/settings").await;
    assert!(
        s.body["passkeys_unavailable_reason"]
            .as_str()
            .unwrap()
            .contains("HTTPS")
    );
}

#[tokio::test]
async fn sessions_can_be_listed_and_revoked() {
    let app = App::new().await;
    let (mut a, _) = app.register_password("admin", None).await;
    let mut b = Client::default();
    app.post(
        &mut b,
        "/api/v1/auth/login/password",
        json!({ "username": "admin", "password": "correct horse battery" }),
    )
    .await;
    let list = app.get(&mut a, "/api/v1/me/sessions").await;
    let list = list.body.as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list.iter().filter(|s| s["current"] == true).count(), 1);

    assert_eq!(
        app.call(&mut a, Method::DELETE, "/api/v1/me/sessions", None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.get(&mut b, "/api/v1/me").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(app.get(&mut a, "/api/v1/me").await.status, StatusCode::OK);
}

#[tokio::test]
async fn settings_are_stored_per_user() {
    let app = App::new().await;
    let (mut a, _) = app.register_password("admin", None).await;
    let res = app
        .call(
            &mut a,
            Method::PUT,
            "/api/v1/me/settings",
            Some(json!({ "theme": "dark", "density": "compact" })),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(
        app.get(&mut a, "/api/v1/me").await.body["settings"]["theme"],
        "dark"
    );
    let res = app
        .call(
            &mut a,
            Method::PUT,
            "/api/v1/me/settings",
            Some(json!([1, 2])),
        )
        .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn watch_progress_and_continue_watching() {
    let app = App::new().await;
    let (mut a, _) = app.register_password("admin", None).await;
    let save = |id: &str, pos: f64| {
        (
            format!("/api/v1/progress/{id}"),
            json!({ "position_s": pos, "duration_s": 600.0 }),
        )
    };

    let (path, body) = save("aaaaaaaaaaa", 120.0);
    let res = app.call(&mut a, Method::PUT, &path, Some(body)).await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["finished"], false);
    let (path, body) = save("bbbbbbbbbbb", 580.0);
    assert_eq!(
        app.call(&mut a, Method::PUT, &path, Some(body)).await.body["finished"],
        true
    );
    let (path, body) = save("ccccccccccc", 5.0);
    app.call(&mut a, Method::PUT, &path, Some(body)).await;

    let cw = app.get(&mut a, "/api/v1/continue-watching").await;
    let items = cw.body.as_array().unwrap();
    assert_eq!(
        items.len(),
        1,
        "finished and barely-started videos are left out"
    );
    assert_eq!(items[0]["video"]["id"], "aaaaaaaaaaa");
    assert_eq!(items[0]["video"]["title"], "Video aaaaaaaaaaa");
    assert_eq!(items[0]["video"]["channel"]["name"], "Fake channel");
    assert!(
        items[0]["video"]["thumbnail_url"]
            .as_str()
            .unwrap()
            .starts_with("/proxy/img?u=")
    );
    assert_eq!(items[0]["position_s"], 120.0);

    let batch = app
        .get(
            &mut a,
            "/api/v1/progress?ids=aaaaaaaaaaa,bbbbbbbbbbb,zzzzzzzzzzz,bad",
        )
        .await;
    assert_eq!(batch.body.as_array().unwrap().len(), 2);

    // Progress is per user.
    let invite = app.invite(&mut a).await;
    let (mut b, _) = app.register_password("bob", Some(&invite)).await;
    assert_eq!(
        app.get(&mut b, "/api/v1/continue-watching").await.body,
        json!([])
    );

    let del = app
        .call(&mut a, Method::DELETE, "/api/v1/progress/aaaaaaaaaaa", None)
        .await;
    assert_eq!(del.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.get(&mut a, "/api/v1/continue-watching").await.body,
        json!([])
    );
}

#[tokio::test]
async fn progress_input_is_validated() {
    let app = App::new().await;
    let (mut a, _) = app.register_password("admin", None).await;
    for (path, body) in [
        (
            "/api/v1/progress/short",
            json!({ "position_s": 1.0, "duration_s": 10.0 }),
        ),
        (
            "/api/v1/progress/aaaaaaaaaaa",
            json!({ "position_s": -1.0, "duration_s": 10.0 }),
        ),
        (
            "/api/v1/progress/aaaaaaaaaaa",
            json!({ "position_s": 1.0, "duration_s": 0.0 }),
        ),
    ] {
        let res = app.call(&mut a, Method::PUT, path, Some(body)).await;
        assert_eq!(res.status, StatusCode::BAD_REQUEST, "{path}");
    }
    // Positions past the end are clamped.
    let res = app
        .call(
            &mut a,
            Method::PUT,
            "/api/v1/progress/aaaaaaaaaaa",
            Some(json!({ "position_s": 999.0, "duration_s": 600.0 })),
        )
        .await;
    assert_eq!(res.body["position_s"], 600.0);
    let mut guest = Client::default();
    assert_eq!(
        app.get(&mut guest, "/api/v1/continue-watching")
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.get(&mut a, "/api/v1/channels/not-a-channel")
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn subscriptions_and_feed() {
    let app = App::new().await;
    let (mut a, _) = app.register_password("admin", None).await;

    let res = app
        .post(
            &mut a,
            "/api/v1/subscriptions",
            json!({ "channel_id": FAKE_CHANNEL }),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    assert_eq!(res.body["name"], "Fake channel");
    assert!(
        res.body["avatar_url"]
            .as_str()
            .unwrap()
            .starts_with("/proxy/img")
    );
    assert!(res.body["fetched_at"].is_null(), "not refreshed yet");
    let again = app
        .post(
            &mut a,
            "/api/v1/subscriptions",
            json!({ "channel_id": FAKE_CHANNEL }),
        )
        .await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(
        app.get(&mut a, "/api/v1/subscriptions/ids").await.body,
        json!([FAKE_CHANNEL])
    );

    // The refresher polls the channel, stores uploads and enriches them.
    let mut events = app.state.feed_events.subscribe();
    assert_eq!(
        katsanto_app::refresh::run_once(&app.state).await.unwrap(),
        1
    );
    let ev = events.try_recv().unwrap();
    assert_eq!(ev.video_ids.len(), 3);
    assert_eq!(
        katsanto_app::refresh::run_once(&app.state).await.unwrap(),
        0,
        "not due again yet"
    );

    let feed = app.get(&mut a, "/api/v1/feed").await;
    let items = feed.body["items"].as_array().unwrap();
    let ids: Vec<&str> = items
        .iter()
        .map(|i| i["video"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["video2aaaaa", "video1aaaaa"],
        "newest first, shorts hidden"
    );
    assert_eq!(
        items[1]["video"]["duration_s"], 321,
        "duration from the channel page"
    );
    assert_eq!(items[1]["video"]["channel"]["name"], "Fake channel");
    let with_shorts = app.get(&mut a, "/api/v1/feed?shorts=true").await;
    let shorts: Vec<_> = with_shorts.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["video"]["is_short"] == true)
        .collect();
    assert_eq!(shorts.len(), 1);

    // Finished videos can be hidden.
    app.call(
        &mut a,
        Method::PUT,
        "/api/v1/progress/video1aaaaa",
        Some(json!({ "position_s": 599.0, "duration_s": 600.0 })),
    )
    .await;
    let feed = app.get(&mut a, "/api/v1/feed").await;
    assert_eq!(feed.body["items"][1]["progress"]["finished"], true);
    let unwatched = app.get(&mut a, "/api/v1/feed?watched=false").await;
    assert_eq!(unwatched.body["items"].as_array().unwrap().len(), 1);

    let subs = app.get(&mut a, "/api/v1/subscriptions").await;
    assert!(subs.body[0]["fetched_at"].is_i64());
    assert!(subs.body[0]["last_video_at"].is_i64());

    // Other users don't see it; unsubscribing empties the feed.
    let invite = app.invite(&mut a).await;
    let (mut b, _) = app.register_password("bob", Some(&invite)).await;
    assert_eq!(
        app.get(&mut b, "/api/v1/feed").await.body["items"],
        json!([])
    );
    let del = app
        .call(
            &mut a,
            Method::DELETE,
            &format!("/api/v1/subscriptions/{FAKE_CHANNEL}"),
            None,
        )
        .await;
    assert_eq!(del.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.get(&mut a, "/api/v1/feed").await.body["items"],
        json!([])
    );
}

#[tokio::test]
async fn subscribe_validates_channels() {
    let app = App::new().await;
    let (mut a, _) = app.register_password("admin", None).await;
    let bad = app
        .post(
            &mut a,
            "/api/v1/subscriptions",
            json!({ "channel_id": "nope" }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let unknown = app
        .post(
            &mut a,
            "/api/v1/subscriptions",
            json!({ "channel_id": "UCbbbbbbbbbbbbbbbbbbbbbb" }),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    let res = app
        .post(
            &mut a,
            "/api/v1/subscriptions/remove",
            json!({ "channel_ids": ["UCx"] }),
        )
        .await;
    assert_eq!(res.body["removed"], 0);
    assert_eq!(
        app.get(&mut a, "/api/v1/feed?cursor=garbage").await.status,
        StatusCode::BAD_REQUEST
    );
}

/// Subscribe to the fake channel and run the refresher once.
async fn subscribed_feed(app: &App, c: &mut Client) {
    app.post(
        c,
        "/api/v1/subscriptions",
        json!({ "channel_id": FAKE_CHANNEL }),
    )
    .await;
    katsanto_app::refresh::run_once(&app.state).await.unwrap();
}

fn ids(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["video"]["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn tags_and_saved_feeds() {
    let app = App::new().await;
    let (mut a, _) = app.register_password("admin", None).await;
    subscribed_feed(&app, &mut a).await;

    // Tags.
    let tag = app
        .post(
            &mut a,
            "/api/v1/tags",
            json!({ "name": "Programming", "color": "blue" }),
        )
        .await;
    assert_eq!(tag.status, StatusCode::CREATED, "{:?}", tag.body);
    let tag_id = tag.body["id"].as_i64().unwrap();
    let dup = app
        .post(&mut a, "/api/v1/tags", json!({ "name": "programming" }))
        .await;
    assert_eq!(dup.status, StatusCode::CONFLICT);
    let bad = app
        .post(
            &mut a,
            "/api/v1/tags",
            json!({ "name": "x", "color": "plaid" }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);

    // A feed for the tag is empty until the channel is tagged.
    let body = json!({ "name": "Programming", "icon": "💻", "filter": { "tag_ids": [tag_id] } });
    let feed = app.post(&mut a, "/api/v1/feeds", body).await;
    assert_eq!(feed.status, StatusCode::CREATED, "{:?}", feed.body);
    let feed_id = feed.body["id"].as_i64().unwrap();
    assert_eq!(feed.body["filter"]["live"], true, "defaults filled in");
    let feed_path = format!("/api/v1/feeds/{feed_id}/videos");
    assert!(ids(&app.get(&mut a, &feed_path).await.body).is_empty());

    let res = app
        .call(
            &mut a,
            Method::PUT,
            &format!("/api/v1/subscriptions/{FAKE_CHANNEL}/tags"),
            Some(json!({ "tag_ids": [tag_id] })),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.get(&mut a, "/api/v1/subscriptions").await.body[0]["tag_ids"],
        json!([tag_id])
    );
    assert_eq!(
        ids(&app.get(&mut a, &feed_path).await.body),
        ["video2aaaaa", "video1aaaaa"]
    );

    // Title filter (in Rust) and update.
    let body = json!({ "name": "Video 1", "icon": "", "filter": { "tag_ids": [tag_id], "title_any": ["VIDEO1"] } });
    let upd = app
        .call(
            &mut a,
            Method::PUT,
            &format!("/api/v1/feeds/{feed_id}"),
            Some(body),
        )
        .await;
    assert_eq!(upd.status, StatusCode::OK, "{:?}", upd.body);
    assert_eq!(
        ids(&app.get(&mut a, &feed_path).await.body),
        ["video1aaaaa"]
    );

    // Preview of an unsaved filter.
    let prev = app
        .post(
            &mut a,
            "/api/v1/feeds/preview",
            json!({ "filter": { "shorts": true, "title_none": ["video"] } }),
        )
        .await;
    assert_eq!(ids(&prev.body), ["short1aaaaa"]);
    let bad = app
        .post(
            &mut a,
            "/api/v1/feeds/preview",
            json!({ "filter": { "title_regex": "(" } }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);

    // Order and delete.
    let second = app
        .post(
            &mut a,
            "/api/v1/feeds",
            json!({ "name": "Second", "filter": {} }),
        )
        .await;
    let second_id = second.body["id"].as_i64().unwrap();
    app.call(
        &mut a,
        Method::PUT,
        "/api/v1/feeds/order",
        Some(json!({ "ids": [second_id, feed_id] })),
    )
    .await;
    let list = app.get(&mut a, "/api/v1/feeds").await;
    assert_eq!(list.body[0]["name"], "Second");
    assert_eq!(
        app.call(
            &mut a,
            Method::DELETE,
            &format!("/api/v1/feeds/{second_id}"),
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );

    // Feeds and tags are private.
    let invite = app.invite(&mut a).await;
    let (mut b, _) = app.register_password("bob", Some(&invite)).await;
    assert_eq!(
        app.get(&mut b, &format!("/api/v1/feeds/{feed_id}/videos"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let res = app
        .post(
            &mut b,
            &format!("/api/v1/tags/{tag_id}/channels"),
            json!({ "channel_ids": [], "add": true }),
        )
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);

    // Deleting the tag removes it from channels.
    assert_eq!(
        app.call(
            &mut a,
            Method::DELETE,
            &format!("/api/v1/tags/{tag_id}"),
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.get(&mut a, "/api/v1/subscriptions").await.body[0]["tag_ids"],
        json!([])
    );
}

#[tokio::test]
async fn hide_and_mark_watched() {
    let app = App::new().await;
    let (mut a, _) = app.register_password("admin", None).await;
    subscribed_feed(&app, &mut a).await;
    assert_eq!(
        ids(&app.get(&mut a, "/api/v1/feed").await.body),
        ["video2aaaaa", "video1aaaaa"]
    );

    let res = app
        .post(&mut a, "/api/v1/videos/video2aaaaa/hide", json!({}))
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    assert_eq!(
        ids(&app.get(&mut a, "/api/v1/feed").await.body),
        ["video1aaaaa"]
    );
    app.call(
        &mut a,
        Method::DELETE,
        "/api/v1/videos/video2aaaaa/hide",
        None,
    )
    .await;
    assert_eq!(ids(&app.get(&mut a, "/api/v1/feed").await.body).len(), 2);

    let w = app
        .post(&mut a, "/api/v1/videos/video1aaaaa/watched", json!({}))
        .await;
    assert_eq!(w.status, StatusCode::OK, "{:?}", w.body);
    assert_eq!(w.body["finished"], true);
    assert_eq!(
        w.body["position_s"], 321.0,
        "duration known from enrichment"
    );
    let unwatched = app.get(&mut a, "/api/v1/feed?watched=false").await;
    assert_eq!(ids(&unwatched.body), ["video2aaaaa"]);
}

fn list_ids(detail: &Value) -> Vec<String> {
    detail["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["video"]["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn watch_lists() {
    let app = App::new().await;
    let (mut a, _) = app.register_password("admin", None).await;

    // Watch later exists from the start.
    let lists = app.get(&mut a, "/api/v1/lists").await;
    assert_eq!(lists.body.as_array().unwrap().len(), 1);
    assert_eq!(lists.body[0]["kind"], "watch_later");
    assert_eq!(lists.body[0]["remove_watched"], true);
    let wl = lists.body[0]["id"].as_i64().unwrap();

    let created = app
        .post(
            &mut a,
            "/api/v1/lists",
            json!({ "name": "Weekend", "description": "Long ones" }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let weekend = created.body["id"].as_i64().unwrap();

    // Adding stores the video (fake playback) and appends.
    for id in ["aaaaaaaaaaa", "bbbbbbbbbbb", "ccccccccccc"] {
        let res = app
            .post(
                &mut a,
                &format!("/api/v1/lists/{weekend}/items"),
                json!({ "video_id": id }),
            )
            .await;
        assert_eq!(res.body["added"], true, "{:?}", res.body);
    }
    let again = app
        .post(
            &mut a,
            &format!("/api/v1/lists/{weekend}/items"),
            json!({ "video_id": "aaaaaaaaaaa" }),
        )
        .await;
    assert_eq!(again.body["added"], false);
    let detail = app.get(&mut a, &format!("/api/v1/lists/{weekend}")).await;
    assert_eq!(
        list_ids(&detail.body),
        ["aaaaaaaaaaa", "bbbbbbbbbbb", "ccccccccccc"]
    );
    assert_eq!(detail.body["list"]["item_count"], 3);
    assert_eq!(detail.body["list"]["total_duration_s"], 1800);
    assert_eq!(
        detail.body["items"][0]["video"]["title"],
        "Video aaaaaaaaaaa"
    );

    // Reorder and remove.
    let mv = app
        .call(
            &mut a,
            Method::PUT,
            &format!("/api/v1/lists/{weekend}/items/ccccccccccc/position"),
            Some(json!({ "after": null })),
        )
        .await;
    assert_eq!(mv.status, StatusCode::NO_CONTENT);
    app.call(
        &mut a,
        Method::PUT,
        &format!("/api/v1/lists/{weekend}/items/aaaaaaaaaaa/position"),
        Some(json!({ "after": "bbbbbbbbbbb" })),
    )
    .await;
    let detail = app.get(&mut a, &format!("/api/v1/lists/{weekend}")).await;
    assert_eq!(
        list_ids(&detail.body),
        ["ccccccccccc", "bbbbbbbbbbb", "aaaaaaaaaaa"]
    );
    let rm = app
        .call(
            &mut a,
            Method::DELETE,
            &format!("/api/v1/lists/{weekend}/items/bbbbbbbbbbb"),
            None,
        )
        .await;
    assert_eq!(rm.status, StatusCode::NO_CONTENT);

    // Membership for the "save to list" menu.
    app.post(
        &mut a,
        &format!("/api/v1/lists/{wl}/items"),
        json!({ "video_id": "aaaaaaaaaaa" }),
    )
    .await;
    let menu = app.get(&mut a, "/api/v1/lists?contains=aaaaaaaaaaa").await;
    assert!(
        menu.body
            .as_array()
            .unwrap()
            .iter()
            .all(|l| l["contains"] == true)
    );

    // Finishing a video drops it from Watch later only.
    app.call(
        &mut a,
        Method::PUT,
        "/api/v1/progress/aaaaaaaaaaa",
        Some(json!({ "position_s": 599.0, "duration_s": 600.0 })),
    )
    .await;
    assert!(list_ids(&app.get(&mut a, &format!("/api/v1/lists/{wl}")).await.body).is_empty());
    assert_eq!(
        list_ids(
            &app.get(&mut a, &format!("/api/v1/lists/{weekend}"))
                .await
                .body
        ),
        ["ccccccccccc", "aaaaaaaaaaa"]
    );
    let detail = app.get(&mut a, &format!("/api/v1/lists/{weekend}")).await;
    assert_eq!(detail.body["items"][1]["progress"]["finished"], true);

    // Settings, privacy, deletion.
    let upd = app
        .call(
            &mut a,
            Method::PUT,
            &format!("/api/v1/lists/{weekend}"),
            Some(json!({ "name": "Weekend!", "remove_watched": true })),
        )
        .await;
    assert_eq!(upd.body["name"], "Weekend!");
    assert_eq!(upd.body["remove_watched"], true);
    let invite = app.invite(&mut a).await;
    let (mut b, _) = app.register_password("bob", Some(&invite)).await;
    assert_eq!(
        app.get(&mut b, &format!("/api/v1/lists/{weekend}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let res = app
        .post(
            &mut b,
            &format!("/api/v1/lists/{weekend}/items"),
            json!({ "video_id": "ddddddddddd" }),
        )
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert_eq!(
        app.call(&mut a, Method::DELETE, &format!("/api/v1/lists/{wl}"), None)
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.call(
            &mut a,
            Method::DELETE,
            &format!("/api/v1/lists/{weekend}"),
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let bad = app
        .post(&mut a, "/api/v1/lists", json!({ "name": "  " }))
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn backup_moves_an_account() {
    let app = App::new().await;
    let (mut a, _) = app.register_password("admin", None).await;
    let res = app
        .post(
            &mut a,
            "/api/v1/subscriptions",
            json!({ "channel_id": FAKE_CHANNEL }),
        )
        .await;
    assert!(res.status.is_success(), "{:?}", res.body);
    let weekend = app
        .post(
            &mut a,
            "/api/v1/lists",
            json!({ "name": "Weekend", "description": "" }),
        )
        .await
        .body["id"]
        .as_i64()
        .unwrap();
    for id in ["aaaaaaaaaaa", "bbbbbbbbbbb"] {
        app.post(
            &mut a,
            &format!("/api/v1/lists/{weekend}/items"),
            json!({ "video_id": id }),
        )
        .await;
    }

    let file = app.get(&mut a, "/api/v1/me/backup").await;
    assert_eq!(file.status, StatusCode::OK);
    assert_eq!(file.body["format"], "katsanto-backup");
    assert_eq!(file.body["videos"].as_array().unwrap().len(), 2);

    let token = app.invite(&mut a).await;
    let (mut b, _) = app.register_password("bob", Some(&token)).await;
    let res = app
        .post(&mut b, "/api/v1/me/backup", file.body.clone())
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["subscriptions"], 1);
    assert_eq!(res.body["list_items"], 2);
    let again = app
        .post(&mut b, "/api/v1/me/backup", file.body.clone())
        .await;
    assert_eq!(
        again.body["list_items"], 0,
        "importing twice changes nothing"
    );

    let lists = app.get(&mut b, "/api/v1/lists").await;
    let id = lists
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "Weekend")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let detail = app.get(&mut b, &format!("/api/v1/lists/{id}")).await;
    assert_eq!(list_ids(&detail.body), ["aaaaaaaaaaa", "bbbbbbbbbbb"]);
    let subs = app.get(&mut b, "/api/v1/subscriptions").await;
    assert_eq!(subs.body[0]["channel_id"], FAKE_CHANNEL);

    let bad = app
        .post(
            &mut b,
            "/api/v1/me/backup",
            json!({ "format": "other", "version": 1 }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn community_features_can_be_off() {
    let app = App::new().await;
    let (mut a, _) = app.register_password("admin", None).await;
    let f = app.get(&mut a, "/api/v1/features").await;
    assert_eq!(f.body["sponsorblock"], false);
    assert_eq!(f.body["dearrow_thumbnails"], false);
    let s = app.get(&mut a, "/api/v1/videos/aaaaaaaaaaa/segments").await;
    assert_eq!(s.body, json!([]));
    let bad = app.get(&mut a, "/api/v1/videos/nope/segments").await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let b = app
        .post(
            &mut a,
            "/api/v1/branding",
            json!({ "ids": ["aaaaaaaaaaa"] }),
        )
        .await;
    assert_eq!(b.body, json!({}));
    let mut guest = Client::default();
    let g = app
        .get(&mut guest, "/api/v1/videos/aaaaaaaaaaa/segments")
        .await;
    assert_eq!(g.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn websub_callbacks() {
    let app = App::new().await;
    let (mut a, _) = app.register_password("admin", None).await;
    app.post(
        &mut a,
        "/api/v1/subscriptions",
        json!({ "channel_id": FAKE_CHANNEL }),
    )
    .await;
    katsanto_db::websub::requested(app.db.pool(), FAKE_CHANNEL, "test-secret")
        .await
        .unwrap();
    let topic = format!(
        "https%3A%2F%2Fwww.youtube.com%2Fxml%2Ffeeds%2Fvideos.xml%3Fchannel_id%3D{FAKE_CHANNEL}"
    );
    let raw = |method: Method, path: String, body: &'static str, sig: Option<&str>| {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(sig) = sig {
            req = req.header("x-hub-signature", sig);
        }
        let req = req.body(Body::from(body)).unwrap();
        let router = app.router.clone();
        async move {
            let res = router.oneshot(req).await.unwrap();
            let status = res.status();
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            (status, String::from_utf8_lossy(&bytes).into_owned())
        }
    };

    // The hub verifies our subscription request.
    let (status, body) = raw(
        Method::GET,
        format!("/websub/{FAKE_CHANNEL}?hub.mode=subscribe&hub.topic={topic}&hub.challenge=c123&hub.lease_seconds=3600"),
        "",
        None,
    )
    .await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "c123"));
    assert!(
        katsanto_db::websub::is_pushed(app.db.pool(), FAKE_CHANNEL)
            .await
            .unwrap()
    );
    let (status, _) = raw(
        Method::GET,
        format!("/websub/{FAKE_CHANNEL}?hub.mode=subscribe&hub.topic=https%3A%2F%2Fevil.example&hub.challenge=x"),
        "",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "wrong topic");
    let (status, _) = raw(
        Method::GET,
        format!("/websub/{FAKE_CHANNEL}?hub.mode=unsubscribe&hub.topic={topic}&hub.challenge=x"),
        "",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "still wanted: don't confirm unsubscribing"
    );

    // Notifications: only correctly signed ones schedule a refresh.
    let due = || async {
        sqlx::query_scalar::<_, i64>("SELECT next_fetch_at FROM channels WHERE id = ?")
            .bind(FAKE_CHANNEL)
            .fetch_one(app.db.pool())
            .await
            .unwrap()
    };
    sqlx::query("UPDATE channels SET next_fetch_at = 9999999999")
        .execute(app.db.pool())
        .await
        .unwrap();
    let (status, _) = raw(
        Method::POST,
        format!("/websub/{FAKE_CHANNEL}"),
        "<feed>new upload</feed>",
        Some("sha1=0000000000000000000000000000000000000000"),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(due().await, 9999999999, "bad signature is ignored");
    let (status, _) = raw(
        Method::POST,
        format!("/websub/{FAKE_CHANNEL}"),
        "<feed>new upload</feed>",
        Some("sha1=b8c2a5b36328a590d6f0ca9220d50f05224a6dca"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(due().await, 0, "refresh scheduled");

    let (status, _) = raw(
        Method::POST,
        "/websub/UCbbbbbbbbbbbbbbbbbbbbbb".into(),
        "x",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::GONE);
}

#[tokio::test]
async fn channel_page_stands_in_for_a_failing_rss_feed() {
    let app = App::new().await;
    let (mut a, _) = app.register_password("admin", None).await;
    let res = app
        .post(
            &mut a,
            "/api/v1/subscriptions",
            json!({ "channel_id": NO_RSS_CHANNEL }),
        )
        .await;
    assert!(res.status.is_success(), "{:?}", res.body);
    katsanto_app::refresh::run_once(&app.state).await.unwrap();

    let subs = app.get(&mut a, "/api/v1/subscriptions").await;
    assert_eq!(subs.body[0]["fetch_error"], Value::Null, "{:?}", subs.body);
    let feed = app.get(&mut a, "/api/v1/feed").await;
    let item = &feed.body["items"][0]["video"];
    assert_eq!(item["id"], "norssvideo1");
    assert_eq!(
        item["duration_s"], 200,
        "duration from the page, no enrichment needed"
    );
}
