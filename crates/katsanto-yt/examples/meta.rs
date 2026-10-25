//! Live check of channel pages and video info: `--example meta -- VIDEO_ID`
use katsanto_yt::{YoutubeClient, YoutubeConfig, YoutubeSource, model::ChannelTab};

#[tokio::main]
async fn main() {
    let id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "5C_HPTJg5ek".into());
    let yt = YoutubeClient::new(YoutubeConfig {
        data_dir: std::env::temp_dir().join("katsanto-probe"),
        botguard_bin: "rustypipe-botguard".into(),
    })
    .unwrap();
    let info = yt.video_info(&id).await.expect("video_info");
    println!(
        "video: {} | channel {} ({:?} subs, avatar {}) | views {} likes {:?} | published {:?} | {} desc parts, {} chapters, {} related",
        info.title,
        info.channel.name,
        info.channel.subscriber_count,
        info.channel.avatar_url.is_some(),
        info.view_count,
        info.like_count,
        info.published_text,
        info.description.len(),
        info.chapters.len(),
        info.related.len()
    );
    for p in info
        .description
        .iter()
        .filter(|p| !matches!(p, katsanto_yt::model::TextPart::Text { .. }))
        .take(4)
    {
        println!("  {p:?}");
    }
    for tab in [ChannelTab::Videos, ChannelTab::Shorts] {
        let c = yt.channel(&info.channel.id, tab).await.expect("channel");
        println!(
            "channel {:?}: {} {:?} subs={:?} banner={} shorts={} live={} | {} videos, first: {:?} ({:?}s, short={})",
            tab,
            c.channel.name,
            c.channel.handle,
            c.channel.subscriber_count,
            c.channel.banner_url.is_some(),
            c.channel.has_shorts,
            c.channel.has_live,
            c.videos.items.len(),
            c.videos.items.first().map(|v| &v.title),
            c.videos.items.first().and_then(|v| v.duration_s),
            c.videos.items.first().is_some_and(|v| v.is_short)
        );
        if let Some(cur) = &c.videos.cursor {
            let more = yt.channel_more(cur).await.expect("channel_more");
            println!("  page 2: {} videos", more.items.len());
        }
    }
}
