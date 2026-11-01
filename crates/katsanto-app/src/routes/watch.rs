//! Watch progress: resume where you left off, and "continue watching".

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use katsanto_db::{
    hidden, playlists,
    progress::{self, Progress},
    videos::{self, ChannelMeta, VideoMeta},
};
use katsanto_yt::model::{ChannelRef, VideoCard};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{api::proxy_card_images, proxy::is_video_id};
use crate::{
    auth::CurrentUser,
    error::{AppError, AppResult, ErrorBody},
    state::AppState,
};

const MAX_IDS: usize = 100;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_progress))
        .routes(routes!(save_progress, delete_progress))
        .routes(routes!(continue_watching))
        .routes(routes!(hide_video, unhide_video))
        .routes(routes!(mark_watched))
}

#[derive(Clone, Serialize, ToSchema)]
pub struct ProgressInfo {
    pub video_id: String,
    pub position_s: f64,
    pub duration_s: f64,
    pub finished: bool,
    pub updated_at: i64,
}

impl From<Progress> for ProgressInfo {
    fn from(p: Progress) -> Self {
        Self {
            video_id: p.video_id,
            position_s: p.position_s,
            duration_s: p.duration_s,
            finished: p.finished,
            updated_at: p.updated_at,
        }
    }
}

#[derive(Deserialize, IntoParams)]
pub struct IdsQuery {
    /// Comma-separated video ids (at most 100).
    ids: String,
}

/// Progress for the given videos (for progress bars on cards). Unwatched videos are omitted.
#[utoipa::path(get, path = "/progress", tag = "watch", params(IdsQuery),
    responses((status = 200, body = Vec<ProgressInfo>)))]
async fn get_progress(
    State(st): State<AppState>,
    me: CurrentUser,
    Query(q): Query<IdsQuery>,
) -> AppResult<Json<Vec<ProgressInfo>>> {
    let ids: Vec<String> = q
        .ids
        .split(',')
        .filter(|id| is_video_id(id))
        .take(MAX_IDS)
        .map(str::to_owned)
        .collect();
    let list = progress::for_videos(st.db.pool(), me.id(), &ids).await?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}

#[derive(Deserialize, ToSchema)]
pub struct SaveProgress {
    pub position_s: f64,
    pub duration_s: f64,
}

/// Save the playback position. Sent periodically while playing and when leaving the page.
#[utoipa::path(put, path = "/progress/{video_id}", tag = "watch", request_body = SaveProgress,
    responses((status = 200, body = ProgressInfo), (status = "4XX", body = ErrorBody)))]
async fn save_progress(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(video_id): Path<String>,
    Json(b): Json<SaveProgress>,
) -> AppResult<Json<ProgressInfo>> {
    if !is_video_id(&video_id) {
        return Err(AppError::BadRequest("invalid video id".into()));
    }
    let valid = b.duration_s.is_finite()
        && b.position_s.is_finite()
        && b.duration_s > 0.0
        && b.position_s >= 0.0;
    if !valid {
        return Err(AppError::BadRequest("invalid position or duration".into()));
    }
    let position = b.position_s.min(b.duration_s);
    ensure_video_stored(&st, &video_id).await?;
    let p = progress::save(st.db.pool(), me.id(), &video_id, position, b.duration_s).await?;
    if p.finished {
        playlists::remove_watched(st.db.pool(), me.id(), &video_id).await?;
    }
    Ok(Json(p.into()))
}

/// Store a video's metadata the first time someone saves progress for it, so
/// lists like "continue watching" never need YouTube.
pub(crate) async fn ensure_video_stored(st: &AppState, video_id: &str) -> AppResult<()> {
    if videos::exists(st.db.pool(), video_id).await? {
        return Ok(());
    }
    let pb = st.playback(video_id).await?;
    let d = &pb.details;
    // The watch page loads this anyway, so it's usually cached; it adds the avatar.
    let info = st.video_info(video_id).await.ok();
    let avatar = info.as_ref().and_then(|i| i.channel.avatar_url.as_deref());
    let subs = info.as_ref().and_then(|i| i.channel.subscriber_count);
    videos::upsert_channel(
        st.db.pool(),
        &ChannelMeta {
            id: &d.channel.id,
            name: &d.channel.name,
            handle: None,
            avatar_url: avatar,
            subscriber_count: subs.map(|s| s as i64),
        },
    )
    .await?;
    videos::upsert_video(
        st.db.pool(),
        &VideoMeta {
            id: &d.id,
            channel_id: &d.channel.id,
            title: &d.title,
            thumbnail_url: d.thumbnail_url.as_deref(),
            duration_s: Some(i64::from(d.duration_s)),
            published_at: info.as_ref().and_then(|i| i.published_at),
            is_short: false,
            is_live: d.is_live,
        },
    )
    .await?;
    Ok(())
}

