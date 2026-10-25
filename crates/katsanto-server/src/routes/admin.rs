//! Admin: users, invites, instance settings.
//!
//! Changes to accounts and instance settings need a recent re-authentication, so a
//! stolen admin session can't mint reset links or admins.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use katsanto_db::{Db, invites, now, resets, sessions, users};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    auth::{AdminUser, tokens, validate_username},
    error::{AppError, AppResult, ErrorBody},
    instance::{self, InstanceSettings, RegistrationMode},
    state::AppState,
};

const RESET_LINK_TTL_S: i64 = 24 * 3600;
const MAX_INVITE_DAYS: i64 = 30;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_users, create_user))
        .routes(routes!(update_user, delete_user))
        .routes(routes!(create_reset_link))
        .routes(routes!(list_invites, create_invite))
        .routes(routes!(delete_invite))
        .routes(routes!(get_instance_settings, update_instance_settings))
}

#[derive(Serialize, ToSchema)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
    pub disabled: bool,
    pub created_at: i64,
    pub last_login_at: Option<i64>,
    pub has_password: bool,
    pub passkey_count: i64,
}

#[utoipa::path(get, path = "/users", tag = "admin", responses((status = 200, body = Vec<UserInfo>)))]
async fn list_users(State(st): State<AppState>, _: AdminUser) -> AppResult<Json<Vec<UserInfo>>> {
    let list = users::list(st.db.pool()).await?;
    Ok(Json(
        list.into_iter()
            .map(|u| UserInfo {
                id: u.id,
                username: u.username,
                is_admin: u.is_admin,
                disabled: u.disabled,
                created_at: u.created_at,
                last_login_at: u.last_login_at,
                has_password: u.has_password,
                passkey_count: u.passkey_count,
            })
            .collect(),
    ))
}

/// A one-time link; the secret is only ever shown here.
#[derive(Serialize, ToSchema)]
pub struct CreatedLink {
    pub url: String,
    pub expires_at: i64,
}

#[derive(Deserialize, ToSchema)]
pub struct CreateUser {
    pub username: String,
    #[serde(default)]
    pub is_admin: bool,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedUser {
    pub id: i64,
    /// Send this to the user so they can set up a passkey or password.
    pub setup_link: CreatedLink,
}

/// Create an account without credentials and return a setup link for it.
#[utoipa::path(post, path = "/users", tag = "admin", request_body = CreateUser,
    responses((status = 201, body = CreatedUser), (status = "4XX", body = ErrorBody)))]
async fn create_user(
    State(st): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(b): Json<CreateUser>,
) -> AppResult<(StatusCode, Json<CreatedUser>)> {
    admin.require_recent_reauth()?;
    let username = validate_username(&b.username)?;
    let mut tx = st.db.begin().await?;
    let user = users::create(
        &mut *tx,
        users::NewUser {
            username: &username,
            webauthn_id: Uuid::new_v4().as_bytes(),
            password_hash: None,
            is_admin: b.is_admin,
        },
    )
    .await?;
    let link = new_reset_link(&st, &mut tx, user.id, admin.id()).await?;
    Db::commit(tx).await?;
    tracing::info!(admin = admin.user.username, username, "created user");
    Ok((
        StatusCode::CREATED,
        Json(CreatedUser {
            id: user.id,
            setup_link: link,
        }),
    ))
}

async fn new_reset_link(
    st: &AppState,
    conn: &mut katsanto_db::Conn,
    user_id: i64,
    created_by: i64,
) -> AppResult<CreatedLink> {
    let token = tokens::new_token();
    let expires_at = now() + RESET_LINK_TTL_S;
    resets::create(conn, &tokens::hash(&token), user_id, created_by, expires_at).await?;
    Ok(CreatedLink {
        url: st.public_link(&format!("/reset?token={token}")),
        expires_at,
    })
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateUser {
    pub disabled: Option<bool>,
    pub is_admin: Option<bool>,
}

#[utoipa::path(patch, path = "/users/{id}", tag = "admin", request_body = UpdateUser,
    responses((status = 204), (status = "4XX", body = ErrorBody)))]
