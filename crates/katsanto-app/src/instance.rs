//! Instance settings editable by admins.

use katsanto_db::{Executor, settings};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::AppResult;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationMode {
    /// Anyone can register.
    Open,
    /// Registration needs an invite link from an admin.
    #[default]
    Invite,
    /// Only admins create accounts.
    Closed,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct InstanceSettings {
    pub registration_mode: RegistrationMode,
    /// Allow passwords as a fallback to passkeys.
    pub passwords_enabled: bool,
}

impl Default for InstanceSettings {
    fn default() -> Self {
        Self {
            registration_mode: RegistrationMode::Invite,
            passwords_enabled: true,
        }
    }
}

const KEY: &str = "instance";

pub async fn load(e: impl Executor<'_>) -> AppResult<InstanceSettings> {
    Ok(settings::get(e, KEY).await?.unwrap_or_default())
}

pub async fn store(e: impl Executor<'_>, s: &InstanceSettings) -> AppResult<()> {
    Ok(settings::set(e, KEY, s).await?)
}
