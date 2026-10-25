//! Video playback data straight from Innertube.
//!
//! Mirrors what Invidious Companion does: the `TVHTML5_SIMPLY` client (the WEB
//! client only offers server-side ABR streams without URLs), `n`/signature
//! challenges solved with yt-dlp's ejs solver, and a session PO token from
//! `rustypipe-botguard` appended to every stream URL. Without the token
//! googlevideo stops serving after the first ~2.6 MB.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

use crate::{
    model::*,
    solver::Solver,
    source::{YtError, YtResult},
};

/// Used for all requests, including the stream downloads by the proxy.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";
const CLIENT_NAME: &str = "TVHTML5_SIMPLY";
const CLIENT_ID: &str = "75";
const CLIENT_VERSION: &str = "1.0";

/// How often to check whether YouTube rolled out a new player.js.
const PLAYER_CHECK_INTERVAL: Duration = Duration::from_secs(3600);
/// Session PO tokens are valid for at least this long when botguard does not say.
const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(6 * 3600);
const SESSION_REFRESH_MARGIN: Duration = Duration::from_secs(30 * 60);

static PLAYER_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"player\\?/([0-9a-f]{8})\\?/").unwrap());
static STS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:signatureTimestamp|sts)\s*:\s*(\d{5})").unwrap());
static VISITOR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""VISITOR_DATA":"([^"]+)""#).unwrap());

pub struct PlayerClient {
    http: reqwest::Client,
    solver: Solver,
    botguard_bin: PathBuf,
    cache_dir: PathBuf,
    session: Mutex<Option<Session>>,
    player: Mutex<Option<PlayerState>>,
}

#[derive(Clone)]
struct Session {
    visitor_data: String,
    po_token: String,
    created_at: Instant,
    refresh_at: Instant,
}

/// Don't replace a session younger than this: when a token goes bad, many
/// stream requests fail at once and should share one new session.
const MIN_SESSION_AGE: Duration = Duration::from_secs(10);

struct PlayerState {
    js: Arc<PlayerJs>,
    checked_at: Instant,
}

/// One version of YouTube's player.js, preprocessed for the solver.
struct PlayerJs {
    id: String,
    sts: u32,
    preprocessed: String,
}

impl PlayerClient {
    pub fn new(solver: Solver, botguard_bin: PathBuf, cache_dir: PathBuf) -> YtResult<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| YtError::Other(e.to_string()))?;
        Ok(Self {
            http,
            solver,
            botguard_bin,
            cache_dir,
            session: Mutex::new(None),
            player: Mutex::new(None),
        })
    }

    pub async fn playback(&self, video_id: &str) -> YtResult<Playback> {
        let (session, player) = tokio::try_join!(self.session(), self.player_js())?;
        let resp = self.player_request(video_id, &session, &player).await?;
        match self.build_playback(resp, &session, &player).await {
            // A token or session that YouTube no longer accepts shows up as a
            // bot check; start over with a new session once.
            Err(YtError::Blocked(msg)) => {
                tracing::info!(%msg, "player request blocked, renewing session");
                *self.session.lock().await = None;
                let session = self.session().await?;
                let resp = self.player_request(video_id, &session, &player).await?;
                self.build_playback(resp, &session, &player).await
            }
            res => res,
        }
    }

    /// Start a new session (visitor data + PO token) on next use, e.g. after
    /// googlevideo rejected stream URLs. Returns whether the session was dropped.
    pub async fn reset_session(&self) -> bool {
        let mut guard = self.session.lock().await;
        match guard.as_ref() {
            Some(s) if s.created_at.elapsed() >= MIN_SESSION_AGE => {
                tracing::info!("dropping YouTube session after stream rejection");
                *guard = None;
                true
            }
            _ => false,
        }
    }

    async fn player_request(
        &self,
        video_id: &str,
        session: &Session,
        player: &PlayerJs,
    ) -> YtResult<PlayerResponse> {
        let body = json!({
            "context": {
                "client": {
                    "clientName": CLIENT_NAME,
                    "clientVersion": CLIENT_VERSION,
                    "hl": "en",
                    "gl": "US",
                    "visitorData": session.visitor_data,
                }
            },
            "videoId": video_id,
            "contentCheckOk": true,
            "racyCheckOk": true,
            "playbackContext": {
                "contentPlaybackContext": { "signatureTimestamp": player.sts }
            },
        });
        let res = self
            .http
            .post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false")
            .header("X-Goog-Visitor-Id", &session.visitor_data)
            .header("X-Youtube-Client-Name", CLIENT_ID)
            .header("X-Youtube-Client-Version", CLIENT_VERSION)
            .header("Origin", "https://www.youtube.com")
            .json(&body)
            .send()
            .await
            .map_err(http_err)?;
        let status = res.status();
        if !status.is_success() {
            return Err(YtError::Other(format!("player request returned {status}")));
        }
        res.json().await.map_err(http_err)
    }

    async fn build_playback(
        &self,
        resp: PlayerResponse,
        session: &Session,
        player: &PlayerJs,
    ) -> YtResult<Playback> {
        check_playability(&resp.playability_status)?;
        let details = resp
            .video_details
            .ok_or_else(|| YtError::Other("player response without video details".into()))?;
        let streaming = resp
            .streaming_data
            .ok_or_else(|| YtError::Unavailable("no streaming data".into()))?;

        let mut formats = Vec::new();
        for (f, muxed) in streaming
            .formats
            .into_iter()
            .map(|f| (f, true))
            .chain(streaming.adaptive_formats.into_iter().map(|f| (f, false)))
        {
            if f.drm_families.is_some() {
                continue;
            }
            match RawUrl::of(&f) {
                Some(url) => formats.push((f, muxed, url)),
                None => tracing::debug!(itag = f.itag, "format without URL"),
            }
        }
        if formats.is_empty() {
            return Err(YtError::Unavailable("no playable formats".into()));
        }

        let mut n: Vec<String> = formats.iter().filter_map(|(_, _, u)| u.n.clone()).collect();
        let mut sig: Vec<String> = formats
            .iter()
            .filter_map(|(_, _, u)| u.sig.clone())
            .collect();
        n.sort();
        n.dedup();
        sig.sort();
        sig.dedup();
        let solved = self.solver.solve(&player.preprocessed, &n, &sig).await?;

        let streams = formats
            .into_iter()
            .filter_map(|(f, muxed, raw)| {
                let url = raw.finish(&solved, &session.po_token)?;
                Some(stream_from(f, muxed, url))
            })
            .filter(|s| !is_drc(&s.url))
            .collect();

        let expires_in: i64 = streaming
            .expires_in_seconds
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(6 * 3600);
        let now = time::OffsetDateTime::now_utc().unix_timestamp();

        Ok(Playback {
            details: VideoDetails {
                title: details.title.unwrap_or_else(|| details.video_id.clone()),
                id: details.video_id,
                description: details.short_description,
                channel: ChannelRef {
                    id: details.channel_id,
                    name: details.author.unwrap_or_default(),
                    avatar_url: None,
                },
                duration_s: details
                    .length_seconds
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
                view_count: details.view_count.and_then(|s| s.parse().ok()),
                thumbnail_url: details
                    .thumbnail
                    .and_then(|t| {
                        t.thumbnails
                            .into_iter()
                            .max_by_key(|t| t.width.unwrap_or(0))
                    })
                    .map(|t| t.url),
                is_live: details.is_live.unwrap_or(false),
            },
            streams,
            subtitles: resp
                .captions
                .map(|c| c.renderer.caption_tracks)
                .unwrap_or_default()
                .into_iter()
                .map(|t| Subtitle {
                    lang: t.language_code,
                    name: t.name.text(),
                    auto_generated: t.kind.as_deref() == Some("asr"),
                    url: t.base_url,
                })
                .collect(),
            hls_manifest_url: streaming.hls_manifest_url,
            valid_until: now + expires_in,
            user_agent: USER_AGENT.to_owned(),
        })
    }

    /// Visitor data and a PO token bound to it, renewed before it expires.
    async fn session(&self) -> YtResult<Session> {
        let mut guard = self.session.lock().await;
        if let Some(s) = guard.as_ref().filter(|s| Instant::now() < s.refresh_at) {
            return Ok(s.clone());
        }
        let visitor_data = self.fetch_visitor_data().await?;
        let (po_token, ttl) = self.mint_po_token(&visitor_data).await?;
        let session = Session {
            visitor_data,
            po_token,
            created_at: Instant::now(),
            refresh_at: Instant::now() + ttl.saturating_sub(SESSION_REFRESH_MARGIN),
        };
        tracing::info!(ttl_s = ttl.as_secs(), "new YouTube session");
        *guard = Some(session.clone());
        Ok(session)
    }

    async fn fetch_visitor_data(&self) -> YtResult<String> {
        let html = self
            .http
            .get("https://www.youtube.com/")
            // Skip the EU cookie consent interstitial.
            .header("Cookie", "SOCS=CAI")
            .header("Accept-Language", "en-US")
            .send()
            .await
            .map_err(http_err)?
            .text()
            .await
            .map_err(http_err)?;
        VISITOR_RE
            .captures(&html)
            .map(|c| c[1].to_owned())
            .ok_or_else(|| YtError::Other("could not find visitor data".into()))
    }

    async fn mint_po_token(&self, visitor_data: &str) -> YtResult<(String, Duration)> {
        // No snapshot: tokens minted from a reused BotGuard snapshot go stale and
        // YouTube starts rejecting them (403 after the first ~3 MB of a stream).
        // Minting from scratch costs about a second, once per session.
        let out = tokio::process::Command::new(&self.botguard_bin)
            .arg("--no-snapshot")
            .arg("--")
            .arg(visitor_data)
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => YtError::Other(format!(
                    "rustypipe-botguard not found ({}). It is needed to play videos: install it with \
                     `cargo install rustypipe-botguard --root <data_dir>/tools` or set botguard_bin",
                    self.botguard_bin.display()
                )),
                _ => YtError::Other(format!("run {}: {e}", self.botguard_bin.display())),
            })?;
        if !out.status.success() {
            return Err(YtError::Other(format!(
                "rustypipe-botguard failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        parse_botguard_output(&String::from_utf8_lossy(&out.stdout))
    }

    /// Current player.js, checked hourly; preprocessing results are cached on disk
    /// since they take a few seconds and only change when YouTube ships a new player.
    async fn player_js(&self) -> YtResult<Arc<PlayerJs>> {
        let mut guard = self.player.lock().await;
        if let Some(p) = guard
            .as_ref()
            .filter(|p| p.checked_at.elapsed() < PLAYER_CHECK_INTERVAL)
        {
            return Ok(p.js.clone());
        }
        let id = match self.fetch_player_id().await {
            Ok(id) => id,
            // Keep using the known player if the check fails.
            Err(e) => match guard.as_mut() {
                Some(p) => {
                    tracing::warn!(error = %e, "player version check failed");
                    p.checked_at = Instant::now();
                    return Ok(p.js.clone());
                }
                None => return Err(e),
            },
        };
        let js = match guard.as_ref().filter(|p| p.js.id == id) {
            Some(p) => p.js.clone(),
            None => Arc::new(self.load_player(&id).await?),
        };
        *guard = Some(PlayerState {
            js: js.clone(),
            checked_at: Instant::now(),
        });
        Ok(js)
    }

    async fn fetch_player_id(&self) -> YtResult<String> {
        let js = self
            .http
            .get("https://www.youtube.com/iframe_api")
            .send()
            .await
            .map_err(http_err)?
            .text()
            .await
            .map_err(http_err)?;
        PLAYER_ID_RE
            .captures(&js)
            .map(|c| c[1].to_owned())
            .ok_or_else(|| YtError::Other("could not find player id".into()))
    }

    async fn load_player(&self, id: &str) -> YtResult<PlayerJs> {
        let cache = PlayerCache::new(&self.cache_dir, id);
        if let Some(p) = cache.load() {
            return Ok(p);
        }
        let url = format!("https://www.youtube.com/s/player/{id}/player_ias.vflset/en_US/base.js");
        let player_js = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(http_err)?
            .text()
            .await
            .map_err(http_err)?;
        let sts = STS_RE
            .captures(&player_js)
            .and_then(|c| c[1].parse().ok())
            .ok_or_else(|| YtError::Other(format!("no signature timestamp in player {id}")))?;
        let started = Instant::now();
        let (_, preprocessed) = self.solver.solve_player(&player_js, &[], &[]).await?;
        tracing::info!(
            player = id,
            sts,
            took_ms = started.elapsed().as_millis() as u64,
            "preprocessed player.js"
        );
        let p = PlayerJs {
            id: id.to_owned(),
            sts,
            preprocessed,
        };
        cache.store(&p);
        Ok(p)
    }
}

struct PlayerCache {
    path: PathBuf,
}

impl PlayerCache {
    fn new(dir: &Path, id: &str) -> Self {
        Self {
            path: dir.join("players").join(format!("{id}.js")),
        }
    }

    /// File format: first line `// sts=<n>`, then the preprocessed player.
    fn load(&self) -> Option<PlayerJs> {
        let content = std::fs::read_to_string(&self.path).ok()?;
        let (header, preprocessed) = content.split_once('\n')?;
        let sts = header.strip_prefix("// sts=")?.parse().ok()?;
        let id = self.path.file_stem()?.to_str()?.to_owned();
        Some(PlayerJs {
            id,
            sts,
            preprocessed: preprocessed.to_owned(),
        })
    }

    fn store(&self, p: &PlayerJs) {
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(self.path.parent().unwrap())?;
            let tmp = self.path.with_extension("tmp");
            std::fs::write(&tmp, format!("// sts={}\n{}", p.sts, p.preprocessed))?;
            std::fs::rename(tmp, &self.path)
        };
        if let Err(e) = write() {
            tracing::warn!(path = %self.path.display(), error = %e, "could not cache player");
        }
    }
}

fn parse_botguard_output(out: &str) -> YtResult<(String, Duration)> {
    let mut words = out.split_whitespace();
    let token = words
        .next()
        .ok_or_else(|| YtError::Other("rustypipe-botguard returned no token".into()))?
        .to_owned();
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let ttl = words
        .filter_map(|w| w.strip_prefix("valid_until="))
        .find_map(|v| v.parse::<i64>().ok())
        .map(|until| Duration::from_secs(until.saturating_sub(now).max(0) as u64))
        .unwrap_or(DEFAULT_SESSION_TTL);
    Ok((token, ttl))
}

fn check_playability(p: &PlayabilityStatus) -> YtResult<()> {
    let reason = || p.reason.clone().unwrap_or_else(|| p.status.clone());
    match p.status.as_str() {
        "OK" => Ok(()),
        "LOGIN_REQUIRED" if reason().contains("bot") => Err(YtError::Blocked(reason())),
        "ERROR" if reason().contains("unavailable") => Err(YtError::NotFound(reason())),
        _ => Err(YtError::Unavailable(reason())),
    }
}

/// Stream URL before solving: the `n` and optional signature challenges.
struct RawUrl {
    url: url::Url,
    n: Option<String>,
    sig: Option<String>,
    sig_param: String,
}

impl RawUrl {
    fn of(f: &Format) -> Option<Self> {
        let (url, sig, sig_param) = match (&f.url, &f.signature_cipher) {
            (Some(u), _) => (u.clone(), None, String::new()),
            (None, Some(cipher)) => {
                let params: HashMap<_, _> =
                    url::form_urlencoded::parse(cipher.as_bytes()).collect();
                (
                    params.get("url")?.to_string(),
                    Some(params.get("s")?.to_string()),
                    params
                        .get("sp")
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "signature".into()),
                )
            }
            (None, None) => return None,
        };
        let url = url::Url::parse(&url).ok()?;
        let n = url
            .query_pairs()
            .find(|(k, _)| k == "n")
            .map(|(_, v)| v.into_owned());
        Some(Self {
            url,
            n,
            sig,
            sig_param,
        })
    }

    fn finish(self, solved: &crate::solver::Solutions, po_token: &str) -> Option<String> {
        let n = match &self.n {
            Some(n) => Some(solved.n.get(n)?.clone()),
            None => None,
        };
        let sig = match &self.sig {
            Some(s) => Some(solved.sig.get(s)?.clone()),
            None => None,
        };
        let mut url = self.url;
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(k, _)| k != "n")
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        {
            let mut q = url.query_pairs_mut();
            q.clear();
            for (k, v) in &pairs {
                q.append_pair(k, v);
            }
            if let Some(n) = &n {
                q.append_pair("n", n);
            }
            if let Some(sig) = &sig {
                q.append_pair(&self.sig_param, sig);
            }
            q.append_pair("pot", po_token);
        }
        Some(url.into())
    }
}