async fn update_user(
    State(st): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<i64>,
    Json(b): Json<UpdateUser>,
) -> AppResult<StatusCode> {
    admin.require_recent_reauth()?;
    let user = users::by_id(st.db.pool(), id)
        .await?
        .ok_or_else(|| AppError::NotFound("user".into()))?;
    let loses_admin =
        user.is_admin && !user.disabled && (b.disabled == Some(true) || b.is_admin == Some(false));
    if loses_admin && users::count_active_admins(st.db.pool()).await? <= 1 {
        return Err(AppError::Conflict(
            "There must be at least one active admin".into(),
        ));
    }
    if id == admin.id() && b.disabled == Some(true) {
        return Err(AppError::Conflict(
            "You can't disable your own account".into(),
        ));
    }
    if let Some(d) = b.disabled {
        users::set_disabled(st.db.pool(), id, d).await?;
        if d {
            sessions::delete_all(st.db.pool(), id, None).await?;
        }
    }
    if let Some(a) = b.is_admin {
        users::set_admin(st.db.pool(), id, a).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/users/{id}", tag = "admin",
    responses((status = 204), (status = "4XX", body = ErrorBody)))]
async fn delete_user(
    State(st): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    admin.require_recent_reauth()?;
    if id == admin.id() {
        return Err(AppError::Conflict(
            "Delete your own account from your settings".into(),
        ));
    }
    let user = users::by_id(st.db.pool(), id)
        .await?
        .ok_or_else(|| AppError::NotFound("user".into()))?;
    if user.is_admin && !user.disabled && users::count_active_admins(st.db.pool()).await? <= 1 {
        return Err(AppError::Conflict(
            "There must be at least one active admin".into(),
        ));
    }
    users::delete(st.db.pool(), id).await?;
    tracing::info!(
        admin = admin.user.username,
        user = user.username,
        "deleted user"
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Single-use link (24 h) letting the user add a new passkey or set a password.
/// Earlier unused links for the user stop working.
#[utoipa::path(post, path = "/users/{id}/reset-link", tag = "admin",
    responses((status = 201, body = CreatedLink), (status = "4XX", body = ErrorBody)))]
async fn create_reset_link(
    State(st): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<i64>,
) -> AppResult<(StatusCode, Json<CreatedLink>)> {
    admin.require_recent_reauth()?;
    users::by_id(st.db.pool(), id)
        .await?
        .ok_or_else(|| AppError::NotFound("user".into()))?;
    let mut tx = st.db.begin().await?;
    let link = new_reset_link(&st, &mut tx, id, admin.id()).await?;
    Db::commit(tx).await?;
    Ok((StatusCode::CREATED, Json(link)))
}

// ---------- invites ----------

#[derive(Serialize, ToSchema)]
pub struct InviteInfo {
    pub id: i64,
    pub note: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub used_by: Option<i64>,
    pub used_at: Option<i64>,
}

#[utoipa::path(get, path = "/invites", tag = "admin", responses((status = 200, body = Vec<InviteInfo>)))]
async fn list_invites(
    State(st): State<AppState>,
    _: AdminUser,
) -> AppResult<Json<Vec<InviteInfo>>> {
    let list = invites::list(st.db.pool()).await?;
    Ok(Json(
        list.into_iter()
            .map(|i| InviteInfo {
                id: i.id,
                note: i.note,
                created_at: i.created_at,
                expires_at: i.expires_at,
                used_by: i.used_by,
                used_at: i.used_at,
            })
            .collect(),
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateInvite {
    /// Who the invite is for, shown in the admin list.
    #[serde(default)]
    pub note: String,
    /// Validity in days (1–30, default 7).
    pub expires_in_days: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedInvite {
    pub invite: InviteInfo,
    pub link: CreatedLink,
}

#[utoipa::path(post, path = "/invites", tag = "admin", request_body = CreateInvite,
    responses((status = 201, body = CreatedInvite), (status = "4XX", body = ErrorBody)))]
async fn create_invite(
    State(st): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(b): Json<CreateInvite>,
) -> AppResult<(StatusCode, Json<CreatedInvite>)> {
    let days = b.expires_in_days.unwrap_or(7);
    if !(1..=MAX_INVITE_DAYS).contains(&days) {
        return Err(AppError::BadRequest(format!(
            "Invites are valid for 1–{MAX_INVITE_DAYS} days"
        )));
    }
    let note: String = b.note.trim().chars().take(100).collect();
    let token = tokens::new_token();
    let expires_at = now() + days * 24 * 3600;
    let inv = invites::create(
        st.db.pool(),
        &tokens::hash(&token),
        &note,
        admin.id(),
        expires_at,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedInvite {
            link: CreatedLink {
                url: st.public_link(&format!("/register?invite={token}")),
                expires_at,
            },
            invite: InviteInfo {
                id: inv.id,
                note: inv.note,
                created_at: inv.created_at,
                expires_at: inv.expires_at,
                used_by: None,
                used_at: None,
            },
        }),
    ))
}