/// Forget the progress (e.g. remove from "continue watching").
#[utoipa::path(delete, path = "/progress/{video_id}", tag = "watch",
    responses((status = 204), (status = 404, body = ErrorBody)))]
async fn delete_progress(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(video_id): Path<String>,
) -> AppResult<StatusCode> {
    if !progress::delete(st.db.pool(), me.id(), &video_id).await? {
        return Err(AppError::NotFound("progress".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct ContinueItem {
    pub video: VideoCard,
    pub position_s: f64,
    pub duration_s: f64,
    pub updated_at: i64,
}

#[derive(Deserialize, IntoParams)]
pub struct LimitQuery {
    /// Default 12, at most 50.
    limit: Option<i64>,
}

/// Started but unfinished videos, most recent first.
#[utoipa::path(get, path = "/continue-watching", tag = "watch", params(LimitQuery),
    responses((status = 200, body = Vec<ContinueItem>)))]
async fn continue_watching(
    State(st): State<AppState>,
    me: CurrentUser,
    Query(q): Query<LimitQuery>,
) -> AppResult<Json<Vec<ContinueItem>>> {
    let limit = q.limit.unwrap_or(12).clamp(1, 50);
    let rows = progress::continue_watching(st.db.pool(), me.id(), limit).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| {
                let v = r.video;
                let mut video = VideoCard {
                    id: v.id,
                    title: v.title,
                    channel: Some(ChannelRef {
                        id: v.channel_id,
                        name: v.channel_name,
                        avatar_url: v.channel_avatar_url,
                    }),
                    thumbnail_url: v.thumbnail_url,
                    duration_s: v.duration_s.map(|d| d as u32),
                    published_at: v.published_at,
                    published_text: None,
                    view_count: None,
                    is_live: v.is_live,
                    is_short: v.is_short,
                    is_upcoming: false,
                };
                proxy_card_images(&mut video);
                ContinueItem {
                    video,
                    position_s: r.position_s,
                    duration_s: r.duration_s_watched,
                    updated_at: r.updated_at,
                }
            })
            .collect(),
    ))
}

/// Hide a video from feeds ("not interested").
#[utoipa::path(post, path = "/videos/{video_id}/hide", tag = "watch",
    responses((status = 204), (status = 400, body = ErrorBody)))]
async fn hide_video(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(video_id): Path<String>,
) -> AppResult<StatusCode> {
    if !is_video_id(&video_id) {
        return Err(AppError::BadRequest("invalid video id".into()));
    }
    hidden::hide(st.db.pool(), me.id(), &video_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/videos/{video_id}/hide", tag = "watch",
    responses((status = 204), (status = 404, body = ErrorBody)))]
async fn unhide_video(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(video_id): Path<String>,
) -> AppResult<StatusCode> {
    if !hidden::unhide(st.db.pool(), me.id(), &video_id).await? {
        return Err(AppError::NotFound("hidden video".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Mark a video as watched without playing it.
#[utoipa::path(post, path = "/videos/{video_id}/watched", tag = "watch",
    responses((status = 200, body = ProgressInfo), (status = "4XX", body = ErrorBody)))]
async fn mark_watched(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(video_id): Path<String>,
) -> AppResult<Json<ProgressInfo>> {
    if !is_video_id(&video_id) {
        return Err(AppError::BadRequest("invalid video id".into()));
    }
    let duration = match videos::duration(st.db.pool(), &video_id).await? {
        Some(d) if d > 0 => d,
        _ => {
            // Not stored yet (or no duration): fetch it like the player would.
            ensure_video_stored(&st, &video_id).await?;
            videos::duration(st.db.pool(), &video_id)
                .await?
                .filter(|d| *d > 0)
                .ok_or_else(|| AppError::BadRequest("video duration unknown".into()))?
        }
    } as f64;
    let p = progress::save(st.db.pool(), me.id(), &video_id, duration, duration).await?;
    playlists::remove_watched(st.db.pool(), me.id(), &video_id).await?;
    Ok(Json(p.into()))
}
