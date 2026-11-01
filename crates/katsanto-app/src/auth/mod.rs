//! Accounts: passkeys first, password fallback, recovery codes, reset links.

pub mod passkey;
pub mod password;
pub mod session;
pub mod tokens;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use axum_extra::extract::cookie::CookieJar;
use katsanto_db::{
    Conn, Db, Executor, challenges, invites, passkeys, recovery, resets, sessions, users,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use webauthn_rs::prelude::*;

pub use session::{AdminUser, CurrentUser};

use crate::{
    error::{AppError, AppResult, ErrorBody},
    instance::{self, RegistrationMode},
    state::AppState,
};

/// WebAuthn ceremonies must finish within this time.
const CHALLENGE_TTL_S: i64 = 5 * 60;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(config))
        .routes(routes!(register_passkey_start))
        .routes(routes!(register_passkey_finish))
        .routes(routes!(register_password))
        .routes(routes!(login_passkey_start))
        .routes(routes!(login_passkey_finish))
        .routes(routes!(login_password))
        .routes(routes!(login_recovery))
        .routes(routes!(logout))
        .routes(routes!(reset_info))
        .routes(routes!(reset_passkey_start))
        .routes(routes!(reset_passkey_finish))
        .routes(routes!(reset_password))
        .routes(routes!(reauth_passkey_start))
        .routes(routes!(reauth_passkey_finish))
        .routes(routes!(reauth_password))
}

// ---------- shared types ----------

#[derive(Serialize, ToSchema)]
pub struct AuthConfig {
    /// Passkeys work on this instance (HTTPS + domain name, or localhost).
    pub passkeys_available: bool,
    /// Password login/registration allowed.
    pub passwords_enabled: bool,
    pub registration_mode: RegistrationMode,
    /// No accounts yet: the first registration creates the admin.
    pub needs_setup: bool,
    /// Whether the `invite` query parameter is a usable invite.
    pub invite_valid: Option<bool>,
}

/// Options to pass to `navigator.credentials.create/get` (via @simplewebauthn/browser).
#[derive(Serialize, ToSchema)]
pub struct Challenge {
    pub challenge_id: String,
    #[schema(value_type = Object)]
    pub options: serde_json::Value,
}

