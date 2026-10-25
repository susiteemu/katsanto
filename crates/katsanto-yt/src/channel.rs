//! Video lists parsed directly from Innertube responses: channel tabs (`browse`)
//! and related videos (`next`).
//!
//! rustypipe 0.11 predates YouTube's `lockupViewModel` items (it loses durations,
//! channels and Shorts), so these are parsed here. The parser is deliberately
//! lenient: it walks the JSON for the view models it knows and skips anything
//! it doesn't understand.

use serde_json::Value;

use crate::model::{ChannelInfo, ChannelPage, ChannelRef, ChannelTab, VideoCard, VideoPage};

/// `params` for the channel tab browse request.
pub fn tab_params(tab: ChannelTab) -> &'static str {
    match tab {
        ChannelTab::Videos => "EgZ2aWRlb3PyBgQKAjoA",
        ChannelTab::Shorts => "EgZzaG9ydHPyBgUKA5oBAA==",
        ChannelTab::Live => "EgdzdHJlYW1z8gYECgJ6AA==",
    }
}

/// Parse the first page of a channel tab. Returns `None` if the response is
/// not a channel page (e.g. unknown channel).
pub fn parse_channel_page(v: &Value, tab: ChannelTab) -> Option<ChannelPage> {
    let meta = find(v, "channelMetadataRenderer")?;
    let id = str_at(meta, &["externalId"])?.to_owned();
    let header = find(v, "pageHeaderViewModel");

    let header_parts: Vec<&str> = header
        .and_then(|h| find(h, "contentMetadataViewModel"))
        .map(metadata_parts)
        .unwrap_or_default();
    let handle = header_parts
        .iter()
        .find(|p| p.starts_with('@'))
        .map(|p| (*p).to_owned());
    let subscriber_count = header_parts
        .iter()
        .find(|p| p.contains("subscriber"))
        .and_then(|p| parse_count(p));
    let video_count = header_parts
        .iter()
        .find(|p| p.contains("video"))
        .and_then(|p| parse_count(p));

    let tabs: Vec<&str> = find_all(v, "tabRenderer")
        .into_iter()
        .filter_map(|t| str_at(t, &["title"]))
        .collect();

    let channel = ChannelInfo {
        name: str_at(meta, &["title"]).unwrap_or_default().to_owned(),
        avatar_url: header
            .and_then(|h| find(h, "avatarViewModel"))
            .and_then(|a| best_image(a, 64))
            .or_else(|| meta.get("avatar").and_then(|a| best_image(a, 64))),
        banner_url: header
            .and_then(|h| find(h, "imageBannerViewModel"))
            .and_then(|b| best_image(b, u32::MAX)),
        description: str_at(meta, &["description"])
            .unwrap_or_default()
            .to_owned(),
        has_shorts: tabs.contains(&"Shorts"),
        has_live: tabs.contains(&"Live"),
        id,
        handle,
        subscriber_count,
        video_count,
    };

    let grid = find(v, "richGridRenderer");
    let videos = grid.map(|g| parse_items(g, tab)).unwrap_or_default();
    Some(ChannelPage { channel, videos })
}

/// Related videos from a `next` (watch page) response.
pub fn parse_related(v: &Value) -> Vec<VideoCard> {
    let Some(secondary) = find(v, "secondaryResults") else {
        return Vec::new();
    };
    find_all(secondary, "lockupViewModel")
        .into_iter()
        .filter_map(|l| parse_lockup(l, ChannelTab::Videos))
        .collect()
}

/// Parse a continuation (`appendContinuationItemsAction`) response.
pub fn parse_continuation(v: &Value, tab: ChannelTab) -> VideoPage {
    find(v, "appendContinuationItemsAction")
        .map(|a| parse_items(a, tab))
        .unwrap_or_default()
}

fn parse_items(container: &Value, tab: ChannelTab) -> VideoPage {
    let mut items = Vec::new();
    for item in find_all(container, "richItemRenderer") {
        let parsed = if let Some(l) = find(item, "lockupViewModel") {
            parse_lockup(l, tab)
        } else if let Some(s) = find(item, "shortsLockupViewModel") {
            parse_short(s)
        } else {
            None
        };
        items.extend(parsed);
    }
    let cursor = find_all(container, "continuationItemRenderer")
        .into_iter()
        .find_map(|c| str_at(c, &["continuationEndpoint", "continuationCommand", "token"]))
        .map(str::to_owned);
    VideoPage { items, cursor }
}

