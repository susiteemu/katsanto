use std::path::PathBuf;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rustypipe::{
    client::{ClientType, RustyPipe, RustyPipeQuery},
    error::{Error as RpError, ExtractionError, UnavailabilityReason},
    model::{
        ChannelItem, Thumbnail, UrlTarget, VideoItem, YouTubeItem,
        paginator::{ContinuationEndpoint, Paginator},
        richtext::{RichText, TextComponent},
    },
    param::search_filter::{self, ItemType, SearchFilter},
};
use serde::{Deserialize, Serialize};

use crate::{
    channel,
    model::*,
    source::{YtError, YtResult},
};

#[derive(Clone, Debug, Default)]
pub(crate) struct RustyPipeConfig {
    /// Directory for rustypipe's cache file.
    pub storage_dir: Option<PathBuf>,
}

/// Search and metadata via rustypipe.
pub(crate) struct RustyPipeSource {
    rp: RustyPipe,
}

impl RustyPipeSource {
    pub(crate) fn new(config: RustyPipeConfig) -> YtResult<Self> {
        // Playback does not go through rustypipe (see `player`), so it never needs botguard.
        let mut builder = RustyPipe::builder().no_reporter().no_botguard();
        if let Some(dir) = config.storage_dir {
            builder = builder.storage_dir(dir);
        }
        let rp = builder.build().map_err(map_err)?;
        Ok(Self { rp })
    }

    fn query(&self) -> RustyPipeQuery {
        self.rp.query().unauthenticated()
    }
}

/// Contents of the opaque search cursor handed to clients.
#[derive(Serialize, Deserialize)]
struct Cursor {
    c: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    v: Option<String>,
}

fn encode_cursor(ctoken: Option<&str>, visitor_data: Option<&str>) -> Option<String> {
    let cursor = Cursor {
        c: ctoken?.to_owned(),
        v: visitor_data.map(str::to_owned),
    };
    Some(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&cursor).ok()?))
}

fn decode_cursor(s: &str) -> YtResult<Cursor> {
    URL_SAFE_NO_PAD
        .decode(s)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| YtError::BadRequest("invalid cursor".into()))
}

fn build_filter(params: &SearchParams) -> SearchFilter {
    let item_type = match params.kind {
        SearchKind::Video => ItemType::Video,
        SearchKind::Channel => ItemType::Channel,
    };
    let date = params.date.map(|d| match d {
        UploadDate::Hour => search_filter::UploadDate::Hour,
        UploadDate::Today => search_filter::UploadDate::Day,
        UploadDate::Week => search_filter::UploadDate::Week,
        UploadDate::Month => search_filter::UploadDate::Month,
        UploadDate::Year => search_filter::UploadDate::Year,
    });
    let length = params.length.map(|l| match l {
        Length::Short => search_filter::Length::Short,
        Length::Medium => search_filter::Length::Medium,
        Length::Long => search_filter::Length::Long,
    });
    let sort = params.sort.and_then(|s| match s {
        SortOrder::Relevance => None,
        SortOrder::Date => Some(search_filter::Order::Date),
        SortOrder::Views => Some(search_filter::Order::Views),
    });
    SearchFilter::new()
        .item_type(item_type)
        .date_opt(date)
        .length_opt(length)
        .sort_opt(sort)
}

/// Channel cursors carry the tab, which decides how items are parsed.
#[derive(Serialize, Deserialize)]
struct ChannelCursor {
    c: String,
    t: ChannelTab,
}

fn encode_channel_cursor(token: &str, tab: ChannelTab) -> String {
    let c = ChannelCursor {
        c: token.to_owned(),
        t: tab,
    };
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(&c).expect("serializable"))
}

fn decode_channel_cursor(s: &str) -> YtResult<(String, ChannelTab)> {
    URL_SAFE_NO_PAD
        .decode(s)
        .ok()
        .and_then(|b| serde_json::from_slice::<ChannelCursor>(&b).ok())
        .map(|c| (c.c, c.t))
        .ok_or_else(|| YtError::BadRequest("invalid cursor".into()))
}