#[derive(Deserialize, ToSchema)]
pub struct CredentialBody {
    pub challenge_id: String,
    /// The browser's `PublicKeyCredential` as JSON.
    #[schema(value_type = Object)]
    pub credential: serde_json::Value,
    /// Name for a newly registered passkey, e.g. "iPhone".
    pub name: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SignupResult {
    pub user: Me,
    /// Shown once; store them somewhere safe.
    pub recovery_codes: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub struct LoginResult {
    pub user: Me,
    /// Set when logging in used a recovery code: the user should add a new passkey or password.
    pub used_recovery_code: bool,
}

#[derive(Serialize, ToSchema)]
pub struct Me {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
    pub has_password: bool,
    pub passkey_count: i64,
    pub recovery_codes_remaining: i64,
    #[schema(value_type = Object)]
    pub settings: serde_json::Value,
}

impl Me {
    pub async fn load(e: &katsanto_db::Db, user: &users::User) -> AppResult<Self> {
        Ok(Self {
            id: user.id,
            username: user.username.clone(),
            is_admin: user.is_admin,
            has_password: user.password_hash.is_some(),
            passkey_count: passkeys::count_for_user(e.pool(), user.id).await?,
            recovery_codes_remaining: recovery::remaining(e.pool(), user.id).await?,
            settings: serde_json::from_str(&user.settings)
                .unwrap_or_else(|_| serde_json::json!({})),
        })
    }
}

/// Challenge state stored between the start and finish requests.
#[derive(Serialize, Deserialize)]
pub(crate) enum Ceremony {
    Signup {
        reg: PasskeyRegistration,
        username: String,
        webauthn_id: Uuid,
        invite: Option<String>,
    },
    AddPasskey {
        reg: PasskeyRegistration,
        user_id: i64,
    },
    Reset {
        reg: PasskeyRegistration,
        token: String,
    },
    Login(DiscoverableAuthentication),
    Reauth {
        auth: PasskeyAuthentication,
        user_id: i64,
    },
}

impl Ceremony {
    fn kind(&self) -> &'static str {
        match self {
            Ceremony::Signup { .. } | Ceremony::AddPasskey { .. } | Ceremony::Reset { .. } => {
                "register"
            }
            Ceremony::Login(_) => "login",
            Ceremony::Reauth { .. } => "reauth",
        }
    }
}

pub(crate) async fn put_ceremony(st: &AppState, c: Ceremony) -> AppResult<String> {
    let id = tokens::new_token();
    let state = serde_json::to_string(&c).map_err(AppError::internal)?;
    challenges::put(st.db.pool(), &id, c.kind(), &state, CHALLENGE_TTL_S).await?;
    Ok(id)
}

pub(crate) async fn take_ceremony(st: &AppState, id: &str, kind: &str) -> AppResult<Ceremony> {
    let raw = challenges::take(st.db.pool(), id, kind)
        .await?
        .ok_or_else(|| AppError::BadRequest("This request expired, please try again".into()))?;
    serde_json::from_str(&raw).map_err(AppError::internal)
}

pub(crate) fn webauthn(st: &AppState) -> AppResult<&Webauthn> {
    st.webauthn
        .as_deref()
        .ok_or_else(|| AppError::Forbidden("Passkeys are not available on this server".into()))
}

pub(crate) fn credential<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> AppResult<T> {
    serde_json::from_value(v).map_err(|e| AppError::BadRequest(format!("invalid credential: {e}")))
}

fn options_json(v: impl Serialize) -> AppResult<serde_json::Value> {
    serde_json::to_value(v).map_err(AppError::internal)
}

/// Passwords are always allowed when passkeys are not available.
pub(crate) async fn passwords_enabled(st: &AppState) -> AppResult<bool> {
    Ok(!st.config.passkeys_available() || instance::load(st.db.pool()).await?.passwords_enabled)
}

async fn require_passwords(st: &AppState) -> AppResult<()> {
    if passwords_enabled(st).await? {
        Ok(())
    } else {
        Err(AppError::Forbidden(
            "Passwords are disabled on this server, please use a passkey".into(),
        ))
    }
}

pub(crate) fn validate_username(name: &str) -> AppResult<String> {
    let name = name.trim();
    let valid = (3..=32).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && name.starts_with(|c: char| c.is_ascii_alphanumeric());
    if valid {
        Ok(name.to_owned())
    } else {
        Err(AppError::BadRequest(
            "Usernames are 3–32 characters: letters, digits, '.', '_' and '-', starting with a letter or digit".into(),
        ))
    }
}

struct Gate {
    first_user: bool,
    invite_id: Option<i64>,
}

/// May someone register right now (with this invite)?
async fn registration_gate(conn: &mut Conn, invite: Option<&str>) -> AppResult<Gate> {
    if users::count(&mut *conn).await? == 0 {
        return Ok(Gate {
            first_user: true,
            invite_id: None,
        });
    }
    let invite_id = match invite {
        Some(token) => Some(
            invites::find_valid(&mut *conn, &tokens::hash(token))
                .await?
                .ok_or_else(|| {
                    AppError::Forbidden("This invite link is invalid or has expired".into())
                })?
                .id,
        ),
        None => None,
    };
    match (
        instance::load(&mut *conn).await?.registration_mode,
        invite_id,
    ) {
        (RegistrationMode::Closed, _) => Err(AppError::Forbidden("Registration is closed".into())),
        (RegistrationMode::Invite, None) => Err(AppError::Forbidden(
            "You need an invite link to register".into(),
        )),
        _ => Ok(Gate {
            first_user: false,
            invite_id,
        }),
    }
}

/// Store new recovery codes for a user and return them in plain text.
pub(crate) async fn new_recovery_codes(conn: &mut Conn, user_id: i64) -> AppResult<Vec<String>> {
    let codes = tokens::new_recovery_codes();
    let hashes: Vec<_> = codes
        .iter()
        .map(|c| tokens::hash_recovery_code(c))
        .collect();
    recovery::replace(conn, user_id, &hashes).await?;
    Ok(codes)
}

enum NewCredential<'a> {
    Passkey(&'a Passkey, String),
    Password(String),
}

/// Create an account in one transaction: user, credential, invite use, recovery codes.
async fn create_account(
    st: &AppState,
    username: &str,
    webauthn_id: Uuid,
    invite: Option<&str>,
    cred: NewCredential<'_>,
) -> AppResult<(users::User, Vec<String>)> {
    let mut tx = st.db.begin().await?;
    let gate = registration_gate(&mut tx, invite).await?;
    let password_hash = match &cred {
        NewCredential::Password(h) => Some(h.as_str()),
        NewCredential::Passkey(..) => None,
    };
    let user = users::create(
        &mut *tx,
        users::NewUser {
            username,
            webauthn_id: webauthn_id.as_bytes(),
            password_hash,
            is_admin: gate.first_user,
        },
    )
    .await?;
    if let Some(id) = gate.invite_id
        && !invites::consume(&mut *tx, id, user.id).await?
    {
        return Err(AppError::Forbidden(
            "This invite link has already been used".into(),
        ));
    }
    if let NewCredential::Passkey(pk, name) = &cred {
        passkeys::create(
            &mut *tx,
            user.id,
            pk.cred_id(),
            &passkey::to_json(pk)?,
            name,
        )
        .await?;
    }
    let codes = new_recovery_codes(&mut tx, user.id).await?;
    Db::commit(tx).await?;
    if gate.first_user {
        tracing::info!(username, "created first account (admin)");
    }
    Ok((user, codes))
}

async fn ensure_username_free(e: impl Executor<'_>, username: &str) -> AppResult<()> {
    if users::by_username(e, username).await?.is_some() {
        return Err(AppError::Conflict("This username is taken".into()));
    }
    Ok(())
}

// ---------- config ----------

#[derive(Deserialize, IntoParams)]
pub struct ConfigQuery {
    /// Invite token from an invite link, to check its validity.
    invite: Option<String>,
}

/// Public: what the login/registration pages should offer.
#[utoipa::path(get, path = "/config", tag = "auth", params(ConfigQuery),
    responses((status = 200, body = AuthConfig)))]
