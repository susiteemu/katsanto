//! The SvelteKit app (`web/build`), embedded into the binary in release builds.
//!
//! A crate of its own that only the binary depends on: a new web build then recompiles
//! this crate and relinks the binary, but not the server library.

use std::borrow::Cow;

use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../web/build"]
#[allow_missing = true]
struct Assets;

/// Contents of a file in the web build, by its path relative to `web/build`.
pub fn get(path: &str) -> Option<Cow<'static, [u8]>> {
    Assets::get(path).map(|f| f.data)
}
