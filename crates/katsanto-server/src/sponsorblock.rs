//! SponsorBlock segments and DeArrow titles/thumbnails (both served by the SponsorBlock
//! API). Requests go through the server, and only the first characters of the video
//! id's SHA-256 hash are sent (the API's k-anonymity lookup), so neither the viewer's
//! IP nor the exact video leaves the instance. Results are cached for an hour.

use std::{collections::HashMap, sync::Arc, time::Duration};

use moka::future::Cache;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use crate::error::{AppError, AppResult};

/// Hash prefix length: about 1 in 65 536 of all videos share a prefix.
const PREFIX_LEN: usize = 4;
const TTL: Duration = Duration::from_secs(3600);

/// Every category the player knows; the user's settings decide what happens to each.
pub const CATEGORIES: &[&str] = &[
    "sponsor",
    "selfpromo",
    "interaction",
    "intro",
    "outro",
    "preview",
    "hook",
    "music_offtopic",
    "filler",
    "poi_highlight",
];

#[derive(Clone, Debug, Serialize, ToSchema, PartialEq)]
pub struct Segment {
    pub category: String,
    /// `skip`, `mute`, `poi` (a highlight: a single point) or `full` (the whole video is
    /// e.g. sponsored).
    pub action: String,
    pub start_s: f64,
    pub end_s: f64,
    pub uuid: String,
}

/// Community branding for a video; `None` fields keep YouTube's.
#[derive(Clone, Debug, Default, Serialize, ToSchema, PartialEq)]
pub struct Branding {
    pub title: Option<String>,
    /// Frame (seconds) to show as the thumbnail.
    pub thumbnail_time: Option<f64>,
}

pub struct SponsorBlock {
    http: reqwest::Client,
    /// API base URL, e.g. `https://sponsor.ajay.app`.
    api: String,
    segments: Cache<String, Arc<Vec<Segment>>>,
    branding: Cache<String, Arc<Branding>>,
}

impl SponsorBlock {
    pub fn new(http: reqwest::Client, api: &str) -> Self {
        Self {
            http,
            api: api.trim_end_matches('/').to_owned(),
            segments: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(TTL)
                .build(),
            branding: Cache::builder()
                .max_capacity(50_000)
                .time_to_live(TTL)
                .build(),
        }
    }