async fn config(
    State(st): State<AppState>,
    Query(q): Query<ConfigQuery>,
) -> AppResult<Json<AuthConfig>> {
    let settings = instance::load(st.db.pool()).await?;
    let invite_valid = match q.invite {
        Some(t) => Some(
            invites::find_valid(st.db.pool(), &tokens::hash(&t))
                .await?
                .is_some(),
        ),
        None => None,
    };
    Ok(Json(AuthConfig {
        passkeys_available: st.webauthn.is_some(),
        passwords_enabled: passwords_enabled(&st).await?,
        registration_mode: settings.registration_mode,
        needs_setup: users::count(st.db.pool()).await? == 0,
        invite_valid,
    }))
}

// ---------- registration ----------

#[derive(Deserialize, ToSchema)]
pub struct RegisterStart {
    pub username: String,
    pub invite: Option<String>,
}

#[utoipa::path(post, path = "/register/passkey/start", tag = "auth", request_body = RegisterStart,
    responses((status = 200, body = Challenge), (status = "4XX", body = ErrorBody)))]
async fn register_passkey_start(
    State(st): State<AppState>,
    Json(b): Json<RegisterStart>,
) -> AppResult<Json<Challenge>> {
    let wa = webauthn(&st)?;
    let username = validate_username(&b.username)?;
    // Fail early; everything is checked again when the account is created.
    let mut conn = st.db.acquire().await?;
    registration_gate(&mut conn, b.invite.as_deref()).await?;
    drop(conn);
    ensure_username_free(st.db.pool(), &username).await?;

    let webauthn_id = Uuid::new_v4();
    let (ccr, reg) = wa
        .start_passkey_registration(webauthn_id, &username, &username, None)
        .map_err(AppError::internal)?;
    let challenge_id = put_ceremony(
        &st,
        Ceremony::Signup {
            reg,
            username,
            webauthn_id,
            invite: b.invite,
        },
    )
    .await?;
    Ok(Json(Challenge {
        challenge_id,
        options: passkey::registration_options(ccr)?,
    }))
}

#[utoipa::path(post, path = "/register/passkey/finish", tag = "auth", request_body = CredentialBody,
    responses((status = 201, body = SignupResult), (status = "4XX", body = ErrorBody)))]
