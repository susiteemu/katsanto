use std::{future::Future, panic::AssertUnwindSafe, path::PathBuf};

use async_trait::async_trait;
use futures_util::FutureExt;

use crate::{
    model::{
        ChannelFeed, ChannelPage, ChannelTab, Playback, ResolvedUrl, SearchPage, SearchParams,
        VideoInfo, VideoPage,
    },
    player::PlayerClient,
    rss,
    rustypipe_source::{RustyPipeConfig, RustyPipeSource},
    solver::{Solver, SolverScripts},
    source::{YoutubeSource, YtError, YtResult},
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
            .http2_max_header_list_size(crate::MAX_RESPONSE_HEADERS)
            .build()
            .map_err(|e| YtError::Other(e.to_string()))?;
        Ok(Self { meta, player, http })
    }
}

/// Turn a panic into an error. rustypipe unwraps some failures, such as a failed visitor
/// data request; a panic would drop the connection instead of giving the client a response.
async fn guarded<T>(call: impl Future<Output = YtResult<T>>) -> YtResult<T> {
    AssertUnwindSafe(call)
        .catch_unwind()
        .await
        .unwrap_or_else(|panic| {
            let msg = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("unknown panic");
            Err(YtError::Other(format!("rustypipe panicked: {msg}")))
        })
}

#[async_trait]
impl YoutubeSource for YoutubeClient {
    async fn search(&self, query: &str, params: &SearchParams) -> YtResult<SearchPage> {
        guarded(self.meta.search(query, params)).await
    }

    async fn search_more(&self, cursor: &str) -> YtResult<SearchPage> {
        guarded(self.meta.search_more(cursor)).await
    }

    async fn search_suggestions(&self, query: &str) -> YtResult<Vec<String>> {
        guarded(self.meta.search_suggestions(query)).await
    }

    async fn resolve_url(&self, url: &str) -> YtResult<ResolvedUrl> {
        guarded(self.meta.resolve_url(url)).await
    }

    async fn playback(&self, video_id: &str) -> YtResult<Playback> {
        self.player.playback(video_id).await
    }

    async fn video_info(&self, video_id: &str) -> YtResult<VideoInfo> {
        guarded(self.meta.video_info(video_id)).await
    }

    async fn channel(&self, channel_id: &str, tab: ChannelTab) -> YtResult<ChannelPage> {
        guarded(self.meta.channel(channel_id, tab)).await
    }

    async fn channel_more(&self, cursor: &str) -> YtResult<VideoPage> {
        guarded(self.meta.channel_more(cursor)).await
    }

    async fn channel_feed(&self, channel_id: &str) -> YtResult<ChannelFeed> {
        rss::fetch(&self.http, channel_id).await
    }

    async fn reset_session(&self) {
        self.player.reset_session().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn guarded_turns_a_panic_into_an_error() {
        let res: YtResult<()> = guarded(async { panic!("no visitor data: {}", 1) }).await;
        assert!(
            matches!(res, Err(YtError::Other(msg)) if msg == "rustypipe panicked: no visitor data: 1")
        );
    }

    #[tokio::test]
    async fn guarded_passes_results_through() {
        assert_eq!(guarded(async { Ok(1) }).await.unwrap(), 1);
        let res: YtResult<()> = guarded(async { Err(YtError::NotFound("x".into())) }).await;
        assert!(matches!(res, Err(YtError::NotFound(_))));
    }
}