fn text_parts(text: RichText) -> Vec<TextPart> {
    text.0
        .into_iter()
        .filter_map(|c| match c {
            TextComponent::Text { text, .. } => Some(TextPart::Text { text }),
            // The client puts links in `href`: never a `javascript:` or other scheme.
            TextComponent::Web { text, url } if is_web_url(&url) => {
                Some(TextPart::Link { text, url })
            }
            TextComponent::Web { text, .. } => Some(TextPart::Text { text }),
            TextComponent::YouTube { text, target } => Some(match target {
                UrlTarget::Video { id, start_time } => TextPart::Video {
                    text,
                    id,
                    start_s: start_time,
                },
                UrlTarget::Channel { id } => TextPart::Channel { text, id },
                UrlTarget::Playlist { id } => TextPart::Link {
                    text,
                    url: format!("https://www.youtube.com/playlist?list={id}"),
                },
                UrlTarget::Album { id } => TextPart::Link {
                    text,
                    url: format!("https://music.youtube.com/browse/{id}"),
                },
            }),
            // TextComponent is non-exhaustive.
            _ => None,
        })
        .collect()
}

fn is_web_url(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

fn page_from(p: Paginator<YouTubeItem>, corrected_query: Option<String>) -> SearchPage {
    let cursor = encode_cursor(p.ctoken.as_deref(), p.visitor_data.as_deref());
    let items = p
        .items
        .into_iter()
        .filter_map(|item| match item {
            YouTubeItem::Video(v) => Some(SearchItem::Video(video_card(v))),
            YouTubeItem::Channel(c) => Some(SearchItem::Channel(channel_card(c))),
            YouTubeItem::Playlist(_) => None,
        })
        .collect();
    SearchPage {
        items,
        cursor,
        corrected_query,
    }
}

impl RustyPipeSource {
    pub async fn search(&self, query: &str, params: &SearchParams) -> YtResult<SearchPage> {
        let res = self
            .query()
            .search_filter::<YouTubeItem, _>(query, &build_filter(params))
            .await
            .map_err(map_err)?;
        Ok(page_from(res.items, res.corrected_query))
    }

    pub async fn search_more(&self, cursor: &str) -> YtResult<SearchPage> {
        let cursor = decode_cursor(cursor)?;
        let p = self
            .query()
            .continuation::<YouTubeItem, _>(
                &cursor.c,
                ContinuationEndpoint::Search,
                cursor.v.as_deref(),
            )
            .await
            .map_err(map_err)?;
        Ok(page_from(p, None))
    }

    pub async fn search_suggestions(&self, query: &str) -> YtResult<Vec<String>> {
        self.query().search_suggestion(query).await.map_err(map_err)
    }

    pub async fn video_info(&self, video_id: &str) -> YtResult<VideoInfo> {
        // rustypipe parses the description and chapters; its related videos lack
        // channel, views and duration (new lockup format), so those come from our
        // own parse of the same endpoint.
        let next_body =
            serde_json::json!({ "videoId": video_id, "contentCheckOk": true, "racyCheckOk": true });
        let query = self.query();
        let (details, next) =
            tokio::join!(query.video_details(video_id), self.raw("next", &next_body));
        let d = details.map_err(map_err)?;
        let related = match next {
            Ok(v) => channel::parse_related(&v),
            Err(e) => {
                tracing::warn!(error = %e, "related videos unavailable");
                Vec::new()
            }
        };
        Ok(VideoInfo {
            id: d.id,
            title: d.name,
            description: text_parts(d.description),
            channel: ChannelCard {
                avatar_url: avatar(&d.channel.avatar),
                id: d.channel.id,
                name: d.channel.name,
                handle: None,
                subscriber_count: d.channel.subscriber_count,
                description: String::new(),
            },
            view_count: d.view_count,
            like_count: d.like_count,
            published_at: d.publish_date.map(|t| t.unix_timestamp()),
            published_text: d.publish_date_txt,
            is_live: d.is_live,
            chapters: d
                .chapters
                .into_iter()
                .map(|c| Chapter {
                    title: c.name,
                    start_s: c.position,
                })
                .collect(),
            related,
        })
    }

    /// Channel tabs are parsed by [`crate::channel`]; rustypipe only makes the request.
    pub async fn channel(&self, channel_id: &str, tab: ChannelTab) -> YtResult<ChannelPage> {
        let body =
            serde_json::json!({ "browseId": channel_id, "params": channel::tab_params(tab) });
        let raw = self.browse(&body).await?;
        let mut page = channel::parse_channel_page(&raw, tab)
            .ok_or_else(|| YtError::NotFound(format!("channel {channel_id}")))?;
        page.videos.cursor = page.videos.cursor.map(|c| encode_channel_cursor(&c, tab));
        Ok(page)
    }

    pub async fn channel_more(&self, cursor: &str) -> YtResult<VideoPage> {
        let (token, tab) = decode_channel_cursor(cursor)?;
        let raw = self
            .browse(&serde_json::json!({ "continuation": token }))
            .await?;
        let mut page = channel::parse_continuation(&raw, tab);
        page.cursor = page.cursor.map(|c| encode_channel_cursor(&c, tab));
        Ok(page)
    }

    async fn browse(&self, body: &serde_json::Value) -> YtResult<serde_json::Value> {
        self.raw("browse", body).await
    }

    /// Innertube request with rustypipe's client context, returning parsed JSON.
    async fn raw(&self, endpoint: &str, body: &serde_json::Value) -> YtResult<serde_json::Value> {
        let raw = self
            .query()
            .raw(ClientType::Desktop, endpoint, body)
            .await
            .map_err(map_err)?;
        serde_json::from_str(&raw).map_err(|e| YtError::Other(format!("{endpoint} response: {e}")))
    }

    pub async fn resolve_url(&self, url: &str) -> YtResult<ResolvedUrl> {
        match self
            .query()
            .resolve_url(url, false)
            .await
            .map_err(map_err)?
        {
            UrlTarget::Video { id, start_time } => Ok(ResolvedUrl::Video {
                id,
                start_s: start_time,
            }),
            UrlTarget::Channel { id } => Ok(ResolvedUrl::Channel { id }),
            UrlTarget::Playlist { id } => Ok(ResolvedUrl::Playlist { id }),
            UrlTarget::Album { .. } => Err(YtError::BadRequest(
                "YouTube Music albums are not supported".into(),
            )),
        }
    }
}

fn normalize_url(url: &str) -> String {
    match url.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_owned(),
    }
}

