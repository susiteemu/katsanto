//! WebAuthn setup and helpers around webauthn-rs.

use webauthn_rs::prelude::*;

use crate::{
    config::Config,
    error::{AppError, AppResult},
};

/// `None` when the public URL cannot host passkeys (plain HTTP or an IP address).
pub fn build(config: &Config) -> AppResult<Option<Webauthn>> {
    if !config.passkeys_available() {
        return Ok(None);
    }
    let rp_id = config
        .public_url
        .host_str()
        .ok_or_else(|| AppError::internal("public_url has no host"))?;
    let wa = WebauthnBuilder::new(rp_id, &config.public_url)
        .map_err(AppError::internal)?
        .rp_name("Katsanto")
        .build()
        .map_err(AppError::internal)?;
    Ok(Some(wa))
}

/// Registration options as JSON for the browser.
///
/// webauthn-rs registers passkeys without requiring a discoverable credential;
/// username-less login (and the browser's passkey autofill) needs one.
pub fn registration_options(ccr: CreationChallengeResponse) -> AppResult<serde_json::Value> {
    let mut v = serde_json::to_value(ccr).map_err(AppError::internal)?;
    let sel = &mut v["publicKey"]["authenticatorSelection"];
    if !sel.is_object() {
        *sel = serde_json::json!({});
    }
    sel["residentKey"] = "required".into();
    sel["requireResidentKey"] = true.into();
    Ok(v)
}

pub fn to_json(p: &Passkey) -> AppResult<String> {
    serde_json::to_string(p).map_err(AppError::internal)
}

pub fn from_json(s: &str) -> AppResult<Passkey> {
    serde_json::from_str(s).map_err(AppError::internal)
}

/// Default name for a new passkey when the client does not suggest one.
pub fn default_name(name: Option<String>) -> String {
    let name = name.unwrap_or_default();
    let name = name.trim();
    if name.is_empty() {
        "Passkey".into()
    } else {
        name.chars().take(64).collect()
    }
}

pub fn auth_failed(e: WebauthnError) -> AppError {
    tracing::info!(error = %e, "passkey verification failed");
    AppError::Unauthorized("Passkey verification failed".into())
}