/// The TV client reports VP9 as plain `vp9`, which Media Source Extensions
/// reject as ambiguous. Declare profile 0 (8-bit) or 2 (10-bit HDR) at level 5.1,
/// which covers everything YouTube serves up to 4K.
fn full_codec(codecs: String, hdr: bool) -> String {
    match codecs.as_str() {
        "vp9" if hdr => "vp09.02.51.10".into(),
        "vp9" => "vp09.00.51.08".into(),
        _ => codecs,
    }
}

fn stream_from(f: Format, muxed: bool, url: String) -> Stream {
    let (mime_type, codecs) = split_mime(&f.mime_type);
    let hdr = f.color_info.as_ref().is_some_and(|c| {
        matches!(
            c.transfer_characteristics.as_deref(),
            Some(
                "COLOR_TRANSFER_CHARACTERISTICS_SMPTEST2084"
                    | "COLOR_TRANSFER_CHARACTERISTICS_ARIB_STD_B67"
            )
        )
    });
    let codecs = full_codec(codecs, hdr);
    let kind = if muxed {
        StreamKind::Muxed
    } else if mime_type.starts_with("audio/") {
        StreamKind::Audio
    } else {
        StreamKind::Video
    };
    let track = f.audio_track.map(|t| AudioTrackInfo {
        lang: t.id.split('.').next().map(str::to_owned),
        id: t.id,
        name: t.display_name.unwrap_or_default(),
        is_default: t.audio_is_default.unwrap_or(false),
    });
    let is_video = kind != StreamKind::Audio;
    Stream {
        id: stream_id(f.itag, track.as_ref().map(|t| t.id.as_str())),
        kind,
        itag: f.itag,
        url,
        mime_type,
        codecs,
        bitrate: f.bitrate.unwrap_or(0),
        size: f.content_length.and_then(|s| s.parse().ok()),
        init_range: f.init_range.and_then(Range::parse),
        index_range: f.index_range.and_then(Range::parse),
        width: f.width.filter(|_| is_video),
        height: f.height.filter(|_| is_video),
        fps: f.fps.filter(|_| is_video),
        quality_label: f.quality_label,
        hdr,
        audio_channels: f.audio_channels,
        audio_track: track,
    }
}