fn parse_lockup(l: &Value, tab: ChannelTab) -> Option<VideoCard> {
    if str_at(l, &["contentType"]) != Some("LOCKUP_CONTENT_TYPE_VIDEO") {
        return None;
    }
    let id = str_at(l, &["contentId"])?.to_owned();
    let meta = find(l, "lockupMetadataViewModel")?;
    let title = str_at(meta, &["title", "content"])?.to_owned();

    let badges: Vec<&str> = find_all(l, "thumbnailBadgeViewModel")
        .into_iter()
        .filter_map(|b| str_at(b, &["text"]))
        .collect();
    let duration_s = badges.iter().find_map(|b| parse_duration(b));
    let badge_has = |word: &str| badges.iter().any(|b| b.to_ascii_uppercase().contains(word));

    let parts = find(meta, "contentMetadataViewModel")
        .map(labeled_parts)
        .unwrap_or_default();
    // Compact layouts show views as "1M" next to an eye icon; the accessibility
    // label still says "… views".
    let is_views = |(text, label): &(&str, Option<&str>)| {
        let has = |w: &str| text.contains(w) || label.is_some_and(|l| l.contains(w));
        has("view") || has("watching")
    };
    let is_date = |text: &str| {
        text.contains("ago") || text.starts_with("Streamed") || text.starts_with("Premiere")
    };
    // Outside a channel's own page the first part is the channel name, and the
    // lockup links to the channel.
    let channel = find_all(l, "browseEndpoint")
        .into_iter()
        .find_map(|b| str_at(b, &["browseId"]).filter(|id| id.starts_with("UC")))
        .zip(parts.first().filter(|p| !is_views(p) && !is_date(p.0)))
        .map(|(id, (name, _))| ChannelRef {
            id: id.to_owned(),
            name: (*name).to_owned(),
            avatar_url: find(meta, "avatarViewModel").and_then(|a| best_image(a, 64)),
        });
    let view_count = parts
        .iter()
        .find(|p| is_views(p))
        .and_then(|(text, _)| parse_count(text));
    let published_text = parts
        .iter()
        .find(|(text, _)| is_date(text))
        .map(|(text, _)| (*text).to_owned());

    Some(VideoCard {
        thumbnail_url: find(l, "contentImage").and_then(|i| best_image(i, u32::MAX)),
        id,
        title,
        channel,
        duration_s,
        published_at: published_text
            .as_deref()
            .and_then(|t| parse_ago(t, time::OffsetDateTime::now_utc().unix_timestamp())),
        published_text,
        view_count,
        is_live: badge_has("LIVE") && duration_s.is_none(),
        is_short: tab == ChannelTab::Shorts,
        is_upcoming: badge_has("UPCOMING")
            || parts.iter().any(|(text, _)| text.starts_with("Scheduled")),
    })
}

fn parse_short(s: &Value) -> Option<VideoCard> {
    let id = find(s, "reelWatchEndpoint")
        .and_then(|e| str_at(e, &["videoId"]))?
        .to_owned();
    Some(VideoCard {
        title: str_at(s, &["overlayMetadata", "primaryText", "content"])
            .unwrap_or_default()
            .to_owned(),
        view_count: str_at(s, &["overlayMetadata", "secondaryText", "content"])
            .and_then(parse_count),
        thumbnail_url: find(s, "thumbnailViewModel").and_then(|t| best_image(t, u32::MAX)),
        id,
        channel: None,
        duration_s: None,
        published_at: None,
        published_text: None,
        is_live: false,
        is_short: true,
        is_upcoming: false,
    })
}

/// Text of all `metadataParts` in a `contentMetadataViewModel`.
fn metadata_parts(m: &Value) -> Vec<&str> {
    labeled_parts(m).into_iter().map(|(text, _)| text).collect()
}

/// `metadataParts` text with its accessibility label, if any.
fn labeled_parts(m: &Value) -> Vec<(&str, Option<&str>)> {
    find_all(m, "metadataParts")
        .into_iter()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|p| {
            Some((
                str_at(p, &["text", "content"])?,
                str_at(p, &["accessibilityLabel"]),
            ))
        })
        .collect()
}

