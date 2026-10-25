//! Watch lists.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use katsanto_db::{Db, playlists};
use katsanto_yt::model::{ChannelRef, VideoCard};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{
    api::proxy_card_images,
    proxy::{image_url, is_video_id},
    watch::{ProgressInfo, ensure_video_stored},
};
use crate::{
    auth::CurrentUser,
    error::{AppError, AppResult, ErrorBody},
    state::AppState,
};

const MAX_LISTS: usize = 200;
const MAX_ITEMS: i64 = 5_000;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_lists, create_list))
        .routes(routes!(get_list, update_list, delete_list))
        .routes(routes!(add_item))
        .routes(routes!(remove_item))
        .routes(routes!(move_item))
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ListKind {
    WatchLater,
    Custom,
}

#[derive(Serialize, ToSchema)]
pub struct ListInfo {
    pub id: i64,
    pub kind: ListKind,
    pub name: String,
    pub description: String,
    /// Videos are removed once watched to the end.
    pub remove_watched: bool,
    pub item_count: i64,
    pub total_duration_s: i64,
    pub thumbnail_url: Option<String>,
    pub updated_at: i64,
    /// With `?contains=`: whether the video is in this list.
    pub contains: Option<bool>,
}

impl ListInfo {
    fn new(p: playlists::Playlist, contains: Option<bool>) -> Self {
        Self {
            kind: if p.kind == playlists::WATCH_LATER {
                ListKind::WatchLater
            } else {
                ListKind::Custom
            },
            id: p.id,
            name: p.name,
            description: p.description,
            remove_watched: p.remove_watched,
            item_count: p.item_count,
            total_duration_s: p.total_duration_s,
            thumbnail_url: p.thumbnail_url.as_deref().map(image_url),
            updated_at: p.updated_at,
            contains,
        }
    }
}

async fn owned(st: &AppState, user_id: i64, id: i64) -> AppResult<playlists::Playlist> {
    playlists::get(st.db.pool(), user_id, id)
        .await?
        .ok_or_else(|| AppError::NotFound("list".into()))
}

#[derive(Deserialize, IntoParams)]
pub struct ListQuery {
    /// A video id: each list then says whether it contains the video.
    contains: Option<String>,
}

/// The user's lists, Watch later first (created on first use).
#[utoipa::path(get, path = "/lists", tag = "lists", params(ListQuery),
    responses((status = 200, body = Vec<ListInfo>)))]
async fn list_lists(
    State(st): State<AppState>,
    me: CurrentUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Vec<ListInfo>>> {
    let mut conn = st.db.acquire().await?;
    playlists::ensure_watch_later(&mut conn, me.id()).await?;
    drop(conn);
    let containing = match &q.contains {
        Some(v) => Some(playlists::containing(st.db.pool(), me.id(), v).await?),
        None => None,
    };
    let lists = playlists::list(st.db.pool(), me.id()).await?;
    Ok(Json(
        lists
            .into_iter()
            .map(|p| {
                let contains = containing.as_ref().map(|ids| ids.contains(&p.id));
                ListInfo::new(p, contains)
            })
            .collect(),
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct ListBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub remove_watched: bool,
}

fn validate(b: &ListBody) -> AppResult<(String, String)> {
    let name = b.name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(AppError::BadRequest(
            "List names are 1–60 characters".into(),
        ));
    }
    let description = b.description.trim();
    if description.chars().count() > 500 {
        return Err(AppError::BadRequest("The description is too long".into()));
    }
    Ok((name.to_owned(), description.to_owned()))
}

#[utoipa::path(post, path = "/lists", tag = "lists", request_body = ListBody,
    responses((status = 201, body = ListInfo), (status = "4XX", body = ErrorBody)))]
async fn create_list(
    State(st): State<AppState>,
    me: CurrentUser,
    Json(b): Json<ListBody>,
) -> AppResult<(StatusCode, Json<ListInfo>)> {
    let (name, description) = validate(&b)?;
    if playlists::list(st.db.pool(), me.id()).await?.len() >= MAX_LISTS {
        return Err(AppError::Conflict(format!(
            "You can have at most {MAX_LISTS} lists"
        )));
    }
    let id = playlists::create(st.db.pool(), me.id(), &name, &description).await?;
    if b.remove_watched {
        playlists::update(st.db.pool(), me.id(), id, &name, &description, true).await?;
    }
    Ok((
        StatusCode::CREATED,
        Json(ListInfo::new(owned(&st, me.id(), id).await?, None)),
    ))
}

#[derive(Serialize, ToSchema)]
pub struct ListItem {
    pub video: VideoCard,
    pub progress: Option<ProgressInfo>,
    pub added_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ListDetail {
    pub list: ListInfo,
    pub items: Vec<ListItem>,
}

#[utoipa::path(get, path = "/lists/{id}", tag = "lists",
    responses((status = 200, body = ListDetail), (status = 404, body = ErrorBody)))]
