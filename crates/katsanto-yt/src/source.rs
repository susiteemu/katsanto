use async_trait::async_trait;

use crate::model::{
    ChannelFeed, ChannelPage, ChannelTab, Playback, ResolvedUrl, SearchPage, SearchParams,
    VideoInfo, VideoPage,
};

#[derive(Debug, thiserror::Error)]
pub enum YtError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unavailable: {0}")]
    Unavailable(String),
    #[error("invalid input: {0}")]
    BadRequest(String),
    #[error("blocked by YouTube: {0}")]
    Blocked(String),
    #[error("youtube error: {0}")]
    Other(String),
}

pub type YtResult<T> = Result<T, YtError>;

/// Everything Katsanto needs from YouTube. Implemented by [`crate::RustyPipeSource`]
/// and by test doubles.
#[async_trait]
pub trait YoutubeSource: Send + Sync + 'static {
    async fn search(&self, query: &str, params: &SearchParams) -> YtResult<SearchPage>;

    /// Fetch the next page for a cursor returned by [`YoutubeSource::search`].
    async fn search_more(&self, cursor: &str) -> YtResult<SearchPage>;

    async fn search_suggestions(&self, query: &str) -> YtResult<Vec<String>>;

    async fn resolve_url(&self, url: &str) -> YtResult<ResolvedUrl>;

    async fn playback(&self, video_id: &str) -> YtResult<Playback>;

    /// Description, channel, chapters and related videos for the watch page.
    async fn video_info(&self, video_id: &str) -> YtResult<VideoInfo>;

    async fn channel(&self, channel_id: &str, tab: ChannelTab) -> YtResult<ChannelPage>;

    /// Next page of a channel's videos for a cursor from [`YoutubeSource::channel`].
    async fn channel_more(&self, cursor: &str) -> YtResult<VideoPage>;

    /// The channel's RSS feed (latest uploads), for polling subscriptions.
    async fn channel_feed(&self, channel_id: &str) -> YtResult<ChannelFeed>;

    /// Forget the current YouTube session so the next playback request starts a
    /// new one. Called when stream URLs are rejected despite being fresh.
    async fn reset_session(&self) {}
}