/// Smallest image source at least `min_width` wide, else the largest one.
fn best_image(v: &Value, min_width: u32) -> Option<String> {
    let sources: Vec<(u32, &str)> = find_all(v, "sources")
        .into_iter()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|s| {
            let url = s.get("url")?.as_str()?;
            let width = s.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
            Some((width, url))
        })
        .collect();
    let pick = if min_width == u32::MAX {
        sources.iter().max_by_key(|(w, _)| *w)
    } else {
        sources
            .iter()
            .filter(|(w, _)| *w >= min_width)
            .min_by_key(|(w, _)| *w)
            .or_else(|| sources.iter().max_by_key(|(w, _)| *w))
    };
    pick.map(|(_, url)| match url.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => (*url).to_owned(),
    })
}

/// `"6:04"` → 364, `"1:02:03"` → 3723.
pub fn parse_duration(s: &str) -> Option<u32> {
    let parts: Vec<u32> = s
        .trim()
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    match parts.as_slice() {
        [m, s] if *s < 60 => Some(m * 60 + s),
        [h, m, s] if *m < 60 && *s < 60 => Some(h * 3600 + m * 60 + s),
        _ => None,
    }
}

/// `"3 days ago"` (also "Streamed 2 hours ago") → an estimated unix time. Coarse —
/// "1 year ago" means anything up to two — but enough to order fresh uploads.
pub fn parse_ago(s: &str, now: i64) -> Option<i64> {
    let words: Vec<&str> = s.split_whitespace().collect();
    let at = words.iter().position(|w| *w == "ago")?;
    let [n, unit] = words.get(at.checked_sub(2)?..at)? else {
        return None;
    };
    let n: i64 = n.parse().ok()?;
    let secs = match unit.trim_end_matches('s') {
        "second" => 1,
        "minute" => 60,
        "hour" => 3600,
        "day" => 86400,
        "week" => 7 * 86400,
        "month" => 30 * 86400,
        "year" => 365 * 86400,
        _ => return None,
    };
    Some(now - n * secs)
}

/// `"4.31M subscribers"` → 4 310 000, `"1,234 views"` → 1234, `"No views"` → 0.
pub fn parse_count(s: &str) -> Option<u64> {
    let first = s.split_whitespace().next()?;
    if first.eq_ignore_ascii_case("no") {
        return Some(0);
    }
    let (num, mult) = match first.chars().last()? {
        'K' | 'k' => (&first[..first.len() - 1], 1e3),
        'M' => (&first[..first.len() - 1], 1e6),
        'B' => (&first[..first.len() - 1], 1e9),
        _ => (first, 1.0),
    };
    let n: f64 = num.replace(',', "").parse().ok()?;
    Some((n * mult).round() as u64)
}

fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(v, |v, k| v.get(k))?.as_str()
}

/// Depth-first search for the first value under `key`.
fn find<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m.get(key).or_else(|| m.values().find_map(|c| find(c, key))),
        Value::Array(a) => a.iter().find_map(|c| find(c, key)),
        _ => None,
    }
}