async fn get_list(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Json<ListDetail>> {
    let list = owned(&st, me.id(), id).await?;
    let items = playlists::items(st.db.pool(), me.id(), id).await?;
    Ok(Json(ListDetail {
        list: ListInfo::new(list, None),
        items: items.into_iter().map(list_item).collect(),
    }))
}

fn list_item(i: playlists::Item) -> ListItem {
    let progress = match (i.position_s, i.watched_duration_s) {
        (Some(position_s), Some(duration_s)) => Some(ProgressInfo {
            video_id: i.video_id.clone(),
            position_s,
            duration_s,
            finished: i.finished.unwrap_or(false),
            updated_at: i.progress_updated_at.unwrap_or(0),
        }),
        _ => None,
    };
    let mut video = VideoCard {
        id: i.video_id,
        title: i.title,
        channel: Some(ChannelRef {
            id: i.channel_id,
            name: i.channel_name,
            avatar_url: i.channel_avatar_url,
        }),
        thumbnail_url: i.thumbnail_url,
        duration_s: i.duration_s.map(|d| d as u32),
        published_at: i.published_at,
        published_text: None,
        view_count: None,
        is_live: i.is_live,
        is_short: i.is_short,
        is_upcoming: false,
    };
    proxy_card_images(&mut video);
    ListItem {
        video,
        progress,
        added_at: i.added_at,
    }
}

#[utoipa::path(put, path = "/lists/{id}", tag = "lists", request_body = ListBody,
    responses((status = 200, body = ListInfo), (status = "4XX", body = ErrorBody)))]
async fn update_list(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(id): Path<i64>,
    Json(b): Json<ListBody>,
) -> AppResult<Json<ListInfo>> {
    let (name, description) = validate(&b)?;
    if !playlists::update(
        st.db.pool(),
        me.id(),
        id,
        &name,
        &description,
        b.remove_watched,
    )
    .await?
    {
        return Err(AppError::NotFound("list".into()));
    }
    Ok(Json(ListInfo::new(owned(&st, me.id(), id).await?, None)))
}

/// Delete a list. Watch later can't be deleted, only emptied.
#[utoipa::path(delete, path = "/lists/{id}", tag = "lists",
    responses((status = 204), (status = "4XX", body = ErrorBody)))]
async fn delete_list(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let list = owned(&st, me.id(), id).await?;
    if list.kind == playlists::WATCH_LATER {
        return Err(AppError::Conflict("Watch later can't be deleted".into()));
    }
    playlists::delete(st.db.pool(), me.id(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct AddItem {
    pub video_id: String,
}

#[derive(Serialize, ToSchema)]
pub struct Added {
    /// False if the video was already in the list.
    pub added: bool,
}

/// Add a video to the end of a list.
#[utoipa::path(post, path = "/lists/{id}/items", tag = "lists", request_body = AddItem,
    responses((status = 200, body = Added), (status = "4XX", body = ErrorBody)))]
async fn add_item(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(id): Path<i64>,
    Json(b): Json<AddItem>,
) -> AppResult<Json<Added>> {
    if !is_video_id(&b.video_id) {
        return Err(AppError::BadRequest("invalid video id".into()));
    }
    let list = owned(&st, me.id(), id).await?;
    if list.item_count >= MAX_ITEMS {
        return Err(AppError::Conflict(format!(
            "Lists hold at most {MAX_ITEMS} videos"
        )));
    }
    // Lists show stored metadata, so the video must be known first.
    ensure_video_stored(&st, &b.video_id).await?;
    let mut conn = st.db.acquire().await?;
    let added = playlists::add(&mut conn, id, &b.video_id).await?;
    Ok(Json(Added { added }))
}

#[utoipa::path(delete, path = "/lists/{id}/items/{video_id}", tag = "lists",
    responses((status = 204), (status = 404, body = ErrorBody)))]
async fn remove_item(
    State(st): State<AppState>,
    me: CurrentUser,
    Path((id, video_id)): Path<(i64, String)>,
) -> AppResult<StatusCode> {
    owned(&st, me.id(), id).await?;
    if !playlists::remove(st.db.pool(), id, &video_id).await? {
        return Err(AppError::NotFound("video in list".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct MoveItem {
    /// Place the video right after this one; `null` moves it to the top.
    pub after: Option<String>,
}

#[utoipa::path(put, path = "/lists/{id}/items/{video_id}/position", tag = "lists", request_body = MoveItem,
    responses((status = 204), (status = 404, body = ErrorBody)))]
async fn move_item(
    State(st): State<AppState>,
    me: CurrentUser,
    Path((id, video_id)): Path<(i64, String)>,
    Json(b): Json<MoveItem>,
) -> AppResult<StatusCode> {
    owned(&st, me.id(), id).await?;
    let mut tx = st.db.begin().await?;
    if !playlists::move_item(&mut tx, id, &video_id, b.after.as_deref()).await? {
        return Err(AppError::NotFound("video in list".into()));
    }
    Db::commit(tx).await?;
    Ok(StatusCode::NO_CONTENT)
}
