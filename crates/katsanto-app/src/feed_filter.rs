//! Saved-feed filters: what a feed shows from the user's subscriptions.
//!
//! Most of a filter compiles to SQL ([`katsanto_db::feed::FeedQuery`]). Title
//! matching runs here because SQLite only folds ASCII case (no "Ä" = "ä") and
//! has no regex; pages are filled by scanning in batches.

use katsanto_db::{
    feed::{self, FeedQuery, FeedRow, Watched},
    now,
};
use katsanto_yt::model::{ChannelRef, VideoCard};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    error::{AppError, AppResult},
    routes::{api::proxy_card_images, watch::ProgressInfo},
    state::AppState,
};

pub const PAGE: usize = 30;
/// Rows fetched per round when titles are filtered in Rust.
const BATCH: i64 = 200;
/// Stop scanning after this many rows; the client can ask for more.
const MAX_SCAN: usize = 5_000;
const MAX_LIST: usize = 200;
const MAX_TEXT: usize = 100;
const MAX_REGEX: usize = 200;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WatchedFilter {
    #[default]
    All,
    HideFinished,
    OnlyUnwatched,
    InProgress,
}

/// Which videos a feed shows. Every field is optional; the default shows
/// everything from all subscriptions except Shorts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct FeedFilter {
    /// Only channels with any of these tags (or listed in `channels`).
    pub tag_ids: Vec<i64>,
    /// Channels to include besides the tagged ones.
    pub channels: Vec<String>,
    pub exclude_channels: Vec<String>,
    /// Title contains any of these (case-insensitive). Empty: no restriction.
    pub title_any: Vec<String>,
    /// Title contains none of these.
    pub title_none: Vec<String>,
    /// Case-insensitive regular expression the title must match.
    pub title_regex: Option<String>,
    pub min_duration_s: Option<u32>,
    pub max_duration_s: Option<u32>,
    /// Include Shorts.
    pub shorts: bool,
    /// Include live streams and premieres.
    pub live: bool,
    pub watched: WatchedFilter,
    pub published_within_days: Option<u32>,
}

impl Default for FeedFilter {
    fn default() -> Self {
        Self {
            tag_ids: Vec::new(),
            channels: Vec::new(),
            exclude_channels: Vec::new(),
            title_any: Vec::new(),
            title_none: Vec::new(),
            title_regex: None,
            min_duration_s: None,
            max_duration_s: None,
            shorts: false,
            live: true,
            watched: WatchedFilter::All,
            published_within_days: None,
        }
    }
}

impl FeedFilter {
    /// Trim keyword lists and reject oversized or invalid input.
    pub fn normalized(mut self) -> AppResult<Self> {
        let clean = |list: Vec<String>| -> AppResult<Vec<String>> {
            let list: Vec<String> = list
                .into_iter()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();
            if list.len() > MAX_LIST || list.iter().any(|s| s.chars().count() > MAX_TEXT) {
                return Err(AppError::BadRequest("filter is too large".into()));
            }
            Ok(list)
        };
        self.title_any = clean(self.title_any)?;
        self.title_none = clean(self.title_none)?;
        self.channels = clean(self.channels)?;
        self.exclude_channels = clean(self.exclude_channels)?;
        if self.tag_ids.len() > MAX_LIST {
            return Err(AppError::BadRequest("filter is too large".into()));
        }
        self.title_regex = self
            .title_regex
            .map(|r| r.trim().to_owned())
            .filter(|r| !r.is_empty());
        if let Some(r) = &self.title_regex {
            if r.chars().count() > MAX_REGEX {
                return Err(AppError::BadRequest("regex is too long".into()));
            }
            compile(r)?;
        }
        if let (Some(min), Some(max)) = (self.min_duration_s, self.max_duration_s)
            && min > max
        {
            return Err(AppError::BadRequest(
                "minimum duration is above the maximum".into(),
            ));
        }
        Ok(self)
    }

    fn query(&self, after: Option<(i64, String)>, limit: i64) -> FeedQuery {
        FeedQuery {
            include_shorts: self.shorts,
            include_live: self.live,
            watched: match self.watched {
                WatchedFilter::All => Watched::All,
                WatchedFilter::HideFinished => Watched::HideFinished,
                WatchedFilter::OnlyUnwatched => Watched::OnlyUnwatched,
                WatchedFilter::InProgress => Watched::InProgress,
            },
            tag_ids: self.tag_ids.clone(),
            channels: self.channels.clone(),
            exclude_channels: self.exclude_channels.clone(),
            min_duration_s: self.min_duration_s.map(i64::from),
            max_duration_s: self.max_duration_s.map(i64::from),
            published_after: self
                .published_within_days
                .map(|d| now() - i64::from(d) * 86400),
            after,
            limit,
        }
    }

    fn matcher(&self) -> AppResult<TitleMatcher> {
        Ok(TitleMatcher {
            any: self.title_any.iter().map(|s| s.to_lowercase()).collect(),
            none: self.title_none.iter().map(|s| s.to_lowercase()).collect(),
            regex: self.title_regex.as_deref().map(compile).transpose()?,
        })
    }
}

fn compile(pattern: &str) -> AppResult<Regex> {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .size_limit(1 << 20)
        .build()
        .map_err(|e| AppError::BadRequest(format!("invalid regex: {e}")))
}

struct TitleMatcher {
    any: Vec<String>,
    none: Vec<String>,
    regex: Option<Regex>,
}