async fn register_passkey_finish(
    State(st): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(b): Json<CredentialBody>,
) -> AppResult<(StatusCode, CookieJar, Json<SignupResult>)> {
    let wa = webauthn(&st)?;
    let Ceremony::Signup {
        reg,
        username,
        webauthn_id,
        invite,
    } = take_ceremony(&st, &b.challenge_id, "register").await?
    else {
        return Err(AppError::BadRequest("unexpected challenge".into()));
    };
    let pk = wa
        .finish_passkey_registration(&credential(b.credential)?, &reg)
        .map_err(passkey::auth_failed)?;
    let name = passkey::default_name(b.name);
    let (user, codes) = create_account(
        &st,
        &username,
        webauthn_id,
        invite.as_deref(),
        NewCredential::Passkey(&pk, name),
    )
    .await?;
    let jar = session::start(&st, jar, &headers, user.id).await?;
    let me = Me::load(&st.db, &user).await?;
    Ok((
        StatusCode::CREATED,
        jar,
        Json(SignupResult {
            user: me,
            recovery_codes: codes,
        }),
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct RegisterPassword {
    pub username: String,
    pub password: String,
    pub invite: Option<String>,
}

#[utoipa::path(post, path = "/register/password", tag = "auth", request_body = RegisterPassword,
    responses((status = 201, body = SignupResult), (status = "4XX", body = ErrorBody)))]
async fn register_password(
    State(st): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(b): Json<RegisterPassword>,
) -> AppResult<(StatusCode, CookieJar, Json<SignupResult>)> {
    require_passwords(&st).await?;
    let username = validate_username(&b.username)?;
    password::validate(&b.password)?;
    // Check the invite first so people without one can't probe for usernames.
    let mut conn = st.db.acquire().await?;
    registration_gate(&mut conn, b.invite.as_deref()).await?;
    drop(conn);
    ensure_username_free(st.db.pool(), &username).await?;
    let hash = password::hash(b.password).await?;
    let (user, codes) = create_account(
        &st,
        &username,
        Uuid::new_v4(),
        b.invite.as_deref(),
        NewCredential::Password(hash),
    )
    .await?;
    let jar = session::start(&st, jar, &headers, user.id).await?;
    let me = Me::load(&st.db, &user).await?;
    Ok((
        StatusCode::CREATED,
        jar,
        Json(SignupResult {
            user: me,
            recovery_codes: codes,
        }),
    ))
}

// ---------- login ----------

/// Username-less passkey login; the options also work for the browser's passkey autofill.
#[utoipa::path(post, path = "/login/passkey/start", tag = "auth",
    responses((status = 200, body = Challenge), (status = "4XX", body = ErrorBody)))]
async fn login_passkey_start(State(st): State<AppState>) -> AppResult<Json<Challenge>> {
    let wa = webauthn(&st)?;
    let (rcr, auth) = wa
        .start_discoverable_authentication()
        .map_err(AppError::internal)?;
    let challenge_id = put_ceremony(&st, Ceremony::Login(auth)).await?;
    Ok(Json(Challenge {
        challenge_id,
        options: options_json(rcr)?,
    }))
}

#[utoipa::path(post, path = "/login/passkey/finish", tag = "auth", request_body = CredentialBody,
    responses((status = 200, body = LoginResult), (status = "4XX", body = ErrorBody)))]
async fn login_passkey_finish(
    State(st): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(b): Json<CredentialBody>,
) -> AppResult<(CookieJar, Json<LoginResult>)> {
    let wa = webauthn(&st)?;
    let Ceremony::Login(state) = take_ceremony(&st, &b.challenge_id, "login").await? else {
        return Err(AppError::BadRequest("unexpected challenge".into()));
    };
    let cred: PublicKeyCredential = credential(b.credential)?;
    let (webauthn_id, _) = wa
        .identify_discoverable_authentication(&cred)
        .map_err(passkey::auth_failed)?;
    let user = users::by_webauthn_id(st.db.pool(), webauthn_id.as_bytes())
        .await?
        .filter(|u| !u.disabled)
        .ok_or_else(|| AppError::Unauthorized("Unknown passkey".into()))?;

    let rows = passkeys::for_user(st.db.pool(), user.id).await?;
    let keys = rows
        .iter()
        .map(|r| passkey::from_json(&r.passkey))
        .collect::<AppResult<Vec<_>>>()?;
    let discoverable: Vec<DiscoverableKey> = keys.iter().map(Into::into).collect();
    let result = wa
        .finish_discoverable_authentication(&cred, state, &discoverable)
        .map_err(passkey::auth_failed)?;
    record_passkey_use(&st, &rows, keys, &result).await?;

    let jar = session::start(&st, jar, &headers, user.id).await?;
    Ok((
        jar,
        Json(LoginResult {
            user: Me::load(&st.db, &user).await?,
            used_recovery_code: false,
        }),
    ))
}

