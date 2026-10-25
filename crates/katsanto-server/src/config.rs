use std::{
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::client_ip::IpNet;

/// Server configuration: defaults, then `katsanto.toml` (or the file named by
/// `KATSANTO_CONFIG`), then `KATSANTO_*` environment variables.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub bind: SocketAddr,
    /// URL users open Katsanto at. Determines cookie security and whether
    /// passkeys are available (they need HTTPS and a domain name, or localhost).
    pub public_url: Url,
    pub data_dir: PathBuf,
    /// `rustypipe-botguard` for PO tokens. Defaults to
    /// `<data_dir>/tools/bin/rustypipe-botguard` if it exists, otherwise `PATH`.
    pub botguard_bin: Option<PathBuf>,
    /// Stream video through the server (default) instead of handing out googlevideo URLs.
    pub proxy_video: bool,
    /// SponsorBlock API (also serves DeArrow titles); empty disables both.
    pub sponsorblock_api: String,
    /// DeArrow thumbnail generator; empty disables DeArrow thumbnails.
    pub dearrow_thumbnails: String,
    /// Push notifications for new uploads from YouTube's WebSub hub. Needs `public_url`
    /// to be reachable from the internet.
    pub websub: bool,
    pub websub_hub: String,
    /// Reverse proxies (addresses or networks like `172.16.0.0/12`) whose
    /// `X-Forwarded-For` tells the client's address, for per-client rate limiting.
    pub trusted_proxies: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:3000".parse().unwrap(),
            public_url: Url::parse("http://localhost:3000").unwrap(),
            data_dir: "data".into(),
            botguard_bin: None,
            proxy_video: true,
            sponsorblock_api: "https://sponsor.ajay.app".into(),
            dearrow_thumbnails: "https://dearrow-thumb.ajay.app".into(),
            websub: false,
            websub_hub: "https://pubsubhubbub.appspot.com/subscribe".into(),
            trusted_proxies: Vec::new(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let file = std::env::var("KATSANTO_CONFIG").unwrap_or_else(|_| "katsanto.toml".into());
        let config: Self = Self::figment(Path::new(&file))
            .extract()
            .map_err(|e| e.to_string())?;
        config.trusted_proxy_nets()?;
        Ok(config)
    }

    pub fn trusted_proxy_nets(&self) -> Result<Vec<IpNet>, String> {
        self.trusted_proxies
            .iter()
            .map(|s| IpNet::parse(s).map_err(|e| format!("trusted_proxies: {e}")))
            .collect()
    }

    fn figment(file: &Path) -> Figment {
        Figment::from(Serialized::defaults(Config::default()))
            .merge(Toml::file(file))
            .merge(Env::prefixed("KATSANTO_").ignore(&["config"]))
    }

    pub fn botguard_bin(&self) -> PathBuf {
        self.botguard_bin.clone().unwrap_or_else(|| {
            let local = self.data_dir.join("tools/bin/rustypipe-botguard");
            if local.exists() {
                local
            } else {
                "rustypipe-botguard".into()
            }
        })
    }

    /// The botguard binary if it can be found (as a path, or by name in `PATH`).
    pub fn find_botguard(&self) -> Option<PathBuf> {
        let bin = self.botguard_bin();
        if bin.components().count() > 1 {
            return bin.is_file().then_some(bin);
        }
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(&bin))
                .find(|p| p.is_file())
        })
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("katsanto.db")
    }

    /// Cookies get the `Secure` flag when served over HTTPS.
    pub fn secure_cookies(&self) -> bool {
        self.public_url.scheme() == "https"
    }

    /// WebAuthn requires a secure context and a domain name as relying party ID:
    /// `https://<domain>`, or `http://localhost` for development.
    pub fn passkeys_available(&self) -> bool {
        let Some(host) = self.public_url.host_str() else {
            return false;
        };
        let is_ip = host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok();
        match self.public_url.scheme() {
            "https" => !is_ip,
            "http" => host == "localhost",
            _ => false,
        }
    }

    /// Why passkeys are unavailable, for the admin UI.
    pub fn passkeys_unavailable_reason(&self) -> Option<String> {
        (!self.passkeys_available()).then(|| {
            format!(
                "Passkeys need Katsanto to be served over HTTPS with a domain name \
                 (public_url is {}). Put Katsanto behind a reverse proxy with TLS \
                 (e.g. Caddy, or Tailscale HTTPS) and set public_url accordingly.",
                self.public_url
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_url(url: &str) -> Config {
        Config {
            public_url: Url::parse(url).unwrap(),
            ..Config::default()
        }
    }

    #[test]
    fn passkeys_need_https_domain_or_localhost() {
        assert!(with_url("https://katsanto.example.com").passkeys_available());
        assert!(with_url("https://tv.tail1234.ts.net:8443").passkeys_available());
        assert!(with_url("http://localhost:3000").passkeys_available());
        assert!(!with_url("http://katsanto.example.com").passkeys_available());
        assert!(!with_url("http://192.168.1.10:3000").passkeys_available());
        assert!(!with_url("https://192.168.1.10").passkeys_available());
        assert!(!with_url("https://[::1]:3000").passkeys_available());
        assert!(
            with_url("http://192.168.1.10:3000")
                .passkeys_unavailable_reason()
                .is_some()
        );
    }

    #[test]
    #[allow(clippy::result_large_err)]
    fn layers_file_and_env() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "k.toml",
                r#"
                public_url = "https://k.example.com"
                proxy_video = false
                "#,
            )?;
            jail.set_env("KATSANTO_BIND", "0.0.0.0:8080");
            jail.set_env(
                "KATSANTO_TRUSTED_PROXIES",
                r#"["172.16.0.0/12", "10.0.0.1"]"#,
            );
            let c: Config = Config::figment(Path::new("k.toml")).extract()?;
            assert_eq!(c.public_url.as_str(), "https://k.example.com/");
            assert!(!c.proxy_video);
            assert_eq!(c.bind.port(), 8080);
            assert!(c.secure_cookies());
            assert_eq!(c.trusted_proxy_nets().unwrap().len(), 2);
            Ok(())
        });
    }
}
