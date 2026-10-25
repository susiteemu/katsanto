//! Katsanto's own YouTube data types.
//!
//! These are decoupled from rustypipe's model so the rest of the application
//! does not change when the extractor does.

use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChannelRef {
    pub id: String,
    pub name: String,
    pub avatar_url: Option<String>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VideoCard {
    pub id: String,
    pub title: String,
    pub channel: Option<ChannelRef>,
    pub thumbnail_url: Option<String>,
    pub duration_s: Option<u32>,
    /// Unix timestamp, when YouTube provides an exact date.
    pub published_at: Option<i64>,
    /// Relative date as displayed by YouTube ("3 days ago").
    pub published_text: Option<String>,
    pub view_count: Option<u64>,
    pub is_live: bool,
    pub is_short: bool,
    pub is_upcoming: bool,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChannelCard {
    pub id: String,
    pub name: String,
    pub handle: Option<String>,
    pub avatar_url: Option<String>,
    pub subscriber_count: Option<u64>,
    pub description: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SearchItem {
    Video(VideoCard),
    Channel(ChannelCard),
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchPage {
    pub items: Vec<SearchItem>,
    /// Opaque token for fetching the next page.
    pub cursor: Option<String>,
    pub corrected_query: Option<String>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SearchKind {
    #[default]
    Video,
    Channel,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UploadDate {
    Hour,
    Today,
    Week,
    Month,
    Year,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Length {
    /// Under 4 minutes
    Short,
    /// 4–20 minutes
    Medium,
    /// Over 20 minutes
    Long,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    Relevance,
    Date,
    Views,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SearchParams {
    #[serde(default)]
    pub kind: SearchKind,
    pub date: Option<UploadDate>,
    pub length: Option<Length>,
    pub sort: Option<SortOrder>,
}

/// What a pasted YouTube URL points to.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResolvedUrl {
    Video { id: String, start_s: u32 },
    Channel { id: String },
    Playlist { id: String },
}

/// Inclusive byte range, as used in DASH `indexRange` / `Initialization@range`.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u32,
    pub end: u32,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    /// Video with audio (progressive download, usually ≤360p).
    Muxed,
    Video,
    Audio,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AudioTrackInfo {
    pub id: String,
    pub lang: Option<String>,
    pub name: String,
    pub is_default: bool,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Stream {
    /// Unique within a [`Playback`]: the itag, plus the audio track for multi-language videos.
    pub id: String,
    pub kind: StreamKind,
    pub itag: u32,
    /// Upstream googlevideo URL. Never sent to clients when proxying.
    #[serde(skip_serializing)]
    #[cfg_attr(feature = "openapi", schema(ignore))]
    pub url: String,
    /// Container mime type, e.g. `video/mp4`.
    pub mime_type: String,
    /// Codec string, e.g. `avc1.640028`.
    pub codecs: String,
    pub bitrate: u32,
    pub size: Option<u64>,
    pub init_range: Option<ByteRange>,
    pub index_range: Option<ByteRange>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u8>,
    pub quality_label: Option<String>,
    pub hdr: bool,
    pub audio_channels: Option<u8>,
    pub audio_track: Option<AudioTrackInfo>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Subtitle {
    pub lang: String,
    pub name: String,
    pub auto_generated: bool,
    #[serde(skip_serializing)]
    #[cfg_attr(feature = "openapi", schema(ignore))]
    pub url: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VideoDetails {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub channel: ChannelRef,
    pub duration_s: u32,
    pub view_count: Option<u64>,
    pub thumbnail_url: Option<String>,
    pub is_live: bool,
}

/// Playback data for a video. Stream URLs expire at `valid_until`.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Playback {
    pub details: VideoDetails,
    pub streams: Vec<Stream>,
    pub subtitles: Vec<Subtitle>,
    pub hls_manifest_url: Option<String>,
    /// Unix timestamp.
    pub valid_until: i64,
    /// User agent that must be used when requesting the stream URLs.
    #[serde(skip_serializing)]
    #[cfg_attr(feature = "openapi", schema(ignore))]
    pub user_agent: String,
}

impl Playback {
    pub fn stream(&self, id: &str) -> Option<&Stream> {
        self.streams.iter().find(|s| s.id == id)
    }

    pub fn is_expired(&self, now: i64, margin_s: i64) -> bool {
        now + margin_s >= self.valid_until
    }
}

/// A page of videos with an opaque cursor for the next one.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct VideoPage {
    pub items: Vec<VideoCard>,
    pub cursor: Option<String>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ChannelTab {
    #[default]
    Videos,
    Shorts,
    Live,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChannelInfo {
    pub id: String,
    pub name: String,
    pub handle: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub subscriber_count: Option<u64>,
    pub video_count: Option<u64>,
    pub description: String,
    pub has_shorts: bool,
    pub has_live: bool,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChannelPage {
    pub channel: ChannelInfo,
    pub videos: VideoPage,
}

/// A piece of a video description. YouTube links are resolved so the app can
/// keep them inside Katsanto (and turn timestamps into seeks).
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextPart {
    Text {
        text: String,
    },
    Link {
        text: String,
        url: String,
    },
    Video {
        text: String,
        id: String,
        start_s: u32,
    },
    Channel {
        text: String,
        id: String,
    },
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Chapter {
    pub title: String,
    pub start_s: u32,
}

/// Watch-page information beyond what playback needs.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VideoInfo {
    pub id: String,
    pub title: String,
    pub description: Vec<TextPart>,
    pub channel: ChannelCard,
    pub view_count: u64,
    pub like_count: Option<u32>,
    pub published_at: Option<i64>,
    pub published_text: Option<String>,
    pub is_live: bool,
    pub chapters: Vec<Chapter>,
    pub related: Vec<VideoCard>,
}

/// A channel's RSS feed: its latest uploads with exact publish times.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ChannelFeed {
    pub channel_id: String,
    pub channel_name: String,
    pub videos: Vec<FeedVideo>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct FeedVideo {
    pub id: String,
    pub title: String,
    /// Unix timestamp.
    pub published_at: i64,
    pub thumbnail_url: Option<String>,
    pub view_count: Option<u64>,
    /// Linked as `/shorts/…` in the feed.
    pub is_short: bool,
}
