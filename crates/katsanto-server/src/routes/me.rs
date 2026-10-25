//! The logged-in user's own account: settings, passkeys, password, recovery codes, sessions,
//! backups.

use axum::{
    Json,
    extract::{DefaultBodyLimit, Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use axum_extra::extract::cookie::CookieJar;
use katsanto_db::{Db, backup, passkeys, sessions, users};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    auth::{
        self, Challenge, CredentialBody, CurrentUser, Me, NewPassword, passkey, password, session,
    },
    error::{AppError, AppResult, ErrorBody},
    state::AppState,
};

/// Per-user settings (theme, layout…) are stored as an opaque JSON object.
const MAX_SETTINGS_BYTES: usize = 16 * 1024;
/// Years of history with thousands of list items stay well below this.
const MAX_BACKUP_BYTES: usize = 64 * 1024 * 1024;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(me, delete_account))
        .routes(routes!(update_my_settings))
        .routes(routes!(list_passkeys))
        .routes(routes!(add_passkey_start))
        .routes(routes!(add_passkey_finish))
        .routes(routes!(rename_passkey, delete_passkey))
        .routes(routes!(set_password, remove_password))
        .routes(routes!(regenerate_recovery_codes))
        .routes(routes!(list_sessions, sign_out_others))
        .routes(routes!(revoke_session))
        .merge(
            OpenApiRouter::new()
                .routes(routes!(export_backup, import_backup))
                .layer(DefaultBodyLimit::max(MAX_BACKUP_BYTES)),
        )
}

#[utoipa::path(get, path = "", tag = "me", responses((status = 200, body = Me), (status = 401, body = ErrorBody)))]
async fn me(State(st): State<AppState>, me: CurrentUser) -> AppResult<Json<Me>> {
    Ok(Json(Me::load(&st.db, &me.user).await?))
}

#[utoipa::path(put, path = "/settings", tag = "me",
    request_body(content = Object, description = "Replaces all settings"),
    responses((status = 200, body = Me), (status = "4XX", body = ErrorBody)))]
async fn update_my_settings(
    State(st): State<AppState>,
    me: CurrentUser,
    Json(settings): Json<serde_json::Value>,
) -> AppResult<Json<Me>> {
    if !settings.is_object() {
        return Err(AppError::BadRequest("settings must be an object".into()));
    }
    let raw = settings.to_string();
    if raw.len() > MAX_SETTINGS_BYTES {
        return Err(AppError::BadRequest("settings are too large".into()));
    }
    users::set_settings(st.db.pool(), me.id(), &raw).await?;
    let user = reload(&st, me.id()).await?;
    Ok(Json(Me::load(&st.db, &user).await?))
}

// ---------- backup ----------

#[utoipa::path(get, path = "/backup", tag = "me",
    responses((status = 200, description = "The user's Katsanto data as a JSON file", body = Object)))]
async fn export_backup(
    State(st): State<AppState>,
    me: CurrentUser,
) -> AppResult<impl IntoResponse> {
    let mut conn = st.db.acquire().await?;
    let data = backup::export(&mut conn, me.id()).await?;
    let date = time::OffsetDateTime::now_utc().date();
    let filename = format!("katsanto-{}-{date}.json", me.user.username);
    Ok((
        [(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )],
        Json(data),
    ))
}

/// What a restore added; anything already in the account is left as it is.
#[derive(Serialize, ToSchema)]
pub struct ImportResult {
    pub subscriptions: u64,
    pub tags: u64,
    pub feeds: u64,
    pub lists: u64,
    pub list_items: u64,
    pub progress: u64,
    pub hidden: u64,
}

#[utoipa::path(post, path = "/backup", tag = "me",
    request_body(content = Object, description = "A file from `GET /me/backup`"),
    responses((status = 200, body = ImportResult), (status = "4XX", body = ErrorBody)))]
async fn import_backup(
    State(st): State<AppState>,
    me: CurrentUser,
    Json(data): Json<serde_json::Value>,
) -> AppResult<Json<ImportResult>> {
    let data: backup::Backup = serde_json::from_value(data)
        .map_err(|e| AppError::BadRequest(format!("not a Katsanto backup: {e}")))?;
    backup::validate(&data).map_err(|e| AppError::BadRequest(e.to_string()))?;
    let mut tx = st.db.begin().await?;
    let s = backup::import(&mut tx, me.id(), &data).await?;
    Db::commit(tx).await?;
    if s.subscriptions > 0 {
        st.refresh_wakeup.notify_one();
    }
    Ok(Json(ImportResult {
        subscriptions: s.subscriptions,
        tags: s.tags,
        feeds: s.feeds,
        lists: s.lists,
        list_items: s.list_items,
        progress: s.progress,
        hidden: s.hidden,
    }))
}

