//! Channel RSS feeds (`/feeds/videos.xml`): the cheap way to poll for uploads.
//!
//! The feed has the latest 15 uploads with exact publish times, but no durations;
//! those come from the channel page (see [`crate::channel`]). Shorts are linked as
//! `/shorts/…`, which is the only hint the feed gives about them.

use quick_xml::{Reader, events::Event};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    model::{ChannelFeed, FeedVideo},
    source::{YtError, YtResult},
};

pub fn feed_url(channel_id: &str) -> String {
    format!("https://www.youtube.com/feeds/videos.xml?channel_id={channel_id}")
}

pub async fn fetch(http: &reqwest::Client, channel_id: &str) -> YtResult<ChannelFeed> {
    let res = http
        .get(feed_url(channel_id))
        .send()
        .await
        .map_err(|e| YtError::Other(format!("rss: {e}")))?;
    match res.status().as_u16() {
        200 => {}
        404 => return Err(YtError::NotFound(format!("channel {channel_id}"))),
        s => return Err(YtError::Other(format!("rss returned {s}"))),
    }
    let xml = res
        .text()
        .await
        .map_err(|e| YtError::Other(format!("rss: {e}")))?;
    parse(&xml)
}

/// Parse an Atom feed from YouTube.
pub fn parse(xml: &str) -> YtResult<ChannelFeed> {
    let mut reader = Reader::from_str(xml);
    let mut feed = ChannelFeed {
        channel_id: String::new(),
        channel_name: String::new(),
        videos: Vec::new(),
    };
    let mut entry: Option<FeedVideo> = None;
    // Element names from the root down to the current element.
    let mut path: Vec<String> = Vec::new();

    loop {
        let event = reader
            .read_event()
            .map_err(|e| YtError::Other(format!("rss xml: {e}")))?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let attr = |key: &str| {
                    e.attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == key.as_bytes())
                        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
                };
                match (name.as_str(), entry.as_mut()) {
                    ("entry", _) => entry = Some(FeedVideo::default()),
                    ("link", Some(v)) if attr("href").is_some_and(|h| h.contains("/shorts/")) => {
                        v.is_short = true;
                    }
                    ("media:thumbnail", Some(v)) => v.thumbnail_url = attr("url"),
                    ("media:statistics", Some(v)) => {
                        v.view_count = attr("views").and_then(|s| s.parse().ok());
                    }
                    _ => {}
                }
                // Self-closing elements have no `End` event.
                if matches!(event, Event::Start(_)) {
                    path.push(name);
                }
            }
            Event::Text(t) => {
                let text = t
                    .unescape()
                    .map_err(|e| YtError::Other(format!("rss xml: {e}")))?;
                let text = text.trim();
                let parent = path.get(path.len().wrapping_sub(2)).map(String::as_str);
                match (path.last().map(String::as_str), parent, entry.as_mut()) {
                    (Some("yt:videoId"), _, Some(v)) => v.id = text.to_owned(),
                    (Some("title"), Some("entry"), Some(v)) => v.title = text.to_owned(),
                    (Some("published"), Some("entry"), Some(v)) => {
                        v.published_at = OffsetDateTime::parse(text, &Rfc3339)
                            .map(|t| t.unix_timestamp())
                            .unwrap_or(0);
                    }
                    (Some("name"), Some("author"), None) => feed.channel_name = text.to_owned(),
                    (Some("uri"), Some("author"), None) => {
                        if let Some(id) = text.rsplit('/').next() {
                            feed.channel_id = id.to_owned();
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                if e.name().as_ref() == b"entry"
                    && let Some(v) = entry.take()
                    && !v.id.is_empty()
                {
                    feed.videos.push(v);
                }
                path.pop();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(feed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_channel_feed() {
        let xml = std::fs::read_to_string(format!(
            "{}/tests/fixtures/channel_rss.xml",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let feed = parse(&xml).unwrap();
        assert_eq!(feed.channel_id, "UCsBjURrPoezykLs9EqgamOA");
        assert_eq!(feed.channel_name, "Fireship");
        assert_eq!(feed.videos.len(), 15);
        let v = &feed.videos[0];
        assert_eq!(v.id, "SMGg7iisHmc");
        assert_eq!(v.title, "The most vulnerable week in internet history...");
        assert_eq!(v.published_at, 1_792_779_598);
        assert_eq!(v.view_count, Some(1_064_305));
        assert!(v.thumbnail_url.as_deref().unwrap().contains("SMGg7iisHmc"));
        assert!(!v.is_short);
        assert!(
            feed.videos
                .iter()
                .all(|v| v.published_at > 0 && !v.title.is_empty())
        );
    }

    #[test]
    fn detects_shorts_and_unescapes() {
        let xml = r#"<?xml version="1.0"?>
<feed xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns:media="http://search.yahoo.com/mrss/" xmlns="http://www.w3.org/2005/Atom">
 <title>A &amp; B</title>
 <author><name>A &amp; B</name><uri>https://www.youtube.com/channel/UCaaaaaaaaaaaaaaaaaaaaaa</uri></author>
 <entry>
  <yt:videoId>abcdefghijk</yt:videoId>
  <title>Tips &amp; tricks</title>
  <link rel="alternate" href="https://www.youtube.com/shorts/abcdefghijk"/>
  <published>2026-01-02T03:04:05+00:00</published>
  <media:group><media:title>Tips &amp; tricks</media:title></media:group>
 </entry>
</feed>"#;
        let feed = parse(xml).unwrap();
        assert_eq!(feed.channel_name, "A & B");
        assert_eq!(feed.videos[0].title, "Tips & tricks");
        assert!(feed.videos[0].is_short);
        assert_eq!(feed.videos[0].view_count, None);
    }

    #[test]
    fn rejects_broken_xml() {
        assert!(parse("<feed><entry>").is_ok_and(|f| f.videos.is_empty()));
        assert!(parse("<feed></entry>").is_err());
    }
}