#[utoipa::path(delete, path = "/invites/{id}", tag = "admin",
    responses((status = 204), (status = 404, body = ErrorBody)))]
async fn delete_invite(
    State(st): State<AppState>,
    _: AdminUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    if !invites::delete(st.db.pool(), id).await? {
        return Err(AppError::NotFound("invite".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------- instance settings ----------

#[derive(Serialize, ToSchema)]
pub struct AdminSettings {
    #[serde(flatten)]
    pub settings: InstanceSettings,
    pub passkeys_available: bool,
    /// Explains how to enable passkeys when they are unavailable.
    pub passkeys_unavailable_reason: Option<String>,
    pub proxy_video: bool,
    pub public_url: String,
}

#[utoipa::path(get, path = "/settings", tag = "admin", responses((status = 200, body = AdminSettings)))]
async fn get_instance_settings(
    State(st): State<AppState>,
    _: AdminUser,
) -> AppResult<Json<AdminSettings>> {
    admin_settings(&st).await.map(Json)
}

async fn admin_settings(st: &AppState) -> AppResult<AdminSettings> {
    Ok(AdminSettings {
        settings: instance::load(st.db.pool()).await?,
        passkeys_available: st.webauthn.is_some(),
        passkeys_unavailable_reason: st.config.passkeys_unavailable_reason(),
        proxy_video: st.config.proxy_video,
        public_url: st.config.public_url.to_string(),
    })
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateSettings {
    pub registration_mode: Option<RegistrationMode>,
    pub passwords_enabled: Option<bool>,
}

#[utoipa::path(patch, path = "/settings", tag = "admin", request_body = UpdateSettings,
    responses((status = 200, body = AdminSettings), (status = "4XX", body = ErrorBody)))]
async fn update_instance_settings(
    State(st): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(b): Json<UpdateSettings>,
) -> AppResult<Json<AdminSettings>> {
    admin.require_recent_reauth()?;
    let mut s = instance::load(st.db.pool()).await?;
    if let Some(mode) = b.registration_mode {
        s.registration_mode = mode;
    }
    if let Some(enabled) = b.passwords_enabled {
        if !enabled {
            ensure_nobody_locked_out(&st).await?;
        }
        s.passwords_enabled = enabled;
    }
    instance::store(st.db.pool(), &s).await?;
    tracing::info!(admin = admin.user.username, ?s, "instance settings changed");
    admin_settings(&st).await.map(Json)
}

/// Disabling passwords must not lock out users who only have a password.
async fn ensure_nobody_locked_out(st: &AppState) -> AppResult<()> {
    if st.webauthn.is_none() {
        return Err(AppError::Conflict(
            "Passwords can't be disabled while passkeys are unavailable on this server".into(),
        ));
    }
    let without_passkey: Vec<_> = users::list(st.db.pool())
        .await?
        .into_iter()
        .filter(|u| !u.disabled && u.passkey_count == 0)
        .map(|u| u.username)
        .collect();
    if !without_passkey.is_empty() {
        return Err(AppError::Conflict(format!(
            "These users have no passkey yet and would be locked out: {}",
            without_passkey.join(", ")
        )));
    }
    Ok(())
}