    pub async fn segments(&self, video_id: &str) -> AppResult<Arc<Vec<Segment>>> {
        let id = video_id.to_owned();
        self.segments
            .try_get_with(id.clone(), async {
                let categories = serde_json::to_string(CATEGORIES).unwrap();
                let url = format!(
                    "{}/api/skipSegments/{}?categories={}&actionTypes={}",
                    self.api,
                    hash_prefix(&id),
                    encode(&categories),
                    encode(r#"["skip","mute","poi","full"]"#),
                );
                let body = self.get(&url).await?;
                Ok::<_, AppError>(Arc::new(parse_segments(&body, &id)?))
            })
            .await
            .map_err(|e| AppError::Upstream(e.to_string()))
    }

    pub async fn branding(&self, video_id: &str) -> AppResult<Arc<Branding>> {
        let id = video_id.to_owned();
        self.branding
            .try_get_with(id.clone(), async {
                let url = format!("{}/api/branding/{}", self.api, hash_prefix(&id));
                let body = self.get(&url).await?;
                Ok::<_, AppError>(Arc::new(parse_branding(&body, &id)?))
            })
            .await
            .map_err(|e| AppError::Upstream(e.to_string()))
    }

    /// The body, or `None` when the API has nothing for the prefix (404).
    async fn get(&self, url: &str) -> AppResult<Option<String>> {
        let res = self
            .http
            .get(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| AppError::Upstream(format!("SponsorBlock: {e}")))?;
        match res.status() {
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            s if s.is_success() => res
                .text()
                .await
                .map(Some)
                .map_err(|e| AppError::Upstream(format!("SponsorBlock: {e}"))),
            s => Err(AppError::Upstream(format!("SponsorBlock returned {s}"))),
        }
    }
}

fn hash_prefix(video_id: &str) -> String {
    let hash = Sha256::digest(video_id.as_bytes());
    hash.iter().map(|b| format!("{b:02x}")).collect::<String>()[..PREFIX_LEN].to_owned()
}

fn encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn bad(e: serde_json::Error) -> AppError {
    AppError::Upstream(format!("SponsorBlock: unexpected response: {e}"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VideoSegments {
    #[serde(rename = "videoID")]
    video_id: String,
    segments: Vec<RawSegment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSegment {
    category: String,
    action_type: String,
    segment: (f64, f64),
    #[serde(rename = "UUID")]
    uuid: String,
}

fn parse_segments(body: &Option<String>, video_id: &str) -> AppResult<Vec<Segment>> {
    let Some(body) = body else {
        return Ok(Vec::new());
    };
    let videos: Vec<VideoSegments> = serde_json::from_str(body).map_err(bad)?;
    let mut segments: Vec<Segment> = videos
        .into_iter()
        .filter(|v| v.video_id == video_id)
        .flat_map(|v| v.segments)
        .filter(|s| s.segment.0.is_finite() && s.segment.1 >= s.segment.0)
        .map(|s| Segment {
            category: s.category,
            action: s.action_type,
            start_s: s.segment.0,
            end_s: s.segment.1,
            uuid: s.uuid,
        })
        .collect();
    segments.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    Ok(segments)
}

#[derive(Deserialize)]
struct VideoBranding {
    #[serde(default)]
    titles: Vec<RawTitle>,
    #[serde(default)]
    thumbnails: Vec<RawThumbnail>,
}

#[derive(Deserialize)]
struct RawTitle {
    title: String,
    #[serde(default)]
    original: bool,
    #[serde(default)]
    votes: i64,
    #[serde(default)]
    locked: bool,
}

#[derive(Deserialize)]
struct RawThumbnail {
    timestamp: Option<f64>,
    #[serde(default)]
    original: bool,
    #[serde(default)]
    votes: i64,
    #[serde(default)]
    locked: bool,
}

/// Pick submissions the way the DeArrow extension does: the API sorts them best first;
/// use the top one if the community trusts it (locked, or not voted down) and it isn't
/// "keep the original".
fn parse_branding(body: &Option<String>, video_id: &str) -> AppResult<Branding> {
    let Some(body) = body else {
        return Ok(Branding::default());
    };
    let mut videos: HashMap<String, VideoBranding> = serde_json::from_str(body).map_err(bad)?;
    let Some(v) = videos.remove(video_id) else {
        return Ok(Branding::default());
    };
    let title = v
        .titles
        .into_iter()
        .next()
        .filter(|t| !t.original && (t.locked || t.votes >= 0))
        // `>` marks words that must not be auto-capitalized; Katsanto shows titles as submitted.
        .map(|t| t.title.replace('>', "").trim().to_owned())
        .filter(|t| !t.is_empty());
    let thumbnail_time = v
        .thumbnails
        .into_iter()
        .next()
        .filter(|t| !t.original && (t.locked || t.votes >= 0))
        .and_then(|t| t.timestamp)
        .filter(|t| t.is_finite() && *t >= 0.0);
    Ok(Branding {
        title,
        thumbnail_time,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_prefix_is_sha256() {
        assert_eq!(
            hash_prefix("abc"),
            "ba78" // sha256("abc") = ba7816bf…
        );
    }

    #[test]
    fn segments_for_the_right_video_in_order() {
        let body = Some(
            r#"[
              {"videoID":"other000000","segments":[{"category":"sponsor","actionType":"skip","segment":[1,2],"UUID":"x"}]},
              {"videoID":"aaaaaaaaaaa","hash":"…","segments":[
                {"category":"outro","actionType":"skip","segment":[500.5,600],"UUID":"b","votes":3},
                {"category":"sponsor","actionType":"skip","segment":[10,42.25],"UUID":"a","locked":1},
                {"category":"poi_highlight","actionType":"poi","segment":[120,120],"UUID":"c"}
              ]}
            ]"#
            .to_owned(),
        );
        let s = parse_segments(&body, "aaaaaaaaaaa").unwrap();
        assert_eq!(
            s.iter()
                .map(|s| (s.category.as_str(), s.start_s))
                .collect::<Vec<_>>(),
            [
                ("sponsor", 10.0),
                ("poi_highlight", 120.0),
                ("outro", 500.5)
            ]
        );
        assert!(parse_segments(&None, "aaaaaaaaaaa").unwrap().is_empty());
    }

    #[test]
    fn branding_uses_trusted_non_original_submissions() {
        let body = Some(
            r#"{
              "aaaaaaaaaaa": {
                "titles": [{"title":"A >calm title","original":false,"votes":2,"locked":false,"UUID":"t"}],
                "thumbnails": [{"timestamp":42.5,"original":false,"votes":0,"locked":false,"UUID":"u"}],
                "randomTime": 0.3, "videoDuration": 100
              },
              "bbbbbbbbbbb": {
                "titles": [{"title":"Downvoted","original":false,"votes":-1,"locked":false}],
                "thumbnails": [{"timestamp":null,"original":true,"votes":5,"locked":false}]
              }
            }"#
            .to_owned(),
        );
        assert_eq!(
            parse_branding(&body, "aaaaaaaaaaa").unwrap(),
            Branding {
                title: Some("A calm title".into()),
                thumbnail_time: Some(42.5)
            }
        );
        assert_eq!(
            parse_branding(&body, "bbbbbbbbbbb").unwrap(),
            Branding::default()
        );
        assert_eq!(
            parse_branding(&body, "ccccccccccc").unwrap(),
            Branding::default()
        );
    }
}