/// All values under `key`, in document order (not descending into matches).
fn find_all<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
    fn walk<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
        match v {
            Value::Object(m) => {
                for (k, c) in m {
                    if k == key {
                        out.push(c);
                    } else {
                        walk(c, key, out);
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|c| walk(c, key, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v, key, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_dates() {
        let now = 1_000_000_000;
        assert_eq!(parse_ago("17 hours ago", now), Some(now - 17 * 3600));
        assert_eq!(parse_ago("1 day ago", now), Some(now - 86400));
        assert_eq!(
            parse_ago("Streamed 2 weeks ago", now),
            Some(now - 14 * 86400)
        );
        assert_eq!(
            parse_ago("Premiered 3 months ago", now),
            Some(now - 90 * 86400)
        );
        assert_eq!(parse_ago("Scheduled for 10/26/26", now), None);
        assert_eq!(parse_ago("ago", now), None);
    }

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn parses_counts_and_durations() {
        assert_eq!(parse_count("4.31M subscribers"), Some(4_310_000));
        assert_eq!(parse_count("857 videos"), Some(857));
        assert_eq!(parse_count("1,234 views"), Some(1234));
        assert_eq!(parse_count("12K views"), Some(12_000));
        assert_eq!(parse_count("No views"), Some(0));
        assert_eq!(parse_count("views"), None);
        assert_eq!(parse_duration("6:04"), Some(364));
        assert_eq!(parse_duration("1:02:03"), Some(3723));
        assert_eq!(parse_duration("LIVE"), None);
        assert_eq!(parse_duration("1:75"), None);
    }

    #[test]
    fn channel_videos_tab() {
        let page = parse_channel_page(&fixture("channel_videos.json"), ChannelTab::Videos).unwrap();
        let c = &page.channel;
        assert_eq!(c.id, "UCsBjURrPoezykLs9EqgamOA");
        assert_eq!(c.name, "Fireship");
        assert_eq!(c.handle.as_deref(), Some("@Fireship"));
        assert_eq!(c.subscriber_count, Some(4_310_000));
        assert_eq!(c.video_count, Some(857));
        assert!(c.avatar_url.as_deref().unwrap().starts_with("https://yt3."));
        assert!(c.banner_url.is_some());
        assert!(c.has_shorts && c.has_live);
        assert!(!c.description.is_empty());

        let v = &page.videos;
        assert_eq!(v.items.len(), 30);
        assert!(v.cursor.is_some());
        let first = &v.items[0];
        assert_eq!(first.id, "SMGg7iisHmc");
        assert_eq!(first.duration_s, Some(364));
        assert_eq!(first.view_count, Some(1_000_000));
        assert_eq!(first.published_text.as_deref(), Some("17 hours ago"));
        assert!(
            first
                .thumbnail_url
                .as_deref()
                .unwrap()
                .contains("i.ytimg.com")
        );
        assert!(
            v.items
                .iter()
                .all(|i| i.duration_s.is_some() && !i.is_short)
        );
    }

    #[test]
    fn channel_continuation() {
        let page = parse_continuation(&fixture("channel_videos_cont.json"), ChannelTab::Videos);
        assert_eq!(page.items.len(), 30);
        assert!(page.cursor.is_some());
        assert!(page.items.iter().all(|i| i.duration_s.is_some()));
    }

    #[test]
    fn channel_shorts_tab() {
        let page = parse_channel_page(&fixture("channel_shorts.json"), ChannelTab::Shorts).unwrap();
        let v = &page.videos;
        assert_eq!(v.items.len(), 48);
        let first = &v.items[0];
        assert_eq!(first.id, "fwBIZRq-vzY");
        assert_eq!(first.title, "5 life-changing Linux tips");
        assert_eq!(first.view_count, Some(1_700_000));
        assert!(first.is_short && first.thumbnail_url.is_some());
    }

    #[test]
    fn channel_live_tab() {
        let page = parse_channel_page(&fixture("channel_live.json"), ChannelTab::Live).unwrap();
        assert!(!page.videos.items.is_empty());
        assert_eq!(page.videos.items[0].duration_s, Some(23 * 60 + 22));
    }

    #[test]
    fn related_videos() {
        let related = parse_related(&fixture("watch_next.json"));
        assert_eq!(related.len(), 20);
        let first = &related[0];
        assert_eq!(first.title, "Dart in 100 Seconds");
        assert_eq!(first.duration_s, Some(151));
        let c = first.channel.as_ref().unwrap();
        assert_eq!(
            (c.id.as_str(), c.name.as_str()),
            ("UCsBjURrPoezykLs9EqgamOA", "Fireship")
        );
        assert!(c.avatar_url.is_some());
        assert!(first.view_count.is_some() && first.published_text.is_some());
        assert!(related.iter().all(|r| r.channel.is_some()));
    }

    #[test]
    fn channel_page_items_have_no_channel_name() {
        // On a channel's own page the first metadata part is the view count.
        let page = parse_channel_page(&fixture("channel_videos.json"), ChannelTab::Videos).unwrap();
        assert!(page.videos.items.iter().all(|v| v.channel.is_none()));
    }

    #[test]
    fn non_channel_response() {
        assert!(
            parse_channel_page(&serde_json::json!({"error": {}}), ChannelTab::Videos).is_none()
        );
    }
}