async fn reload(st: &AppState, id: i64) -> AppResult<users::User> {
    users::by_id(st.db.pool(), id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Please log in".into()))
}

// ---------- passkeys ----------

#[derive(Serialize, ToSchema)]
pub struct PasskeyInfo {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

#[utoipa::path(get, path = "/passkeys", tag = "me", responses((status = 200, body = Vec<PasskeyInfo>)))]
async fn list_passkeys(
    State(st): State<AppState>,
    me: CurrentUser,
) -> AppResult<Json<Vec<PasskeyInfo>>> {
    let rows = passkeys::for_user(st.db.pool(), me.id()).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| PasskeyInfo {
                id: r.id,
                name: r.name,
                created_at: r.created_at,
                last_used_at: r.last_used_at,
            })
            .collect(),
    ))
}

#[utoipa::path(post, path = "/passkeys/start", tag = "me",
    responses((status = 200, body = Challenge), (status = "4XX", body = ErrorBody)))]
async fn add_passkey_start(
    State(st): State<AppState>,
    me: CurrentUser,
) -> AppResult<Json<Challenge>> {
    // A new passkey is a lasting way in: a stolen session alone must not be enough.
    // Checked at the start only, so an expiring window can't strand a created credential.
    me.require_recent_reauth()?;
    let wa = auth::webauthn(&st)?;
    let (options, reg) = auth::start_registration_for(&st, wa, &me.user).await?;
    let challenge_id = auth::put_ceremony(
        &st,
        auth::Ceremony::AddPasskey {
            reg,
            user_id: me.id(),
        },
    )
    .await?;
    Ok(Json(Challenge {
        challenge_id,
        options,
    }))
}

#[utoipa::path(post, path = "/passkeys/finish", tag = "me", request_body = CredentialBody,
    responses((status = 201, body = PasskeyInfo), (status = "4XX", body = ErrorBody)))]
async fn add_passkey_finish(
    State(st): State<AppState>,
    me: CurrentUser,
    Json(b): Json<CredentialBody>,
) -> AppResult<(StatusCode, Json<PasskeyInfo>)> {
    let wa = auth::webauthn(&st)?;
    let auth::Ceremony::AddPasskey { reg, user_id } =
        auth::take_ceremony(&st, &b.challenge_id, "register").await?
    else {
        return Err(AppError::BadRequest("unexpected challenge".into()));
    };
    if user_id != me.id() {
        return Err(AppError::BadRequest("unexpected challenge".into()));
    }
    let pk = wa
        .finish_passkey_registration(&auth::credential(b.credential)?, &reg)
        .map_err(passkey::auth_failed)?;
    let row = passkeys::create(
        st.db.pool(),
        me.id(),
        pk.cred_id(),
        &passkey::to_json(&pk)?,
        &passkey::default_name(b.name),
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(PasskeyInfo {
            id: row.id,
            name: row.name,
            created_at: row.created_at,
            last_used_at: None,
        }),
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct Rename {
    pub name: String,
}

#[utoipa::path(patch, path = "/passkeys/{id}", tag = "me", request_body = Rename,
    responses((status = 204), (status = 404, body = ErrorBody)))]
