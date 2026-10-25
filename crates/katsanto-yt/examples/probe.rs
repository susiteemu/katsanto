//! Live check against YouTube:
//! `KATSANTO_BOTGUARD_BIN=... cargo run -p katsanto-yt --example probe -- "query"`

use katsanto_yt::{YoutubeClient, YoutubeConfig, YoutubeSource, model::*};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let query = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "rust programming".into());
    let yt = YoutubeClient::new(YoutubeConfig {
        data_dir: std::env::temp_dir().join("katsanto-probe"),
        botguard_bin: std::env::var_os("KATSANTO_BOTGUARD_BIN")
            .unwrap_or_else(|| "rustypipe-botguard".into())
            .into(),
    })
    .expect("client");

    let page = yt
        .search(&query, &SearchParams::default())
        .await
        .expect("search");
    println!(
        "search: {} items, cursor: {}",
        page.items.len(),
        page.cursor.is_some()
    );
    let video = page
        .items
        .iter()
        .find_map(|i| match i {
            SearchItem::Video(v) if !v.is_live => Some(v.clone()),
            _ => None,
        })
        .expect("a video result");

    let started = std::time::Instant::now();
    let pb = yt.playback(&video.id).await.expect("playback");
    println!(
        "playback {} ({:?}): {} streams, {} subtitles",
        pb.details.title,
        started.elapsed(),
        pb.streams.len(),
        pb.subtitles.len()
    );
    for s in &pb.streams {
        println!(
            "  {:?} id={} {} {} {}x{} br={} size={:?}",
            s.kind,
            s.id,
            s.mime_type,
            s.codecs,
            s.width.unwrap_or(0),
            s.height.unwrap_or(0),
            s.bitrate,
            s.size
        );
    }
}