impl TitleMatcher {
    fn is_trivial(&self) -> bool {
        self.any.is_empty() && self.none.is_empty() && self.regex.is_none()
    }

    fn matches(&self, title: &str) -> bool {
        let lower = title.to_lowercase();
        (self.any.is_empty() || self.any.iter().any(|k| lower.contains(k)))
            && !self.none.iter().any(|k| lower.contains(k))
            && self.regex.as_ref().is_none_or(|r| r.is_match(title))
    }
}

#[derive(Serialize, ToSchema)]
pub struct FeedItem {
    pub video: VideoCard,
    pub progress: Option<ProgressInfo>,
}

#[derive(Serialize, ToSchema)]
pub struct FeedPage {
    pub items: Vec<FeedItem>,
    /// More results after this; `null` when the feed is exhausted.
    pub cursor: Option<String>,
}

/// One page of a feed for `user_id`.
pub async fn load_page(
    st: &AppState,
    user_id: i64,
    filter: &FeedFilter,
    cursor: Option<&str>,
) -> AppResult<FeedPage> {
    let mut after = cursor.map(parse_cursor).transpose()?;
    let matcher = filter.matcher()?;
    let batch = if matcher.is_trivial() {
        PAGE as i64
    } else {
        BATCH
    };
    let mut items = Vec::new();
    let mut scanned = 0;
    let exhausted = loop {
        let rows = feed::feed(st.db.pool(), user_id, &filter.query(after.clone(), batch)).await?;
        let fetched = rows.len();
        for row in rows {
            after = Some((row.published_at, row.id.clone()));
            scanned += 1;
            if matcher.matches(&row.title) {
                items.push(feed_item(row));
                if items.len() == PAGE {
                    break;
                }
            }
        }
        if (fetched as i64) < batch && items.len() < PAGE {
            break true;
        }
        if items.len() == PAGE || scanned >= MAX_SCAN {
            break false;
        }
    };
    Ok(FeedPage {
        items,
        cursor: if exhausted {
            None
        } else {
            after.map(|(t, id)| format!("{t}:{id}"))
        },
    })
}

fn parse_cursor(c: &str) -> AppResult<(i64, String)> {
    c.split_once(':')
        .and_then(|(t, id)| Some((t.parse().ok()?, id.to_owned())))
        .ok_or_else(|| AppError::BadRequest("invalid cursor".into()))
}

fn feed_item(r: FeedRow) -> FeedItem {
    let progress = match (r.position_s, r.watched_duration_s) {
        (Some(position_s), Some(duration_s)) => Some(ProgressInfo {
            video_id: r.id.clone(),
            position_s,
            duration_s,
            finished: r.finished.unwrap_or(false),
            updated_at: r.progress_updated_at.unwrap_or(0),
        }),
        _ => None,
    };
    let mut video = VideoCard {
        id: r.id,
        title: r.title,
        channel: Some(ChannelRef {
            id: r.channel_id,
            name: r.channel_name,
            avatar_url: r.channel_avatar_url,
        }),
        thumbnail_url: r.thumbnail_url,
        duration_s: r.duration_s.map(|d| d as u32),
        published_at: Some(r.published_at),
        published_text: None,
        view_count: r.view_count.map(|n| n as u64),
        is_live: r.is_live,
        is_short: r.is_short,
        is_upcoming: false,
    };
    proxy_card_images(&mut video);
    FeedItem { video, progress }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(json: serde_json::Value) -> AppResult<FeedFilter> {
        serde_json::from_value::<FeedFilter>(json)
            .unwrap()
            .normalized()
    }

    #[test]
    fn defaults() {
        let f = filter(serde_json::json!({})).unwrap();
        assert!(!f.shorts && f.live && f.watched == WatchedFilter::All);
    }

    #[test]
    fn title_matching_is_unicode_case_insensitive() {
        let f = filter(serde_json::json!({
            "title_any": ["rust", " ÄÄNI "],
            "title_none": ["#shorts", ""],
        }))
        .unwrap();
        assert_eq!(f.title_any, ["rust", "ÄÄNI"]);
        let m = f.matcher().unwrap();
        assert!(m.matches("Learning RUST in 2026"));
        assert!(m.matches("Hyvä ääni"));
        assert!(!m.matches("Rust tips #Shorts"));
        assert!(!m.matches("Cooking"));
    }

    #[test]
    fn regex_filters() {
        let m = filter(serde_json::json!({ "title_regex": r"\bpart \d+" }))
            .unwrap()
            .matcher()
            .unwrap();
        assert!(m.matches("Building a kernel, PART 3"));
        assert!(!m.matches("Departure"));
        assert!(filter(serde_json::json!({ "title_regex": "(" })).is_err());
        assert!(filter(serde_json::json!({ "title_regex": "a".repeat(300) })).is_err());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(
            filter(serde_json::json!({ "min_duration_s": 600, "max_duration_s": 60 })).is_err()
        );
        assert!(filter(serde_json::json!({ "title_any": vec!["x"; 500] })).is_err());
        assert!(filter(serde_json::json!({ "title_any": ["x".repeat(200)] })).is_err());
    }

    #[test]
    fn cursors() {
        assert_eq!(parse_cursor("123:abc").unwrap(), (123, "abc".into()));
        assert_eq!(parse_cursor("123:a:b").unwrap(), (123, "a:b".into()));
        assert!(parse_cursor("x:abc").is_err());
    }
}