async fn rename_passkey(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(id): Path<i64>,
    Json(b): Json<Rename>,
) -> AppResult<StatusCode> {
    if !passkeys::rename(
        st.db.pool(),
        me.id(),
        id,
        &passkey::default_name(Some(b.name)),
    )
    .await?
    {
        return Err(AppError::NotFound("passkey".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/passkeys/{id}", tag = "me",
    responses((status = 204), (status = "4XX", body = ErrorBody)))]
async fn delete_passkey(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    me.require_recent_reauth()?;
    let count = passkeys::count_for_user(st.db.pool(), me.id()).await?;
    if count <= 1 && me.user.password_hash.is_none() {
        return Err(last_credential());
    }
    if !passkeys::delete(st.db.pool(), me.id(), id).await? {
        return Err(AppError::NotFound("passkey".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

fn last_credential() -> AppError {
    AppError::Conflict(
        "You can't remove your last way to log in. Add another passkey or a password first.".into(),
    )
}

// ---------- password ----------

#[utoipa::path(put, path = "/password", tag = "me", request_body = NewPassword,
    responses((status = 204), (status = "4XX", body = ErrorBody)))]
async fn set_password(
    State(st): State<AppState>,
    me: CurrentUser,
    Json(b): Json<NewPassword>,
) -> AppResult<StatusCode> {
    me.require_recent_reauth()?;
    if !auth::passwords_enabled(&st).await? {
        return Err(AppError::Forbidden(
            "Passwords are disabled on this server".into(),
        ));
    }
    password::validate(&b.password)?;
    let hash = password::hash(b.password).await?;
    users::set_password(st.db.pool(), me.id(), Some(&hash)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/password", tag = "me",
    responses((status = 204), (status = "4XX", body = ErrorBody)))]
async fn remove_password(State(st): State<AppState>, me: CurrentUser) -> AppResult<StatusCode> {
    me.require_recent_reauth()?;
    if passkeys::count_for_user(st.db.pool(), me.id()).await? == 0 {
        return Err(last_credential());
    }
    users::set_password(st.db.pool(), me.id(), None).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- recovery codes ----------

#[derive(Serialize, ToSchema)]
pub struct RecoveryCodes {
    pub recovery_codes: Vec<String>,
}

/// Replace all recovery codes; the old ones stop working.
#[utoipa::path(post, path = "/recovery-codes", tag = "me",
    responses((status = 200, body = RecoveryCodes), (status = "4XX", body = ErrorBody)))]
async fn regenerate_recovery_codes(
    State(st): State<AppState>,
    me: CurrentUser,
) -> AppResult<Json<RecoveryCodes>> {
    me.require_recent_reauth()?;
    let mut tx = st.db.begin().await?;
    let codes = auth::new_recovery_codes(&mut tx, me.id()).await?;
    Db::commit(tx).await?;
    Ok(Json(RecoveryCodes {
        recovery_codes: codes,
    }))
}

// ---------- sessions ----------

#[derive(Serialize, ToSchema)]
pub struct SessionInfo {
    pub id: i64,
    pub user_agent: String,
    pub created_at: i64,
    pub last_seen_at: i64,
    /// The session making this request.
    pub current: bool,
}

#[utoipa::path(get, path = "/sessions", tag = "me", responses((status = 200, body = Vec<SessionInfo>)))]
async fn list_sessions(
    State(st): State<AppState>,
    me: CurrentUser,
) -> AppResult<Json<Vec<SessionInfo>>> {
    let list = sessions::for_user(st.db.pool(), me.id()).await?;
    Ok(Json(
        list.into_iter()
            .map(|s| SessionInfo {
                current: s.id == me.session.id,
                id: s.id,
                user_agent: s.user_agent,
                created_at: s.created_at,
                last_seen_at: s.last_seen_at,
            })
            .collect(),
    ))
}

#[utoipa::path(delete, path = "/sessions/{id}", tag = "me",
    responses((status = 204), (status = 404, body = ErrorBody)))]
async fn revoke_session(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    if !sessions::delete(st.db.pool(), me.id(), id).await? {
        return Err(AppError::NotFound("session".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Sign out everywhere except here.
#[utoipa::path(delete, path = "/sessions", tag = "me", responses((status = 204)))]
async fn sign_out_others(State(st): State<AppState>, me: CurrentUser) -> AppResult<StatusCode> {
    sessions::delete_all(st.db.pool(), me.id(), Some(me.session.id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- account ----------

#[utoipa::path(delete, path = "", tag = "me",
    responses((status = 204), (status = "4XX", body = ErrorBody)))]
async fn delete_account(
    State(st): State<AppState>,
    me: CurrentUser,
    jar: CookieJar,
) -> AppResult<(StatusCode, CookieJar)> {
    me.require_recent_reauth()?;
    if me.user.is_admin && users::count_active_admins(st.db.pool()).await? <= 1 {
        return Err(AppError::Conflict(
            "You are the only admin. Make someone else an admin before deleting your account."
                .into(),
        ));
    }
    users::delete(st.db.pool(), me.id()).await?;
    tracing::info!(user = me.user.username, "account deleted");
    Ok((StatusCode::NO_CONTENT, session::clear(&st, jar)))
}
