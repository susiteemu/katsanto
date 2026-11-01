use axum::{
    Json,
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
};
use katsanto_yt::{
    dash,
    model::{
        ChannelPage, ChannelTab, Length, Playback, ResolvedUrl, SearchItem, SearchKind, SearchPage,
        SearchParams, SortOrder, Stream, Subtitle, UploadDate, VideoCard, VideoDetails, VideoInfo,
        VideoPage,
    },
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::proxy::{image_url, is_video_id, stream_url};
use crate::{
    error::{AppError, AppResult, ErrorBody},
    state::{AppState, SearchKey},
};

/// YouTube search and video data. Requires login.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(search))
        .routes(routes!(suggestions))
        .routes(routes!(resolve))
        .routes(routes!(video))
        .routes(routes!(video_info))
        .routes(routes!(captions))
        .routes(routes!(manifest))
        .routes(routes!(channel))
        .routes(routes!(channel_videos))
}

const MAX_QUERY_LEN: usize = 200;

#[derive(Deserialize, IntoParams)]
pub struct SearchQuery {
    /// Search text; required unless `cursor` is given.
    q: Option<String>,
    /// Cursor from a previous page; the other parameters are then ignored.
    cursor: Option<String>,
    #[serde(default)]
    #[param(inline)]
    kind: SearchKind,
    #[param(inline)]
    date: Option<UploadDate>,
    #[param(inline)]
    length: Option<Length>,
    #[param(inline)]
    sort: Option<SortOrder>,
}

#[utoipa::path(get, path = "/search", tag = "youtube", params(SearchQuery),
    responses((status = 200, body = SearchPage), (status = "4XX", body = ErrorBody)))]
async fn search(
    State(st): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<SearchPage>> {
    let key = match (q.cursor, q.q) {
        (Some(cursor), _) => SearchKey::More(cursor),
        (None, Some(text)) => {
            let text = clean_query(&text)?;
            let params = SearchParams {
                kind: q.kind,
                date: q.date,
                length: q.length,
                sort: q.sort,
            };
            SearchKey::First(text, params)
        }
        (None, None) => return Err(AppError::BadRequest("missing `q` or `cursor`".into())),
    };
    let mut page = (*st.search(key).await?).clone();
    proxy_search_images(&mut page);
    Ok(Json(page))
}

#[derive(Deserialize, IntoParams)]
pub struct SuggestQuery {
    q: String,
}

#[utoipa::path(get, path = "/search/suggestions", tag = "youtube", params(SuggestQuery),
    responses((status = 200, body = Vec<String>)))]
async fn suggestions(
    State(st): State<AppState>,
    Query(q): Query<SuggestQuery>,
) -> AppResult<Json<Vec<String>>> {
    let text = clean_query(&q.q)?;
    Ok(Json(st.yt.search_suggestions(&text).await?))
}

#[derive(Deserialize, IntoParams)]
pub struct ResolveQuery {
    url: String,
}

/// What a pasted YouTube link points to.
#[utoipa::path(get, path = "/resolve", tag = "youtube", params(ResolveQuery),
    responses((status = 200, body = ResolvedUrl), (status = "4XX", body = ErrorBody)))]
async fn resolve(
    State(st): State<AppState>,
    Query(q): Query<ResolveQuery>,
) -> AppResult<Json<ResolvedUrl>> {
    Ok(Json(st.yt.resolve_url(q.url.trim()).await?))
}

#[derive(Serialize, ToSchema)]
pub struct VideoResponse {
    details: VideoDetails,
    manifest_url: String,
    streams: Vec<Stream>,
    captions: Vec<CaptionTrack>,
}

/// A subtitle track, served as WebVTT by this server.
#[derive(Serialize, ToSchema)]
pub struct CaptionTrack {
    /// Track id for the captions endpoint, e.g. `en` or `en.asr`.
    pub id: String,
    pub lang: String,
    pub label: String,
    pub auto_generated: bool,
    pub url: String,
}

fn caption_id(s: &Subtitle) -> String {
    if s.auto_generated {
        format!("{}.asr", s.lang)
    } else {
        s.lang.clone()
    }
}

#[utoipa::path(get, path = "/videos/{id}", tag = "youtube",
    responses((status = 200, body = VideoResponse), (status = "4XX", body = ErrorBody)))]
async fn video(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Json<VideoResponse>> {
    check_video_id(&id)?;
    let pb = st.playback(&id).await?;
    let mut details = pb.details.clone();
    details.thumbnail_url = details.thumbnail_url.as_deref().map(image_url);
    let captions = pb
        .subtitles
        .iter()
        .map(|s| {
            let cid = caption_id(s);
            CaptionTrack {
                url: format!("/api/v1/videos/{id}/captions/{cid}"),
                id: cid,
                lang: s.lang.clone(),
                label: s.name.clone(),
                auto_generated: s.auto_generated,
            }
        })
        .collect();
    Ok(Json(VideoResponse {
        manifest_url: format!("/api/v1/videos/{id}/manifest.mpd"),
        details,
        streams: pb.streams.clone(),
        captions,
    }))
}

/// Description, channel, chapters and related videos.
#[utoipa::path(get, path = "/videos/{id}/info", tag = "youtube",
    responses((status = 200, body = VideoInfo), (status = "4XX", body = ErrorBody)))]
async fn video_info(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Json<VideoInfo>> {
    check_video_id(&id)?;
    let mut info = (*st.video_info(&id).await?).clone();
    proxy_image(&mut info.channel.avatar_url);
    info.related.iter_mut().for_each(proxy_card_images);
    Ok(Json(info))
}

/// Subtitles as WebVTT (fetched from YouTube through this server).
#[utoipa::path(get, path = "/videos/{id}/captions/{track}", tag = "youtube",
    responses((status = 200, content_type = "text/vtt", body = String), (status = 404, body = ErrorBody)))]
