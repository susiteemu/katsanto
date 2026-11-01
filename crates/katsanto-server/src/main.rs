use std::{net::SocketAddr, sync::Arc, time::Duration};

use katsanto_app::{config::Config, routes, state::AppState};
use katsanto_db::{Db, challenges, sessions};
use katsanto_yt::{YoutubeClient, YoutubeConfig};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // `katsanto-server openapi` prints the API spec (for generating the web client).
    if std::env::args().nth(1).as_deref() == Some("openapi") {
        println!("{}", routes::openapi_json());
        return;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info")),
        )
        .init();

    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("configuration error: {e}");
            std::process::exit(2);
        }
    };
    std::fs::create_dir_all(&config.data_dir).expect("create data dir");
    // Relative paths depend on the working directory; say where data really goes.
    let data_dir = std::fs::canonicalize(&config.data_dir).unwrap_or(config.data_dir.clone());
    tracing::info!(data_dir = %data_dir.display(), "using data directory");
    match config.find_botguard() {
        Some(bin) => tracing::info!(botguard = %bin.display(), "PO tokens via rustypipe-botguard"),
        None => tracing::error!(
            "rustypipe-botguard not found (looked for {}). Search works, but videos won't play. \
             Install it with `cargo install rustypipe-botguard --root {}/tools`, or set botguard_bin.",
            config.botguard_bin().display(),
            data_dir.display()
        ),
    }

    let db = Db::open(&config.db_path()).await.expect("open database");
    let yt = YoutubeClient::new(YoutubeConfig {
        data_dir: config.data_dir.clone(),
        botguard_bin: config.botguard_bin(),
    })
    .expect("init YouTube client");

    match config.passkeys_unavailable_reason() {
        None => tracing::info!(public_url = %config.public_url, "passkeys enabled"),
        Some(reason) => tracing::warn!("{reason} Falling back to passwords only."),
    }

    tokio::spawn(cleanup(db.clone()));

    let bind = config.bind;
    let state = AppState::new(config, db, Arc::new(yt)).expect("app state");
    katsanto_app::refresh::spawn(state.clone());
    katsanto_app::websub::spawn(state.clone());
    let app = routes::router(
        state,
        routes::Options {
            rate_limit: true,
            assets: Some(katsanto_web::get),
        },
    );
    let listener = tokio::net::TcpListener::bind(bind).await.expect("bind");
    tracing::info!("listening on http://{bind}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
    .expect("server");
}

/// Hourly removal of expired sessions and WebAuthn challenges.
async fn cleanup(db: Db) {
    let mut tick = tokio::time::interval(Duration::from_secs(3600));
    loop {
        tick.tick().await;
        let s = sessions::delete_expired(db.pool()).await;
        let c = challenges::delete_expired(db.pool()).await;
        if let (Err(e), _) | (_, Err(e)) = (&s, &c) {
            tracing::warn!(error = %e, "cleanup failed");
        }
    }
}
