//! Rebuild when the web app changes: rust-embed reads `web/build` when the crate is
//! compiled but doesn't tell cargo to watch it, so a binary built before
//! `npm run build` would keep serving no (or an old) web app.

use std::path::Path;

fn main() {
    let web_build = Path::new("../../web/build");
    println!("cargo:rerun-if-changed={}", web_build.display());
    if !web_build.join("index.html").exists() {
        println!(
            "cargo:warning=web/build/index.html is missing: the server will be built without \
             the web app. Run `npm ci && npm run build` in web/ first."
        );
    }
}