/// Persist the signature counter / backup state and last use of the passkey that was used.
async fn record_passkey_use(
    st: &AppState,
    rows: &[passkeys::PasskeyRow],
    keys: Vec<Passkey>,
    result: &AuthenticationResult,
) -> AppResult<()> {
    for (row, mut key) in rows.iter().zip(keys) {
        if key.cred_id() == result.cred_id() {
            key.update_credential(result);
            passkeys::update_after_use(st.db.pool(), row.id, &passkey::to_json(&key)?).await?;
        }
    }
    Ok(())
}

#[derive(Deserialize, ToSchema)]
pub struct PasswordLogin {
    pub username: String,
    pub password: String,
}

const BAD_LOGIN: &str = "Wrong username or password";

#[utoipa::path(post, path = "/login/password", tag = "auth", request_body = PasswordLogin,
    responses((status = 200, body = LoginResult), (status = "4XX", body = ErrorBody)))]
async fn login_password(
    State(st): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(b): Json<PasswordLogin>,
) -> AppResult<(CookieJar, Json<LoginResult>)> {
    require_passwords(&st).await?;
    let user = users::by_username(st.db.pool(), b.username.trim()).await?;
    let hash = user.as_ref().and_then(|u| u.password_hash.clone());
    let ok = password::verify(b.password, hash).await?;
    let user = match user {
        Some(u) if ok && !u.disabled => u,
        _ => return Err(AppError::Unauthorized(BAD_LOGIN.into())),
    };
    let jar = session::start(&st, jar, &headers, user.id).await?;
    Ok((
        jar,
        Json(LoginResult {
            user: Me::load(&st.db, &user).await?,
            used_recovery_code: false,
        }),
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct RecoveryLogin {
    pub username: String,
    pub code: String,
}

#[utoipa::path(post, path = "/login/recovery", tag = "auth", request_body = RecoveryLogin,
    responses((status = 200, body = LoginResult), (status = "4XX", body = ErrorBody)))]
async fn login_recovery(
    State(st): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(b): Json<RecoveryLogin>,
) -> AppResult<(CookieJar, Json<LoginResult>)> {
    let invalid = || AppError::Unauthorized("Wrong username or recovery code".into());
    let user = users::by_username(st.db.pool(), b.username.trim())
        .await?
        .filter(|u| !u.disabled)
        .ok_or_else(invalid)?;
    if !recovery::consume(st.db.pool(), user.id, &tokens::hash_recovery_code(&b.code)).await? {
        return Err(invalid());
    }
    tracing::info!(user = user.username, "login with recovery code");
    let jar = session::start(&st, jar, &headers, user.id).await?;
    Ok((
        jar,
        Json(LoginResult {
            user: Me::load(&st.db, &user).await?,
            used_recovery_code: true,
        }),
    ))
}

#[utoipa::path(post, path = "/logout", tag = "auth", responses((status = 204)))]
async fn logout(State(st): State<AppState>, jar: CookieJar) -> AppResult<(StatusCode, CookieJar)> {
    if let Some(c) = jar.get(session::COOKIE)
        && let Some(s) = sessions::find_active(st.db.pool(), &tokens::hash(c.value())).await?
    {
        sessions::delete(st.db.pool(), s.user_id, s.id).await?;
    }
    Ok((StatusCode::NO_CONTENT, session::clear(&st, jar)))
}

// ---------- reset links ----------

#[derive(Serialize, ToSchema)]
pub struct ResetInfo {
    pub username: String,
    pub passkeys_available: bool,
    pub passwords_enabled: bool,
}

async fn reset_link(st: &AppState, token: &str) -> AppResult<(resets::ResetLink, users::User)> {
    let invalid = || AppError::NotFound("This reset link is invalid or has expired".into());
    let link = resets::find_valid(st.db.pool(), &tokens::hash(token))
        .await?
        .ok_or_else(invalid)?;
    let user = users::by_id(st.db.pool(), link.user_id)
        .await?
        .filter(|u| !u.disabled)
        .ok_or_else(invalid)?;
    Ok((link, user))
}

/// Complete a reset: use the link, sign out everywhere, new recovery codes, new session.
async fn finish_reset(
    st: &AppState,
    jar: CookieJar,
    headers: &HeaderMap,
    link: &resets::ResetLink,
    user: &users::User,
    cred: NewCredential<'_>,
) -> AppResult<(CookieJar, Json<SignupResult>)> {
    let mut tx = st.db.begin().await?;
    if !resets::consume(&mut *tx, link.id).await? {
        return Err(AppError::NotFound(
            "This reset link has already been used".into(),
        ));
    }
    match cred {
        NewCredential::Passkey(pk, name) => {
            passkeys::create(
                &mut *tx,
                user.id,
                pk.cred_id(),
                &passkey::to_json(pk)?,
                &name,
            )
            .await?;
        }
        NewCredential::Password(hash) => {
            users::set_password(&mut *tx, user.id, Some(&hash)).await?
        }
    }
    sessions::delete_all(&mut *tx, user.id, None).await?;
    let codes = new_recovery_codes(&mut tx, user.id).await?;
    Db::commit(tx).await?;
    tracing::info!(user = user.username, "account reset");

    let user = users::by_id(st.db.pool(), user.id)
        .await?
        .ok_or_else(|| AppError::internal("user vanished"))?;
    let jar = session::start(st, jar, headers, user.id).await?;
    Ok((
        jar,
        Json(SignupResult {
            user: Me::load(&st.db, &user).await?,
            recovery_codes: codes,
        }),
    ))
}

#[utoipa::path(get, path = "/reset/{token}", tag = "auth",
    responses((status = 200, body = ResetInfo), (status = 404, body = ErrorBody)))]