/// Split `video/mp4; codecs="avc1.640028"` into mime type and codecs.
pub(crate) fn split_mime(mime: &str) -> (String, String) {
    let mut parts = mime.splitn(2, ';');
    let mime_type = parts.next().unwrap_or_default().trim().to_owned();
    let codecs = parts
        .next()
        .and_then(|p| p.trim().strip_prefix("codecs="))
        .map(|c| c.trim_matches('"').to_owned())
        .unwrap_or_default();
    (mime_type, codecs)
}

/// Dynamic range compression is flagged in the `xtags` URL parameter, alone
/// (`drc=1`) or next to track info (`acont=original:drc=1:lang=en-US`).
pub(crate) fn is_drc(url: &str) -> bool {
    url.split(['?', '&'])
        .filter_map(|p| p.strip_prefix("xtags="))
        .any(|v| v.split("%3A").any(|tag| tag == "drc%3D1"))
}

/// Stream id safe for use in URL paths, e.g. `251` or `251-de.4`.
pub(crate) fn stream_id(itag: u32, track: Option<&str>) -> String {
    match track {
        Some(t) => {
            let t: String = t
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                .collect();
            format!("{itag}-{t}")
        }
        None => itag.to_string(),
    }
}

fn http_err(e: reqwest::Error) -> YtError {
    YtError::Other(format!("http: {e}"))
}

