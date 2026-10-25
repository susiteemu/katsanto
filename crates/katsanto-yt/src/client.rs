use std::path::PathBuf;

use async_trait::async_trait;

use crate::{
    model::{
        ChannelFeed, ChannelPage, ChannelTab, Playback, ResolvedUrl, SearchPage, SearchParams,
        VideoInfo, VideoPage,
    },
    player::PlayerClient,
    rss,
    rustypipe_source::{RustyPipeConfig, RustyPipeSource},
    solver::{Solver, SolverScripts},
    source::{YoutubeSource, YtResult},
};

#[derive(Clone, Debug)]
pub struct YoutubeConfig {
    /// Cache directory for rustypipe, preprocessed players and the botguard snapshot.
    /// `<data_dir>/ejs/` may contain a newer solver than the vendored one.
    pub data_dir: PathBuf,
    /// `rustypipe-botguard` binary, used to mint PO tokens for streams.
    pub botguard_bin: PathBuf,
}

/// The production [`YoutubeSource`]: rustypipe for search and metadata, our own
/// Innertube player client for playback.
pub struct YoutubeClient {
    meta: RustyPipeSource,
    player: PlayerClient,
    /// Plain HTTP client for RSS feeds.
    http: reqwest::Client,
}

impl YoutubeClient {
    pub fn new(config: YoutubeConfig) -> YtResult<Self> {
        let meta = RustyPipeSource::new(RustyPipeConfig {
            storage_dir: Some(config.data_dir.clone()),
        })?;
        let solver = Solver::spawn(SolverScripts::from_dir_or_vendored(
            &config.data_dir.join("ejs"),
        ))?;
        let player = PlayerClient::new(solver, config.botguard_bin, config.data_dir)?;
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|e| crate::YtError::Other(e.to_string()))?;
        Ok(Self { meta, player, http })
    }
}

#[async_trait]
impl YoutubeSource for YoutubeClient {
    async fn search(&self, query: &str, params: &SearchParams) -> YtResult<SearchPage> {
        self.meta.search(query, params).await
    }

    async fn search_more(&self, cursor: &str) -> YtResult<SearchPage> {
        self.meta.search_more(cursor).await
    }

    async fn search_suggestions(&self, query: &str) -> YtResult<Vec<String>> {
        self.meta.search_suggestions(query).await
    }

    async fn resolve_url(&self, url: &str) -> YtResult<ResolvedUrl> {
        self.meta.resolve_url(url).await
    }

    async fn playback(&self, video_id: &str) -> YtResult<Playback> {
        self.player.playback(video_id).await
    }

    async fn video_info(&self, video_id: &str) -> YtResult<VideoInfo> {
        self.meta.video_info(video_id).await
    }

    async fn channel(&self, channel_id: &str, tab: ChannelTab) -> YtResult<ChannelPage> {
        self.meta.channel(channel_id, tab).await
    }

    async fn channel_more(&self, cursor: &str) -> YtResult<VideoPage> {
        self.meta.channel_more(cursor).await
    }

    async fn channel_feed(&self, channel_id: &str) -> YtResult<ChannelFeed> {
        rss::fetch(&self.http, channel_id).await
    }

    async fn reset_session(&self) {
        self.player.reset_session().await;
    }
}