async fn reset_info(
    State(st): State<AppState>,
    Path(token): Path<String>,
) -> AppResult<Json<ResetInfo>> {
    let (_, user) = reset_link(&st, &token).await?;
    Ok(Json(ResetInfo {
        username: user.username,
        passkeys_available: st.webauthn.is_some(),
        passwords_enabled: passwords_enabled(&st).await?,
    }))
}

#[utoipa::path(post, path = "/reset/{token}/passkey/start", tag = "auth",
    responses((status = 200, body = Challenge), (status = "4XX", body = ErrorBody)))]
async fn reset_passkey_start(
    State(st): State<AppState>,
    Path(token): Path<String>,
) -> AppResult<Json<Challenge>> {
    let wa = webauthn(&st)?;
    let (_, user) = reset_link(&st, &token).await?;
    let (options, reg) = start_registration_for(&st, wa, &user).await?;
    let challenge_id = put_ceremony(&st, Ceremony::Reset { reg, token }).await?;
    Ok(Json(Challenge {
        challenge_id,
        options,
    }))
}

/// Registration options for an existing user, excluding passkeys they already have.
pub(crate) async fn start_registration_for(
    st: &AppState,
    wa: &Webauthn,
    user: &users::User,
) -> AppResult<(serde_json::Value, PasskeyRegistration)> {
    let existing = passkeys::for_user(st.db.pool(), user.id)
        .await?
        .iter()
        .map(|r| passkey::from_json(&r.passkey).map(|p| p.cred_id().clone()))
        .collect::<AppResult<Vec<_>>>()?;
    let webauthn_id = Uuid::from_slice(&user.webauthn_id).map_err(AppError::internal)?;
    let (ccr, reg) = wa
        .start_passkey_registration(webauthn_id, &user.username, &user.username, Some(existing))
        .map_err(AppError::internal)?;
    Ok((passkey::registration_options(ccr)?, reg))
}

#[utoipa::path(post, path = "/reset/{token}/passkey/finish", tag = "auth", request_body = CredentialBody,
    responses((status = 200, body = SignupResult), (status = "4XX", body = ErrorBody)))]
async fn reset_passkey_finish(
    State(st): State<AppState>,
    Path(token): Path<String>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(b): Json<CredentialBody>,
) -> AppResult<(CookieJar, Json<SignupResult>)> {
    let wa = webauthn(&st)?;
    let Ceremony::Reset { reg, token: t } = take_ceremony(&st, &b.challenge_id, "register").await?
    else {
        return Err(AppError::BadRequest("unexpected challenge".into()));
    };
    if t != token {
        return Err(AppError::BadRequest("unexpected challenge".into()));
    }
    let (link, user) = reset_link(&st, &token).await?;
    let pk = wa
        .finish_passkey_registration(&credential(b.credential)?, &reg)
        .map_err(passkey::auth_failed)?;
    finish_reset(
        &st,
        jar,
        &headers,
        &link,
        &user,
        NewCredential::Passkey(&pk, passkey::default_name(b.name)),
    )
    .await
}