// --- Innertube player response (only the fields we use) ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayerResponse {
    playability_status: PlayabilityStatus,
    video_details: Option<RawVideoDetails>,
    streaming_data: Option<StreamingData>,
    captions: Option<Captions>,
}

#[derive(Deserialize)]
struct PlayabilityStatus {
    status: String,
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawVideoDetails {
    video_id: String,
    title: Option<String>,
    length_seconds: Option<String>,
    channel_id: String,
    author: Option<String>,
    short_description: Option<String>,
    view_count: Option<String>,
    thumbnail: Option<Thumbnails>,
    is_live: Option<bool>,
}

#[derive(Deserialize)]
struct Thumbnails {
    thumbnails: Vec<RawThumbnail>,
}

#[derive(Deserialize)]
struct RawThumbnail {
    url: String,
    width: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamingData {
    expires_in_seconds: Option<String>,
    #[serde(default)]
    formats: Vec<Format>,
    #[serde(default)]
    adaptive_formats: Vec<Format>,
    hls_manifest_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Format {
    itag: u32,
    url: Option<String>,
    signature_cipher: Option<String>,
    mime_type: String,
    bitrate: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<u8>,
    quality_label: Option<String>,
    content_length: Option<String>,
    init_range: Option<Range>,
    index_range: Option<Range>,
    audio_channels: Option<u8>,
    audio_track: Option<RawAudioTrack>,
    color_info: Option<ColorInfo>,
    drm_families: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Range {
    start: String,
    end: String,
}

impl Range {
    fn parse(self) -> Option<ByteRange> {
        Some(ByteRange {
            start: self.start.parse().ok()?,
            end: self.end.parse().ok()?,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAudioTrack {
    id: String,
    display_name: Option<String>,
    audio_is_default: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ColorInfo {
    transfer_characteristics: Option<String>,
}

#[derive(Deserialize)]
struct Captions {
    #[serde(rename = "playerCaptionsTracklistRenderer")]
    renderer: CaptionList,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaptionList {
    #[serde(default)]
    caption_tracks: Vec<CaptionTrack>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaptionTrack {
    base_url: String,
    language_code: String,
    name: Text,
    kind: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Text {
    simple_text: Option<String>,
    #[serde(default)]
    runs: Vec<TextRun>,
}

#[derive(Deserialize)]
struct TextRun {
    text: String,
}

impl Text {
    fn text(self) -> String {
        self.simple_text
            .unwrap_or_else(|| self.runs.into_iter().map(|r| r.text).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::Solutions;

    #[test]
    fn vp9_codec_strings_are_completed() {
        assert_eq!(full_codec("vp9".into(), false), "vp09.00.51.08");
        assert_eq!(full_codec("vp9".into(), true), "vp09.02.51.10");
        assert_eq!(full_codec("vp09.00.40.08".into(), false), "vp09.00.40.08");
        assert_eq!(full_codec("avc1.640028".into(), false), "avc1.640028");
    }

    #[test]
    fn splits_mime_and_codecs() {
        assert_eq!(
            split_mime(r#"video/mp4; codecs="avc1.640028""#),
            ("video/mp4".into(), "avc1.640028".into())
        );
        assert_eq!(
            split_mime("audio/webm"),
            ("audio/webm".into(), String::new())
        );
    }

    #[test]
    fn detects_drc_variants() {
        assert!(is_drc("https://x/videoplayback?itag=140&xtags=drc%3D1&n=1"));
        assert!(is_drc(
            "https://x/videoplayback?xtags=acont%3Doriginal%3Adrc%3D1%3Alang%3Den-US"
        ));
        assert!(!is_drc(
            "https://x/videoplayback?xtags=acont%3Doriginal%3Alang%3Den-US"
        ));
        assert!(!is_drc("https://x/videoplayback?itag=140"));
    }

    #[test]
    fn stream_ids_are_path_safe() {
        assert_eq!(stream_id(251, None), "251");
        assert_eq!(stream_id(251, Some("de.4")), "251-de.4");
        assert_eq!(stream_id(140, Some("en/../x?y")), "140-en..xy");
    }

    #[test]
    fn finishes_urls_with_solutions_and_token() {
        let f = Format {
            itag: 251,
            url: None,
            signature_cipher: Some(
                "s=SIGCHALLENGE&sp=sig&url=https%3A%2F%2Frr1.googlevideo.com%2Fvideoplayback%3Fitag%3D251%26n%3DNCHALLENGE%26ip%3D1.2.3.4"
                    .into(),
            ),
            mime_type: r#"audio/webm; codecs="opus""#.into(),
            bitrate: None,
            width: None,
            height: None,
            fps: None,
            quality_label: None,
            content_length: None,
            init_range: None,
            index_range: None,
            audio_channels: None,
            audio_track: None,
            color_info: None,
            drm_families: None,
        };
        let raw = RawUrl::of(&f).unwrap();
        assert_eq!(raw.n.as_deref(), Some("NCHALLENGE"));
        assert_eq!(raw.sig.as_deref(), Some("SIGCHALLENGE"));
        let solved = Solutions {
            n: [("NCHALLENGE".into(), "nsolved".into())].into(),
            sig: [("SIGCHALLENGE".into(), "sig/solved=".into())].into(),
        };
        let url = raw.finish(&solved, "TOKEN").unwrap();
        assert_eq!(
            url,
            "https://rr1.googlevideo.com/videoplayback?itag=251&ip=1.2.3.4&n=nsolved&sig=sig%2Fsolved%3D&pot=TOKEN"
        );
    }

    #[test]
    fn unsolved_challenges_drop_the_stream() {
        let f: Format = serde_json::from_value(serde_json::json!({
            "itag": 18, "mimeType": "video/mp4", "url": "https://x.googlevideo.com/videoplayback?n=abc"
        }))
        .unwrap();
        assert!(
            RawUrl::of(&f)
                .unwrap()
                .finish(&Solutions::default(), "T")
                .is_none()
        );
    }

    #[test]
    fn parses_botguard_output() {
        let (t, ttl) = parse_botguard_output("TOKEN from_snapshot=true\n").unwrap();
        assert_eq!(t, "TOKEN");
        assert_eq!(ttl, DEFAULT_SESSION_TTL);
        let until = time::OffsetDateTime::now_utc().unix_timestamp() + 3600;
        let (_, ttl) = parse_botguard_output(&format!("TOKEN valid_until={until}")).unwrap();
        assert!(ttl.as_secs() > 3500 && ttl.as_secs() <= 3600);
        assert!(parse_botguard_output("").is_err());
    }

    #[test]
    fn maps_playability() {
        let s = |status: &str, reason: &str| PlayabilityStatus {
            status: status.into(),
            reason: Some(reason.into()),
        };
        assert!(check_playability(&s("OK", "")).is_ok());
        assert!(matches!(
            check_playability(&s("LOGIN_REQUIRED", "Sign in to confirm you’re not a bot")),
            Err(YtError::Blocked(_))
        ));
        assert!(matches!(
            check_playability(&s("ERROR", "This video is unavailable")),
            Err(YtError::NotFound(_))
        ));
        assert!(matches!(
            check_playability(&s("LOGIN_REQUIRED", "Sign in to confirm your age")),
            Err(YtError::Unavailable(_))
        ));
    }
}