async fn captions(
    State(st): State<AppState>,
    Path((id, track)): Path<(String, String)>,
) -> AppResult<Response> {
    check_video_id(&id)?;
    let pb = st.playback(&id).await?;
    let sub = pb
        .subtitles
        .iter()
        .find(|s| caption_id(s) == track)
        .ok_or_else(|| AppError::NotFound(format!("caption track {track}")))?;
    let mut url = url::Url::parse(&sub.url).map_err(AppError::internal)?;
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "fmt")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(pairs)
        .append_pair("fmt", "vtt");
    let res = st
        .http
        .get(url)
        .header(header::USER_AGENT, &pb.user_agent)
        .send()
        .await
        .map_err(|e| AppError::Upstream(e.to_string()))?;
    if !res.status().is_success() {
        return Err(AppError::Upstream(format!(
            "captions returned {}",
            res.status()
        )));
    }
    let body = res
        .text()
        .await
        .map_err(|e| AppError::Upstream(e.to_string()))?;
    if !body.starts_with("WEBVTT") {
        return Err(AppError::Upstream("captions are not WebVTT".into()));
    }
    Ok((
        [
            (header::CONTENT_TYPE, "text/vtt; charset=utf-8"),
            (header::CACHE_CONTROL, "private, max-age=3600"),
        ],
        body,
    )
        .into_response())
}

#[derive(Deserialize, IntoParams)]
pub struct ChannelQuery {
    #[serde(default)]
    #[param(inline)]
    tab: ChannelTab,
}

/// Channel info and the first page of a tab.
#[utoipa::path(get, path = "/channels/{id}", tag = "youtube", params(ChannelQuery),
    responses((status = 200, body = ChannelPage), (status = "4XX", body = ErrorBody)))]
async fn channel(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ChannelQuery>,
) -> AppResult<Json<ChannelPage>> {
    if !is_channel_id(&id) {
        return Err(AppError::BadRequest("invalid channel id".into()));
    }
    let mut page = (*st.channel(&id, q.tab).await?).clone();
    proxy_image(&mut page.channel.avatar_url);
    proxy_image(&mut page.channel.banner_url);
    page.videos.items.iter_mut().for_each(proxy_card_images);
    Ok(Json(page))
}

#[derive(Deserialize, IntoParams)]
pub struct CursorQuery {
    cursor: String,
}

/// More videos of a channel tab.
#[utoipa::path(get, path = "/channels/{id}/videos", tag = "youtube", params(CursorQuery),
    responses((status = 200, body = VideoPage), (status = "4XX", body = ErrorBody)))]
async fn channel_videos(
    State(st): State<AppState>,
    Path(_id): Path<String>,
    Query(q): Query<CursorQuery>,
) -> AppResult<Json<VideoPage>> {
    let mut page = (*st.channel_more(&q.cursor).await?).clone();
    page.items.iter_mut().for_each(proxy_card_images);
    Ok(Json(page))
}

pub fn is_channel_id(id: &str) -> bool {
    id.len() == 24
        && id.starts_with("UC")
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[derive(Deserialize, IntoParams)]
pub struct ManifestQuery {
    /// `opus` (in sync, for browsers that play Opus/WebM) or `aac` (default, plays everywhere).
    audio: Option<String>,
}

/// DASH manifest for the player; stream URLs point at the proxy.
#[utoipa::path(get, path = "/videos/{id}/manifest.mpd", tag = "youtube", params(ManifestQuery),
    responses((status = 200, content_type = "application/dash+xml", body = String)))]
async fn manifest(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ManifestQuery>,
) -> AppResult<Response> {
    check_video_id(&id)?;
    let pb = st.playback(&id).await?;
    let audio = match q.audio.as_deref() {
        Some("opus") => dash::AudioFormat::Opus,
        _ => dash::AudioFormat::Aac,
    };
    let mpd = render_manifest(&pb, audio, st.config.proxy_video);
    Ok((
        [
            (header::CONTENT_TYPE, "application/dash+xml"),
            (header::CACHE_CONTROL, "private, no-cache"),
        ],
        mpd,
    )
        .into_response())
}

fn render_manifest(pb: &Playback, audio: dash::AudioFormat, proxy: bool) -> String {
    let id = pb.details.id.clone();
    dash::manifest(pb, audio, |s| {
        if proxy {
            stream_url(&id, &s.id)
        } else {
            s.url.clone()
        }
    })
}

fn check_video_id(id: &str) -> AppResult<()> {
    if is_video_id(id) {
        Ok(())
    } else {
        Err(AppError::BadRequest("invalid video id".into()))
    }
}

fn clean_query(q: &str) -> AppResult<String> {
    let q = q.trim();
    if q.is_empty() {
        return Err(AppError::BadRequest("empty query".into()));
    }
    Ok(q.chars().take(MAX_QUERY_LEN).collect())
}

fn proxy_search_images(page: &mut SearchPage) {
    for item in &mut page.items {
        match item {
            SearchItem::Video(v) => proxy_card_images(v),
            SearchItem::Channel(c) => proxy_image(&mut c.avatar_url),
        }
    }
}

/// Point a YouTube image URL at the image proxy.
pub fn proxy_image(url: &mut Option<String>) {
    *url = url.as_deref().map(image_url);
}

pub fn proxy_card_images(v: &mut VideoCard) {
    proxy_image(&mut v.thumbnail_url);
    if let Some(c) = &mut v.channel {
        proxy_image(&mut c.avatar_url);
    }
}
