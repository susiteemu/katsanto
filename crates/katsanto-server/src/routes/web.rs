//! The SvelteKit app (`web/build`), embedded into the binary in release builds.

use axum::{
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use regex::Regex;
use rust_embed::Embed;
use sha2::{Digest, Sha256};

#[derive(Embed)]
#[folder = "../../web/build"]
#[allow_missing = true]
struct Assets;

/// Serve a static file, or the app shell for client-side routes.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") || path.starts_with("proxy/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Some(res) = file(path) {
        return res;
    }
    // Paths with an extension are files that don't exist; others are app routes.
    if path
        .rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.'))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    file("index.html").unwrap_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "The web app is not built into this server. Run `npm run build` in web/, then rebuild \
             the server (or use the Vite dev server).",
        )
            .into_response()
    })
}

fn file(path: &str) -> Option<Response> {
    let path = if path.is_empty() { "index.html" } else { path };
    let asset = Assets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    // Hashed build output never changes; everything else must be revalidated.
    let cache = if path.starts_with("_app/immutable/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Some(
        (
            [
                (header::CONTENT_TYPE, mime.as_ref().to_owned()),
                (header::CACHE_CONTROL, cache.to_owned()),
            ],
            asset.data,
        )
            .into_response(),
    )
}

/// CSP sources (`'sha256-…'`) for the app shell's inline scripts (app.html's pre-paint
/// script and SvelteKit's bootstrap), so the policy needs no `'unsafe-inline'`.
pub fn inline_script_hashes() -> Vec<String> {
    match Assets::get("index.html") {
        Some(f) => script_hashes(&String::from_utf8_lossy(&f.data)),
        None => Vec::new(),
    }
}

fn script_hashes(html: &str) -> Vec<String> {
    let re = Regex::new(r"(?s)<script(\s[^>]*)?>(.*?)</script>").expect("valid regex");
    re.captures_iter(html)
        .filter(|c| {
            !c.get(1)
                .is_some_and(|attrs| attrs.as_str().contains("src="))
        })
        .map(|c| {
            let digest = Sha256::digest(c[2].as_bytes());
            format!("'sha256-{}'", STANDARD.encode(digest))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_inline_scripts_only() {
        let html = r#"<script>a()</script><script type="module" src="/x.js"></script>
            <script type="module">
b()
</script>"#;
        let hashes = script_hashes(html);
        assert_eq!(hashes.len(), 2);
        // echo -n 'a()' | openssl dgst -sha256 -binary | base64
        assert_eq!(
            hashes[0],
            "'sha256-qVpDBgj7bpq5hMAcGp3AOc79J3Y1Z4HvySTwKrWDoy4='"
        );
    }
}
