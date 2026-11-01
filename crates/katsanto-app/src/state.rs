use std::{sync::Arc, time::Duration};

use katsanto_db::Db;
use katsanto_yt::{
    YoutubeSource, YtResult,
    model::{ChannelPage, ChannelTab, Playback, SearchPage, SearchParams, VideoInfo, VideoPage},
};
use moka::future::Cache;
use webauthn_rs::Webauthn;

use crate::{config::Config, error::AppResult};

/// Refetch player data this long before googlevideo URLs expire, so a stream
/// started near the end of the window does not break mid-playback.
const EXPIRY_MARGIN_S: i64 = 30 * 60;

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

pub struct Inner {
    pub config: Config,
    pub db: Db,
    /// `None` when the public URL cannot host passkeys.
    pub webauthn: Option<Arc<Webauthn>>,
    pub yt: Arc<dyn YoutubeSource>,
    /// Wakes the refresher, e.g. after a new subscription.
    pub refresh_wakeup: tokio::sync::Notify,
    /// New uploads found by the refresher, for live feed updates.
    pub feed_events: tokio::sync::broadcast::Sender<crate::refresh::FeedEvent>,
    /// Upstream client for googlevideo and image hosts.
    pub http: reqwest::Client,
    /// `None` when disabled in the config.
    pub sponsorblock: Option<crate::sponsorblock::SponsorBlock>,
    playback: Cache<String, Arc<Playback>>,
    search: Cache<SearchKey, Arc<SearchPage>>,
    video_info: Cache<String, Arc<VideoInfo>>,
    channels: Cache<(String, ChannelTab), Arc<ChannelPage>>,
    channel_pages: Cache<String, Arc<VideoPage>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SearchKey {
    First(String, SearchParams),
    More(String),
}

impl std::ops::Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl AppState {
    pub fn new(config: Config, db: Db, yt: Arc<dyn YoutubeSource>) -> AppResult<Self> {
        let webauthn = crate::auth::passkey::build(&config)?.map(Arc::new);
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .http2_max_header_list_size(katsanto_yt::MAX_RESPONSE_HEADERS)
            .build()
            .expect("http client");
        let sponsorblock = (!config.sponsorblock_api.trim().is_empty()).then(|| {
            crate::sponsorblock::SponsorBlock::new(http.clone(), &config.sponsorblock_api)
        });
        Ok(Self(Arc::new(Inner {
            sponsorblock,
            config,
            db,
            webauthn,
            yt,
            refresh_wakeup: tokio::sync::Notify::new(),
            feed_events: tokio::sync::broadcast::channel(256).0,
            http,
            playback: Cache::builder()
                .max_capacity(2_000)
                .time_to_live(Duration::from_secs(6 * 3600))
                .build(),
            search: Cache::builder()
                .max_capacity(5_000)
                .time_to_live(Duration::from_secs(10 * 60))
                .build(),
            video_info: Cache::builder()
                .max_capacity(2_000)
                .time_to_live(Duration::from_secs(3600))
                .build(),
            channels: Cache::builder()
                .max_capacity(1_000)
                .time_to_live(Duration::from_secs(10 * 60))
                .build(),
            channel_pages: Cache::builder()
                .max_capacity(2_000)
                .time_to_live(Duration::from_secs(10 * 60))
                .build(),
        })))
    }

    /// Absolute URL for a path on this instance, e.g. for invite links.
    pub fn public_link(&self, path_and_query: &str) -> String {
        let base = self.config.public_url.as_str().trim_end_matches('/');
        format!("{base}{path_and_query}")
    }

    /// Player data for a video, cached until shortly before its stream URLs expire.
    /// Concurrent requests for the same video share one upstream fetch.
    pub async fn playback(&self, video_id: &str) -> AppResult<Arc<Playback>> {
        if let Some(p) = self.playback.get(video_id).await {
            if !p.is_expired(now(), EXPIRY_MARGIN_S) {
                return Ok(p);
            }
            self.playback.invalidate(video_id).await;
        }
        let yt = self.yt.clone();
        let id = video_id.to_owned();
        self.playback
            .try_get_with(
                id.clone(),
                async move { yt.playback(&id).await.map(Arc::new) },
            )
            .await
            .map_err(|e| unshare(&e))
    }

    /// Drop cached player data, e.g. after googlevideo rejected a URL.
    pub async fn invalidate_playback(&self, video_id: &str) {
        self.playback.invalidate(video_id).await;
    }

    pub async fn search(&self, key: SearchKey) -> AppResult<Arc<SearchPage>> {
        let yt = self.yt.clone();
        cached(&self.search, key.clone(), async move {
            match &key {
                SearchKey::First(q, params) => yt.search(q, params).await,
                SearchKey::More(cursor) => yt.search_more(cursor).await,
            }
        })
        .await
    }

    pub async fn video_info(&self, video_id: &str) -> AppResult<Arc<VideoInfo>> {
        let (yt, id) = (self.yt.clone(), video_id.to_owned());
        cached(&self.video_info, id.clone(), async move {
            yt.video_info(&id).await
        })
        .await
    }

    pub async fn channel(&self, channel_id: &str, tab: ChannelTab) -> AppResult<Arc<ChannelPage>> {
        let (yt, id) = (self.yt.clone(), channel_id.to_owned());
        cached(&self.channels, (id.clone(), tab), async move {
            yt.channel(&id, tab).await
        })
        .await
    }

    pub async fn channel_more(&self, cursor: &str) -> AppResult<Arc<VideoPage>> {
        let (yt, c) = (self.yt.clone(), cursor.to_owned());
        cached(&self.channel_pages, c.clone(), async move {
            yt.channel_more(&c).await
        })
        .await
    }
}

/// Get from `cache` or run `fetch` once, even with concurrent callers.
async fn cached<K, V>(
    cache: &Cache<K, Arc<V>>,
    key: K,
    fetch: impl Future<Output = YtResult<V>>,
) -> AppResult<Arc<V>>
where
    K: std::hash::Hash + Eq + Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    cache
        .try_get_with(key, async move { fetch.await.map(Arc::new) })
        .await
        .map_err(|e| unshare(&e))
}

/// moka hands out errors as `Arc<E>`; rebuild an owned error from it.
fn unshare(e: &katsanto_yt::YtError) -> crate::error::AppError {
    use katsanto_yt::YtError::*;
    let e = match e {
        NotFound(m) => NotFound(m.clone()),
        Unavailable(m) => Unavailable(m.clone()),
        BadRequest(m) => BadRequest(m.clone()),
        Blocked(m) => Blocked(m.clone()),
        Other(m) => Other(m.clone()),
    };
    e.into()
}

pub fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
