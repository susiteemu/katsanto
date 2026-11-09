//! Start and end times of live streams and premieres, from a `player` response.
//! Its microformat has them even when playback is refused, so the request needs no
//! PO token.

use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::model::LiveTimes;

/// `None` for videos that aren't live streams or premieres.
pub(crate) fn parse_live_times(player: &Value) -> Option<LiveTimes> {
    let details = player.pointer("/microformat/playerMicroformatRenderer/liveBroadcastDetails")?;
    let at = |key: &str| {
        details
            .get(key)
            .and_then(Value::as_str)
            .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
            .map(OffsetDateTime::unix_timestamp)
    };
    Some(LiveTimes {
        upcoming: player
            .pointer("/videoDetails/isUpcoming")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        starts_at: at("startTimestamp"),
        ended_at: at("endTimestamp"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn player(upcoming: Option<bool>, details: Value) -> Value {
        json!({
            "videoDetails": { "isUpcoming": upcoming },
            "microformat": { "playerMicroformatRenderer": { "liveBroadcastDetails": details } }
        })
    }

    #[test]
    fn upcoming_live_and_ended() {
        assert_eq!(
            parse_live_times(&player(
                Some(true),
                json!({ "isLiveNow": false, "startTimestamp": "2026-11-08T15:00:00+00:00" })
            )),
            Some(LiveTimes {
                upcoming: true,
                starts_at: Some(1_794_150_000),
                ended_at: None
            })
        );
        assert_eq!(
            parse_live_times(&player(
                None,
                json!({ "isLiveNow": true, "startTimestamp": "2026-10-22T14:57:23+00:00" })
            )),
            Some(LiveTimes {
                upcoming: false,
                starts_at: Some(1_792_681_043),
                ended_at: None
            })
        );
        let ended = parse_live_times(&player(
            None,
            json!({
                "isLiveNow": false,
                "startTimestamp": "2026-11-07T21:59:18+00:00",
                "endTimestamp": "2026-11-07T23:35:11+00:00"
            }),
        ))
        .unwrap();
        assert_eq!(ended.ended_at.unwrap() - ended.starts_at.unwrap(), 5753);
    }

    #[test]
    fn regular_videos_have_none() {
        assert_eq!(parse_live_times(&json!({ "videoDetails": {} })), None);
    }
}
