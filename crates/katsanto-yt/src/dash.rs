//! DASH manifest generation from YouTube's adaptive streams.
//!
//! YouTube serves video and audio as separate fragmented MP4 / WebM files with a
//! `sidx` index. Each one maps to a DASH `Representation` with `SegmentBase`, so a
//! static on-demand manifest is enough for dash.js/Shaka to do adaptive playback.

use std::fmt::Write;

use crate::model::{Playback, Stream, StreamKind};

/// Which audio the manifest offers. Only one container is used for all languages:
/// players switch audio tracks within a single source buffer, so mixing AAC/MP4 and
/// Opus/WebM tracks breaks switching (and lists every language twice).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioFormat {
    /// AAC in MP4: plays everywhere, including older Safari. YouTube's AAC streams
    /// start with ~36 ms of encoder priming and no edit list to skip it, so the sound
    /// runs slightly behind the picture.
    #[default]
    Aac,
    /// Opus in WebM: the decoder delay is in the container and browsers compensate,
    /// so audio and video line up exactly. Use it when the browser can play it.
    Opus,
}

/// Build an MPD for the video's adaptive streams. `stream_url` decides where the
/// player fetches each stream from (the proxy, or googlevideo directly).
pub fn manifest(
    playback: &Playback,
    audio: AudioFormat,
    stream_url: impl Fn(&Stream) -> String,
) -> String {
    let audio_mime = preferred_audio_mime(playback, audio);
    let mut sets: Vec<(SetKey, Vec<&Stream>)> = Vec::new();
    for s in playback
        .streams
        .iter()
        .filter(|s| is_dash_capable(s))
        .filter(|s| s.kind != StreamKind::Audio || Some(s.mime_type.as_str()) == audio_mime)
    {
        let key = SetKey::of(s);
        match sets.iter_mut().find(|(k, _)| *k == key) {
            Some((_, streams)) => streams.push(s),
            None => sets.push((key, vec![s])),
        }
    }
    // Video sets first, then the default audio track first, so players without
    // explicit track selection pick sensible defaults.
    sets.sort_by_key(|(k, _)| (k.is_audio, !k.is_default_track, k.hdr));

    let mut out = String::new();
    let _ = write!(
        out,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" profiles="urn:mpeg:dash:profile:isoff-on-demand:2011" type="static" minBufferTime="PT1.5S" mediaPresentationDuration="PT{}S">
<Period>
"#,
        playback.details.duration_s
    );

    for (id, (key, mut streams)) in sets.into_iter().enumerate() {
        streams.sort_by_key(|s| s.bitrate);
        let content_type = if key.is_audio { "audio" } else { "video" };
        let _ = write!(
            out,
            r#"<AdaptationSet id="{id}" contentType="{content_type}" mimeType="{}" subsegmentAlignment="true" subsegmentStartsWithSAP="1""#,
            esc(&key.mime_type)
        );
        if let Some(lang) = &key.lang {
            let _ = write!(out, r#" lang="{}""#, esc(lang));
        }
        if key.is_audio {
            // dash.js picks the initial track by selectionPriority (then bitrate);
            // without it a dubbed track with a higher bitrate would win.
            let priority = if key.is_default_track { 2 } else { 1 };
            let _ = write!(out, r#" selectionPriority="{priority}""#);
        }
        out.push_str(">\n");
        if let Some(label) = &key.track_name {
            let _ = writeln!(out, "<Label>{}</Label>", esc(label));
        }
        if key.is_audio {
            let role = if key.is_default_track {
                "main"
            } else {
                "alternate"
            };
            let _ = writeln!(
                out,
                r#"<Role schemeIdUri="urn:mpeg:dash:role:2011" value="{role}"/>"#
            );
        }
        for s in streams {
            write_representation(&mut out, s, &stream_url(s));
        }
        out.push_str("</AdaptationSet>\n");
    }

    out.push_str("</Period>\n</MPD>\n");
    out
}

fn write_representation(out: &mut String, s: &Stream, url: &str) {
    let _ = write!(
        out,
        r#"<Representation id="{}" codecs="{}" bandwidth="{}""#,
        esc(&s.id),
        esc(&s.codecs),
        s.bitrate
    );
    if let (Some(w), Some(h)) = (s.width, s.height) {
        let _ = write!(out, r#" width="{w}" height="{h}""#);
    }
    if let Some(fps) = s.fps {
        let _ = write!(out, r#" frameRate="{fps}""#);
    }
    out.push_str(">\n");
    if let Some(ch) = s.audio_channels {
        let _ = writeln!(
            out,
            r#"<AudioChannelConfiguration schemeIdUri="urn:mpeg:dash:23003:3:audio_channel_configuration:2011" value="{ch}"/>"#
        );
    }
    // Both ranges are checked in `is_dash_capable`.
    let (init, index) = (s.init_range.unwrap(), s.index_range.unwrap());
    let _ = write!(
        out,
        r#"<BaseURL>{}</BaseURL>
<SegmentBase indexRange="{}-{}"><Initialization range="{}-{}"/></SegmentBase>
</Representation>
"#,
        esc(url),
        index.start,
        index.end,
        init.start,
        init.end
    );
}

fn preferred_audio_mime(playback: &Playback, format: AudioFormat) -> Option<&str> {
    let audio = || {
        playback
            .streams
            .iter()
            .filter(|s| s.kind == StreamKind::Audio && is_dash_capable(s))
    };
    let wanted = match format {
        AudioFormat::Aac => "audio/mp4",
        AudioFormat::Opus => "audio/webm",
    };
    audio()
        .find(|s| s.mime_type == wanted)
        .or_else(|| audio().next())
        .map(|s| s.mime_type.as_str())
}

fn is_dash_capable(s: &Stream) -> bool {
    matches!(s.kind, StreamKind::Video | StreamKind::Audio)
        && s.init_range.is_some()
        && s.index_range.is_some()
        && !s.codecs.is_empty()
}

/// Streams that can switch between each other share an adaptation set: same
/// container, codec family, dynamic range and (for audio) language track.
#[derive(PartialEq, Eq)]
struct SetKey {
    is_audio: bool,
    mime_type: String,
    codec_family: String,
    hdr: bool,
    lang: Option<String>,
    track_name: Option<String>,
    is_default_track: bool,
}

impl SetKey {
    fn of(s: &Stream) -> Self {
        let track = s.audio_track.as_ref();
        Self {
            is_audio: s.kind == StreamKind::Audio,
            mime_type: s.mime_type.clone(),
            codec_family: s.codecs.split('.').next().unwrap_or_default().to_owned(),
            hdr: s.hdr,
            lang: track.and_then(|t| t.lang.clone()),
            track_name: track.map(|t| t.name.clone()),
            is_default_track: track.is_none_or(|t| t.is_default),
        }
    }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AudioTrackInfo, ByteRange, ChannelRef, VideoDetails};

    fn stream(kind: StreamKind, itag: u32, mime: &str, codecs: &str, bitrate: u32) -> Stream {
        Stream {
            id: itag.to_string(),
            kind,
            itag,
            url: format!("https://example.com/{itag}?a=1&b=2"),
            mime_type: mime.into(),
            codecs: codecs.into(),
            bitrate,
            size: Some(1000),
            init_range: Some(ByteRange { start: 0, end: 739 }),
            index_range: Some(ByteRange {
                start: 740,
                end: 1999,
            }),
            width: (kind != StreamKind::Audio).then_some(1920),
            height: (kind != StreamKind::Audio).then_some(1080),
            fps: (kind != StreamKind::Audio).then_some(30),
            quality_label: None,
            hdr: false,
            audio_channels: (kind == StreamKind::Audio).then_some(2),
            audio_track: None,
        }
    }

    fn playback(streams: Vec<Stream>) -> Playback {
        Playback {
            details: VideoDetails {
                id: "abc".into(),
                title: "t".into(),
                description: None,
                channel: ChannelRef {
                    id: "UC".into(),
                    name: "c".into(),
                    avatar_url: None,
                },
                duration_s: 213,
                view_count: None,
                thumbnail_url: None,
                is_live: false,
            },
            streams,
            subtitles: vec![],
            hls_manifest_url: None,
            valid_until: 0,
            user_agent: String::new(),
        }
    }

    #[test]
    fn groups_streams_into_adaptation_sets() {
        let p = playback(vec![
            stream(
                StreamKind::Video,
                137,
                "video/mp4",
                "avc1.640028",
                4_000_000,
            ),
            stream(
                StreamKind::Video,
                136,
                "video/mp4",
                "avc1.4d401f",
                2_000_000,
            ),
            stream(StreamKind::Video, 248, "video/webm", "vp9", 3_000_000),
            stream(StreamKind::Audio, 140, "audio/mp4", "mp4a.40.2", 128_000),
            stream(
                StreamKind::Muxed,
                18,
                "video/mp4",
                "avc1.42001E, mp4a.40.2",
                500_000,
            ),
        ]);
        let mpd = manifest(&p, AudioFormat::Aac, |s| {
            format!("/proxy/video/abc/{}", s.itag)
        });

        assert!(mpd.contains(r#"mediaPresentationDuration="PT213S""#));
        assert_eq!(mpd.matches("<AdaptationSet").count(), 3);
        assert_eq!(mpd.matches("<Representation").count(), 4);
        // muxed stream is not part of the manifest
        assert!(!mpd.contains(r#"id="18""#));
        // lower bitrate first within a set
        assert!(mpd.find(r#"id="136""#) < mpd.find(r#"id="137""#));
        assert!(mpd.contains(
            r#"<SegmentBase indexRange="740-1999"><Initialization range="0-739"/></SegmentBase>"#
        ));
        assert!(mpd.contains("<BaseURL>/proxy/video/abc/140</BaseURL>"));
    }

    #[test]
    fn escapes_urls_and_orders_audio_tracks() {
        let mut dubbed = stream(StreamKind::Audio, 251, "audio/webm", "opus", 160_000);
        dubbed.id = "251-de.4".into();
        dubbed.audio_track = Some(AudioTrackInfo {
            id: "de.4".into(),
            lang: Some("de".into()),
            name: "Deutsch".into(),
            is_default: false,
        });
        let mut original = stream(StreamKind::Audio, 251, "audio/webm", "opus", 160_000);
        original.id = "251-en.4".into();
        original.audio_track = Some(AudioTrackInfo {
            id: "en.4".into(),
            lang: Some("en".into()),
            name: "English original".into(),
            is_default: true,
        });
        let p = playback(vec![dubbed, original]);
        let mpd = manifest(&p, AudioFormat::Aac, |s| s.url.clone());

        assert!(mpd.contains("?a=1&amp;b=2"));
        assert!(mpd.find(r#"lang="en""#) < mpd.find(r#"lang="de""#));
        assert!(mpd.contains(r#"value="main""#) && mpd.contains(r#"value="alternate""#));
        assert!(mpd.contains(r#"<Representation id="251-de.4""#));
    }

    #[test]
    fn original_audio_wins_and_one_container_is_used() {
        let track = |s: &mut Stream, lang: &str, default: bool| {
            s.id = format!("{}-{lang}", s.itag);
            s.audio_track = Some(AudioTrackInfo {
                id: lang.into(),
                lang: Some(lang.into()),
                name: lang.into(),
                is_default: default,
            });
        };
        let mut streams = Vec::new();
        for (lang, default, aac_rate) in [("pt-BR", false, 140_000), ("en-US", true, 128_000)] {
            let mut aac = stream(StreamKind::Audio, 140, "audio/mp4", "mp4a.40.2", aac_rate);
            track(&mut aac, lang, default);
            let mut opus = stream(StreamKind::Audio, 251, "audio/webm", "opus", 160_000);
            track(&mut opus, lang, default);
            streams.extend([aac, opus]);
        }
        let mpd = manifest(&playback(streams), AudioFormat::Aac, |s| s.id.clone());

        // AAC only, one adaptation set per language.
        assert!(!mpd.contains("audio/webm"));
        assert_eq!(mpd.matches(r#"contentType="audio""#).count(), 2);
        // The original track is preferred even though the dub has a higher bitrate.
        assert!(mpd.contains(r#"lang="en-US" selectionPriority="2""#));
        assert!(mpd.contains(r#"lang="pt-BR" selectionPriority="1""#));
        assert!(mpd.find(r#"lang="en-US""#) < mpd.find(r#"lang="pt-BR""#));
    }

    #[test]
    fn falls_back_to_webm_audio_without_aac() {
        let p = playback(vec![
            stream(StreamKind::Video, 248, "video/webm", "vp9", 3_000_000),
            stream(StreamKind::Audio, 251, "audio/webm", "opus", 160_000),
        ]);
        assert!(
            manifest(&p, AudioFormat::Aac, |s| s.id.clone()).contains(r#"mimeType="audio/webm""#)
        );
    }

    #[test]
    fn opus_when_asked_for() {
        let p = playback(vec![
            stream(StreamKind::Video, 248, "video/webm", "vp9", 3_000_000),
            stream(StreamKind::Audio, 140, "audio/mp4", "mp4a.40.2", 128_000),
            stream(StreamKind::Audio, 251, "audio/webm", "opus", 160_000),
        ]);
        let mpd = manifest(&p, AudioFormat::Opus, |s| s.id.clone());
        assert!(mpd.contains(r#"mimeType="audio/webm""#));
        assert!(!mpd.contains(r#"mimeType="audio/mp4""#));
        // And AAC when Opus isn't available.
        let aac_only = playback(vec![stream(
            StreamKind::Audio,
            140,
            "audio/mp4",
            "mp4a.40.2",
            1,
        )]);
        assert!(manifest(&aac_only, AudioFormat::Opus, |s| s.id.clone()).contains("audio/mp4"));
    }
}
