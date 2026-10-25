//! Cookie sessions backed by the `sessions` table.

use axum::{
    extract::{FromRequestParts, Request, State},
    http::{HeaderMap, header, request::Parts},
    middleware::Next,
    response::Response,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use katsanto_db::{now, sessions, users};

use super::tokens;
use crate::{
    error::{AppError, AppResult},
    state::AppState,
};

pub const COOKIE: &str = "katsanto_session";
/// Sessions expire after 30 days without activity.
pub const TTL_S: i64 = 30 * 24 * 3600;
/// Only write `last_seen_at` this often, not on every request.
const TOUCH_INTERVAL_S: i64 = 3600;
/// Sensitive actions need a login or re-authentication this recent.
pub const REAUTH_WINDOW_S: i64 = 5 * 60;

/// The logged-in user and their session. Extracting it fails with 401 for guests.
#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub user: users::User,
    pub session: sessions::Session,
}

impl CurrentUser {
    pub fn id(&self) -> i64 {
        self.user.id
    }

    pub fn require_recent_reauth(&self) -> AppResult<()> {
        match self.session.reauth_at {
            Some(t) if now() - t <= REAUTH_WINDOW_S => Ok(()),
            _ => Err(AppError::ReauthRequired),
        }
    }
}

/// Like [`CurrentUser`], but only for admins (403 otherwise).
pub struct AdminUser(pub CurrentUser);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, st: &AppState) -> AppResult<Self> {
        if let Some(u) = parts.extensions.get::<CurrentUser>() {
            return Ok(u.clone());
        }
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get(COOKIE)
            .map(|c| c.value().to_owned())
            .ok_or_else(|| AppError::Unauthorized("Please log in".into()))?;
        let mut session = sessions::find_active(st.db.pool(), &tokens::hash(&token))
            .await?
            .ok_or_else(|| AppError::Unauthorized("Your session has expired".into()))?;
        let user = users::by_id(st.db.pool(), session.user_id)
            .await?
            .ok_or_else(|| AppError::Unauthorized("Please log in".into()))?;
        if now() - session.last_seen_at > TOUCH_INTERVAL_S {
            sessions::touch(st.db.pool(), session.id, TTL_S).await?;
            session.last_seen_at = now();
        }
        let current = CurrentUser { user, session };
        parts.extensions.insert(current.clone());
        Ok(current)
    }
}

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, st: &AppState) -> AppResult<Self> {
        let user = CurrentUser::from_request_parts(parts, st).await?;
        if !user.user.is_admin {
            return Err(AppError::Forbidden("Admins only".into()));
        }
        Ok(AdminUser(user))
    }
}

/// Route layer that rejects guests before the handler runs.
pub async fn require_user(
    State(st): State<AppState>,
    req: Request,
    next: Next,
) -> AppResult<Response> {
    let (mut parts, body) = req.into_parts();
    CurrentUser::from_request_parts(&mut parts, &st).await?;
    Ok(next.run(Request::from_parts(parts, body)).await)
}

/// Start a session for `user_id` and return the cookie jar to send back.
pub async fn start(
    st: &AppState,
    jar: CookieJar,
    headers: &HeaderMap,
    user_id: i64,
) -> AppResult<CookieJar> {
    let token = tokens::new_token();
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let user_agent: String = user_agent.chars().take(300).collect();
    sessions::create(
        st.db.pool(),
        &tokens::hash(&token),
        user_id,
        &user_agent,
        TTL_S,
    )
    .await?;
    users::touch_login(st.db.pool(), user_id).await?;
    Ok(jar.add(cookie(st, token, TTL_S)))
}

pub fn clear(st: &AppState, jar: CookieJar) -> CookieJar {
    jar.add(cookie(st, String::new(), 0))
}

fn cookie(st: &AppState, value: String, max_age_s: i64) -> Cookie<'static> {
    Cookie::build((COOKIE, value))
        .path("/")
        .http_only(true)
        .secure(st.config.secure_cookies())
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(max_age_s))
        .build()
}