#[derive(Deserialize, ToSchema)]
pub struct NewPassword {
    pub password: String,
}

#[utoipa::path(post, path = "/reset/{token}/password", tag = "auth", request_body = NewPassword,
    responses((status = 200, body = SignupResult), (status = "4XX", body = ErrorBody)))]
async fn reset_password(
    State(st): State<AppState>,
    Path(token): Path<String>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(b): Json<NewPassword>,
) -> AppResult<(CookieJar, Json<SignupResult>)> {
    require_passwords(&st).await?;
    password::validate(&b.password)?;
    let (link, user) = reset_link(&st, &token).await?;
    let hash = password::hash(b.password).await?;
    finish_reset(
        &st,
        jar,
        &headers,
        &link,
        &user,
        NewCredential::Password(hash),
    )
    .await
}

// ---------- re-authentication ----------

#[utoipa::path(post, path = "/reauth/passkey/start", tag = "auth",
    responses((status = 200, body = Challenge), (status = "4XX", body = ErrorBody)))]
async fn reauth_passkey_start(
    State(st): State<AppState>,
    me: CurrentUser,
) -> AppResult<Json<Challenge>> {
    let wa = webauthn(&st)?;
    let keys = passkeys::for_user(st.db.pool(), me.id())
        .await?
        .iter()
        .map(|r| passkey::from_json(&r.passkey))
        .collect::<AppResult<Vec<_>>>()?;
    if keys.is_empty() {
        return Err(AppError::BadRequest("You have no passkeys".into()));
    }
    let (rcr, auth) = wa
        .start_passkey_authentication(&keys)
        .map_err(AppError::internal)?;
    let challenge_id = put_ceremony(
        &st,
        Ceremony::Reauth {
            auth,
            user_id: me.id(),
        },
    )
    .await?;
    Ok(Json(Challenge {
        challenge_id,
        options: options_json(rcr)?,
    }))
}

#[utoipa::path(post, path = "/reauth/passkey/finish", tag = "auth", request_body = CredentialBody,
    responses((status = 204), (status = "4XX", body = ErrorBody)))]
async fn reauth_passkey_finish(
    State(st): State<AppState>,
    me: CurrentUser,
    Json(b): Json<CredentialBody>,
) -> AppResult<StatusCode> {
    let wa = webauthn(&st)?;
    let Ceremony::Reauth { auth, user_id } = take_ceremony(&st, &b.challenge_id, "reauth").await?
    else {
        return Err(AppError::BadRequest("unexpected challenge".into()));
    };
    if user_id != me.id() {
        return Err(AppError::BadRequest("unexpected challenge".into()));
    }
    let result = wa
        .finish_passkey_authentication(&credential(b.credential)?, &auth)
        .map_err(passkey::auth_failed)?;
    let rows = passkeys::for_user(st.db.pool(), me.id()).await?;
    let keys = rows
        .iter()
        .map(|r| passkey::from_json(&r.passkey))
        .collect::<AppResult<Vec<_>>>()?;
    record_passkey_use(&st, &rows, keys, &result).await?;
    sessions::mark_reauth(st.db.pool(), me.session.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct PasswordReauth {
    pub password: String,
}

#[utoipa::path(post, path = "/reauth/password", tag = "auth", request_body = PasswordReauth,
    responses((status = 204), (status = "4XX", body = ErrorBody)))]
async fn reauth_password(
    State(st): State<AppState>,
    me: CurrentUser,
    Json(b): Json<PasswordReauth>,
) -> AppResult<StatusCode> {
    if !password::verify(b.password, me.user.password_hash.clone()).await? {
        return Err(AppError::Unauthorized("Wrong password".into()));
    }
    sessions::mark_reauth(st.db.pool(), me.session.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames() {
        assert_eq!(validate_username(" alice ").unwrap(), "alice");
        assert!(validate_username("a.b_c-d9").is_ok());
        assert!(validate_username("ab").is_err());
        assert!(validate_username(&"a".repeat(33)).is_err());
        assert!(validate_username("-alice").is_err());
        assert!(validate_username("al ice").is_err());
        assert!(validate_username("älice").is_err());
    }
}