/// Largest thumbnail, which is what cards and the player poster use.
fn best_thumbnail(thumbs: &[Thumbnail]) -> Option<String> {
    thumbs
        .iter()
        .max_by_key(|t| t.width * t.height)
        .map(|t| normalize_url(&t.url))
}

/// Smallest avatar that is still at least 64px wide.
fn avatar(thumbs: &[Thumbnail]) -> Option<String> {
    thumbs
        .iter()
        .filter(|t| t.width >= 64)
        .min_by_key(|t| t.width)
        .or_else(|| thumbs.iter().max_by_key(|t| t.width))
        .map(|t| normalize_url(&t.url))
}

fn video_card(v: VideoItem) -> VideoCard {
    VideoCard {
        thumbnail_url: best_thumbnail(&v.thumbnail),
        channel: v.channel.map(|c| ChannelRef {
            avatar_url: avatar(&c.avatar),
            id: c.id,
            name: c.name,
        }),
        id: v.id,
        title: v.name,
        duration_s: v.duration,
        published_at: v.publish_date.map(|d| d.unix_timestamp()),
        published_text: v.publish_date_txt,
        view_count: v.view_count,
        is_live: v.is_live,
        is_short: v.is_short,
        is_upcoming: v.is_upcoming,
    }
}

fn channel_card(c: ChannelItem) -> ChannelCard {
    ChannelCard {
        avatar_url: avatar(&c.avatar),
        id: c.id,
        name: c.name,
        handle: c.handle,
        subscriber_count: c.subscriber_count,
        description: c.short_description,
    }
}

fn map_err(e: RpError) -> YtError {
    match e {
        RpError::Extraction(ExtractionError::NotFound { id, .. }) => YtError::NotFound(id),
        RpError::Extraction(ExtractionError::Unavailable { reason, msg }) => match reason {
            UnavailabilityReason::IpBan
            | UnavailabilityReason::VpnBan
            | UnavailabilityReason::Captcha => YtError::Blocked(format!("{reason}: {msg}")),
            _ => YtError::Unavailable(format!("{reason}: {msg}")),
        },
        RpError::Extraction(ExtractionError::BadRequest(msg)) => YtError::BadRequest(msg.into()),
        other => YtError::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_roundtrip() {
        let c = decode_cursor(&encode_cursor(Some("token"), Some("visitor")).unwrap()).unwrap();
        assert_eq!(c.c, "token");
        assert_eq!(c.v.as_deref(), Some("visitor"));
        assert!(encode_cursor(None, Some("visitor")).is_none());
        assert!(decode_cursor("garbage!").is_err());
    }

    #[test]
    fn only_web_links_stay_links() {
        assert!(is_web_url("https://example.com"));
        assert!(is_web_url("HTTP://example.com"));
        assert!(!is_web_url("javascript:alert(1)"));
        assert!(!is_web_url("data:text/html,x"));
    }

    #[test]
    fn protocol_relative_urls_get_https() {
        assert_eq!(
            normalize_url("//yt3.ggpht.com/a"),
            "https://yt3.ggpht.com/a"
        );
        assert_eq!(
            normalize_url("https://i.ytimg.com/a"),
            "https://i.ytimg.com/a"
        );
    }
}
