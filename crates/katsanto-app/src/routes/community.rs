//! Community data from SponsorBlock and DeArrow (see [`crate::sponsorblock`]).

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Response,
};
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::proxy::{image_response, is_video_id};
use crate::{
    auth::CurrentUser,
    error::{AppError, AppResult, ErrorBody},
    sponsorblock::Segment,
    state::AppState,
};

/// Cards on one page of a feed or search.
const MAX_BRANDING_IDS: usize = 60;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(features))
        .routes(routes!(segments))
        .routes(routes!(branding))
}

/// Optional integrations this instance has switched on.
#[derive(Serialize, ToSchema)]
pub struct Features {
    pub sponsorblock: bool,
    pub dearrow_titles: bool,
    pub dearrow_thumbnails: bool,
}

#[utoipa::path(get, path = "/features", tag = "community", responses((status = 200, body = Features)))]
async fn features(State(st): State<AppState>, _me: CurrentUser) -> Json<Features> {
    let on = st.sponsorblock.is_some();
    Json(Features {
        sponsorblock: on,
        dearrow_titles: on,
        dearrow_thumbnails: on && !st.config.dearrow_thumbnails.trim().is_empty(),
    })
}

#[utoipa::path(get, path = "/videos/{video_id}/segments", tag = "community",
    params(("video_id" = String, Path)),
    responses((status = 200, body = Vec<Segment>), (status = "4XX", body = ErrorBody)))]
async fn segments(
    State(st): State<AppState>,
    _me: CurrentUser,
    Path(video_id): Path<String>,
) -> AppResult<Json<Vec<Segment>>> {
    if !is_video_id(&video_id) {
        return Err(AppError::BadRequest("invalid video id".into()));
    }
    let Some(sb) = &st.sponsorblock else {
        return Ok(Json(Vec::new()));
    };
    Ok(Json(sb.segments(&video_id).await?.as_ref().clone()))
}

#[derive(Deserialize, ToSchema)]
pub struct BrandingRequest {
    pub ids: Vec<String>,
}

/// A DeArrow title and/or thumbnail; videos without one are left out of the response.
#[derive(Serialize, ToSchema)]
pub struct VideoBranding {
    pub title: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[utoipa::path(post, path = "/branding", tag = "community",
    request_body = BrandingRequest,
    responses((status = 200, body = HashMap<String, VideoBranding>), (status = "4XX", body = ErrorBody)))]
async fn branding(
    State(st): State<AppState>,
    _me: CurrentUser,
    Json(req): Json<BrandingRequest>,
) -> AppResult<Json<HashMap<String, VideoBranding>>> {
    if req.ids.len() > MAX_BRANDING_IDS || !req.ids.iter().all(|id| is_video_id(id)) {
        return Err(AppError::BadRequest("invalid video ids".into()));
    }
    let Some(sb) = &st.sponsorblock else {
        return Ok(Json(HashMap::new()));
    };
    let thumbnails = !st.config.dearrow_thumbnails.trim().is_empty();
    let found: Vec<_> = stream::iter(req.ids)
        .map(|id| async move {
            match sb.branding(&id).await {
                Ok(b) => Some((id, b)),
                Err(e) => {
                    tracing::debug!(%id, error = %e, "DeArrow lookup failed");
                    None
                }
            }
        })
        .buffer_unordered(8)
        .collect()
        .await;
    Ok(Json(
        found
            .into_iter()
            .flatten()
            .filter_map(|(id, b)| {
                let thumbnail_url = b
                    .thumbnail_time
                    .filter(|_| thumbnails)
                    .map(|t| format!("/proxy/dearrow/{id}?t={t}"));
                (b.title.is_some() || thumbnail_url.is_some()).then(|| {
                    let title = b.title.clone();
                    (
                        id,
                        VideoBranding {
                            title,
                            thumbnail_url,
                        },
                    )
                })
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct ThumbQuery {
    t: f64,
}

/// A DeArrow thumbnail (a frame at the submitted time), through the server like all images.
/// 404 while the generator has none yet; the client then keeps YouTube's thumbnail.
pub async fn dearrow_thumbnail(
    State(st): State<AppState>,
    Path(video_id): Path<String>,
    Query(q): Query<ThumbQuery>,
) -> AppResult<Response> {
    let base = st.config.dearrow_thumbnails.trim().trim_end_matches('/');
    if base.is_empty() || st.sponsorblock.is_none() {
        return Err(AppError::NotFound("DeArrow thumbnails are disabled".into()));
    }
    if !is_video_id(&video_id) || !q.t.is_finite() || q.t < 0.0 {
        return Err(AppError::BadRequest("invalid thumbnail".into()));
    }
    let upstream = st
        .http
        .get(format!("{base}/api/v1/getThumbnail"))
        .query(&[("videoID", video_id.as_str()), ("time", &q.t.to_string())])
        .send()
        .await
        .map_err(|e| AppError::Upstream(e.to_string()))?;
    // 204: not generated (yet).
    if upstream.status() != StatusCode::OK {
        return Err(AppError::NotFound("thumbnail".into()));
    }
    image_response(upstream)
}
