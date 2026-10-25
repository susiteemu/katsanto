//! Per-user backup: everything a user built in Katsanto as one JSON document, for
//! moving between instances. Restoring merges into the account instead of replacing
//! it, so importing the same file twice changes nothing.
//!
//! Channel and video metadata travel with the backup, so a restore never has to ask
//! YouTube about anything (the background refresher fills in fresh data later).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{Conn, DbResult, now, playlists, progress};

pub const FORMAT: &str = "katsanto-backup";
pub const VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Backup {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub exported_at: i64,
    /// The user's settings object (appearance, language, …).
    #[serde(default)]
    pub settings: serde_json::Value,
    #[serde(default)]
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub videos: Vec<Video>,
    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub feeds: Vec<Feed>,
    #[serde(default)]
    pub lists: Vec<List>,
    #[serde(default)]
    pub progress: Vec<Progress>,
    #[serde(default)]
    pub hidden: Vec<Hidden>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub handle: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Video {
    pub id: String,
    pub channel_id: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub duration_s: Option<i64>,
    pub published_at: Option<i64>,
    #[serde(default)]
    pub is_short: bool,
    #[serde(default)]
    pub is_live: bool,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Subscription {
    pub channel_id: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Tag {
    /// Only meaningful inside the backup: feed filters refer to tags by this id.
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub channel_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Feed {
    pub name: String,
    #[serde(default)]
    pub icon: String,
    pub filter: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct List {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub remove_watched: bool,
    #[serde(default)]
    pub items: Vec<ListItem>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ListItem {
    pub video_id: String,
    pub added_at: i64,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Progress {
    pub video_id: String,
    pub position_s: f64,
    pub duration_s: f64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Hidden {
    pub video_id: String,
    pub created_at: i64,
}

/// What a restore added.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct ImportStats {
    pub subscriptions: u64,
    pub tags: u64,
    pub feeds: u64,
    pub lists: u64,
    pub list_items: u64,
    pub progress: u64,
    pub hidden: u64,
}

pub async fn export(conn: &mut Conn, user_id: i64) -> DbResult<Backup> {
    let settings: String = sqlx::query_scalar("SELECT settings FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&mut *conn)
        .await?;

    let subscriptions: Vec<Subscription> = sqlx::query_as(
        "SELECT channel_id, created_at FROM subscriptions WHERE user_id = ? ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;

    let mut tags = Vec::new();
    let tag_rows: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id, name, color FROM tags WHERE user_id = ? ORDER BY id")
            .bind(user_id)
            .fetch_all(&mut *conn)
            .await?;
    for (id, name, color) in tag_rows {
        let channel_ids = sqlx::query_scalar(
            "SELECT channel_id FROM channel_tags WHERE tag_id = ? ORDER BY channel_id",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
        tags.push(Tag {
            id,
            name,
            color,
            channel_ids,
        });
    }

    let feed_rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT name, icon, filter FROM feeds WHERE user_id = ? ORDER BY position, id",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;
    let feeds = feed_rows
        .into_iter()
        .map(|(name, icon, filter)| Feed {
            name,
            icon,
            filter: serde_json::from_str(&filter).unwrap_or_default(),
        })
        .collect();

    let mut lists = Vec::new();
    let list_rows: Vec<(i64, String, String, String, bool)> = sqlx::query_as(
        "SELECT id, kind, name, description, remove_watched FROM playlists WHERE user_id = ?
         ORDER BY kind <> 'watch_later', id",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;
    for (id, kind, name, description, remove_watched) in list_rows {
        let items = sqlx::query_as(
            "SELECT video_id, added_at FROM playlist_items WHERE playlist_id = ? ORDER BY position",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
        lists.push(List {
            kind,
            name,
            description,
            remove_watched,
            items,
        });
    }

    let progress = sqlx::query_as(
        "SELECT video_id, position_s, duration_s, updated_at FROM watch_progress
         WHERE user_id = ? ORDER BY updated_at",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;

    let hidden = sqlx::query_as(
        "SELECT video_id, created_at FROM hidden_videos WHERE user_id = ? ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;

    // Metadata for every video and channel referenced above.
    let videos: Vec<Video> = sqlx::query_as(
        "SELECT id, channel_id, title, thumbnail_url, duration_s, published_at, is_short, is_live
         FROM videos WHERE id IN (
           SELECT video_id FROM watch_progress WHERE user_id = ?1
           UNION SELECT i.video_id FROM playlist_items i JOIN playlists p ON p.id = i.playlist_id
                 WHERE p.user_id = ?1)
         ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut channel_ids: HashSet<&str> = subscriptions
        .iter()
        .map(|s| s.channel_id.as_str())
        .collect();
    channel_ids.extend(videos.iter().map(|v| v.channel_id.as_str()));
    channel_ids.extend(
        tags.iter()
            .flat_map(|t| t.channel_ids.iter().map(String::as_str)),
    );
    let mut channels = Vec::new();
    for id in channel_ids {
        if let Some(c) = sqlx::query_as::<_, Channel>(
            "SELECT id, name, handle, avatar_url FROM channels WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        {
            channels.push(c);
        }
    }
    channels.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(Backup {
        format: FORMAT.into(),
        version: VERSION,
        exported_at: now(),
        settings: serde_json::from_str(&settings).unwrap_or_default(),
        channels,
        videos,
        subscriptions,
        tags,
        feeds,
        lists,
        progress,
        hidden,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum InvalidBackup {
    #[error("not a Katsanto backup")]
    Format,
    #[error("this backup is from a newer Katsanto (format version {0})")]
    Version(u32),
    #[error("{0}")]
    Data(String),
}

/// Check the parts a restore can't simply skip. Unknown references (a list item whose
/// video isn't in the backup, …) are skipped during the import instead.
pub fn validate(b: &Backup) -> Result<(), InvalidBackup> {
    if b.format != FORMAT {
        return Err(InvalidBackup::Format);
    }
    if b.version > VERSION {
        return Err(InvalidBackup::Version(b.version));
    }
    let bad = |what: &str, id: &str| Err(InvalidBackup::Data(format!("invalid {what} id: {id:?}")));
    for c in &b.channels {
        if !is_channel_id(&c.id) {
            return bad("channel", &c.id);
        }
    }
    for v in &b.videos {
        if !is_video_id(&v.id) {
            return bad("video", &v.id);
        }
    }
    let too_long = b
        .tags
        .iter()
        .any(|t| t.name.trim().is_empty() || t.name.len() > 30)
        || b.feeds
            .iter()
            .any(|f| f.name.trim().is_empty() || f.name.len() > 40)
        || b.lists
            .iter()
            .any(|l| l.name.trim().is_empty() || l.name.len() > 60);
    if too_long {
        return Err(InvalidBackup::Data(
            "a tag, feed or list has an empty or too long name".into(),
        ));
    }
    Ok(())
}

fn is_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn is_channel_id(id: &str) -> bool {
    id.len() == 24
        && id.starts_with("UC")
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Merge a (validated) backup into the user's account. Run it in a transaction.
pub async fn import(conn: &mut Conn, user_id: i64, b: &Backup) -> DbResult<ImportStats> {
    let mut stats = ImportStats::default();

    // Metadata first: never overwrite what this instance already knows.
    let known_channels: HashSet<&str> = b.channels.iter().map(|c| c.id.as_str()).collect();
    for c in &b.channels {
        sqlx::query(
            "INSERT INTO channels (id, name, handle, avatar_url, updated_at) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(id) DO NOTHING",
        )
        .bind(&c.id)
        .bind(&c.name)
        .bind(&c.handle)
        .bind(&c.avatar_url)
        .bind(now())
        .execute(&mut *conn)
        .await?;
    }
    for v in b
        .videos
        .iter()
        .filter(|v| known_channels.contains(v.channel_id.as_str()))
    {
        sqlx::query(
            "INSERT INTO videos (id, channel_id, title, thumbnail_url, duration_s, published_at, is_short, is_live, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO NOTHING",
        )
        .bind(&v.id)
        .bind(&v.channel_id)
        .bind(&v.title)
        .bind(&v.thumbnail_url)
        .bind(v.duration_s)
        .bind(v.published_at)
        .bind(v.is_short)
        .bind(v.is_live)
        .bind(now())
        .execute(&mut *conn)
        .await?;
    }

    for s in &b.subscriptions {
        if !channel_exists(conn, &s.channel_id).await? {
            continue;
        }
        let res = sqlx::query(
            "INSERT INTO subscriptions (user_id, channel_id, created_at) VALUES (?, ?, ?)
             ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(&s.channel_id)
        .bind(s.created_at)
        .execute(&mut *conn)
        .await?;
        if res.rows_affected() == 1 {
            stats.subscriptions += 1;
            // Fetch the channel's videos soon.
            sqlx::query(
                "UPDATE channels SET next_fetch_at = 0 WHERE id = ? AND fetched_at IS NULL",
            )
            .bind(&s.channel_id)
            .execute(&mut *conn)
            .await?;
        }
    }

    // Tags merge by name; remember backup id → local id for the feeds.
    let mut tag_ids = HashMap::new();
    for t in &b.tags {
        let name = t.name.trim();
        let existing: Option<i64> =
            sqlx::query_scalar("SELECT id FROM tags WHERE user_id = ? AND name = ? COLLATE NOCASE")
                .bind(user_id)
                .bind(name)
                .fetch_optional(&mut *conn)
                .await?;
        let id = match existing {
            Some(id) => id,
            None => {
                stats.tags += 1;
                sqlx::query_scalar(
                    "INSERT INTO tags (user_id, name, color, created_at) VALUES (?, ?, ?, ?) RETURNING id",
                )
                .bind(user_id)
                .bind(name)
                .bind(&t.color)
                .bind(now())
                .fetch_one(&mut *conn)
                .await?
            }
        };
        tag_ids.insert(t.id, id);
        for channel in &t.channel_ids {
            if channel_exists(conn, channel).await? {
                sqlx::query(
                    "INSERT INTO channel_tags (tag_id, channel_id) VALUES (?, ?) ON CONFLICT DO NOTHING",
                )
                .bind(id)
                .bind(channel)
                .execute(&mut *conn)
                .await?;
            }
        }
    }

    // Feeds: skip ones that already exist with the same name.
    for f in &b.feeds {
        let name = f.name.trim();
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM feeds WHERE user_id = ? AND name = ? COLLATE NOCASE)",
        )
        .bind(user_id)
        .bind(name)
        .fetch_one(&mut *conn)
        .await?;
        if exists {
            continue;
        }
        let mut filter = f.filter.clone();
        if let Some(ids) = filter.get_mut("tag_ids").and_then(|v| v.as_array_mut()) {
            *ids = ids
                .iter()
                .filter_map(|v| v.as_i64().and_then(|old| tag_ids.get(&old)))
                .map(|&id| id.into())
                .collect();
        }
        sqlx::query(
            "INSERT INTO feeds (user_id, name, icon, position, filter, created_at, updated_at)
             VALUES (?, ?, ?, (SELECT COALESCE(MAX(position), -1) + 1 FROM feeds WHERE user_id = ?), ?, ?, ?)",
        )
        .bind(user_id)
        .bind(name)
        .bind(&f.icon)
        .bind(user_id)
        .bind(filter.to_string())
        .bind(now())
        .bind(now())
        .execute(&mut *conn)
        .await?;
        stats.feeds += 1;
    }

    // Lists merge by name (Watch later by kind); items are appended in order.
    for l in &b.lists {
        let list_id = if l.kind == playlists::WATCH_LATER {
            playlists::ensure_watch_later(conn, user_id).await?
        } else {
            let name = l.name.trim();
            let existing: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM playlists WHERE user_id = ? AND kind = 'custom' AND name = ? COLLATE NOCASE",
            )
            .bind(user_id)
            .bind(name)
            .fetch_optional(&mut *conn)
            .await?;
            match existing {
                Some(id) => id,
                None => {
                    stats.lists += 1;
                    sqlx::query_scalar(
                        "INSERT INTO playlists (user_id, kind, name, description, remove_watched, created_at, updated_at)
                         VALUES (?, 'custom', ?, ?, ?, ?, ?) RETURNING id",
                    )
                    .bind(user_id)
                    .bind(name)
                    .bind(&l.description)
                    .bind(l.remove_watched)
                    .bind(now())
                    .bind(now())
                    .fetch_one(&mut *conn)
                    .await?
                }
            }
        };
        for item in &l.items {
            if !video_exists(conn, &item.video_id).await? {
                continue;
            }
            let res = sqlx::query(
                "INSERT INTO playlist_items (playlist_id, video_id, position, added_at)
                 VALUES (?, ?, (SELECT COALESCE(MAX(position), 0) + 1 FROM playlist_items WHERE playlist_id = ?), ?)
                 ON CONFLICT DO NOTHING",
            )
            .bind(list_id)
            .bind(&item.video_id)
            .bind(list_id)
            .bind(item.added_at)
            .execute(&mut *conn)
            .await?;
            stats.list_items += res.rows_affected();
        }
    }

    // Progress: the newer of the two wins.
    for p in &b.progress {
        if !video_exists(conn, &p.video_id).await?
            || !p.position_s.is_finite()
            || !p.duration_s.is_finite()
        {
            continue;
        }
        let res = sqlx::query(
            "INSERT INTO watch_progress (user_id, video_id, position_s, duration_s, finished, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(user_id, video_id) DO UPDATE SET
               position_s = excluded.position_s, duration_s = excluded.duration_s,
               finished = excluded.finished, updated_at = excluded.updated_at
             WHERE excluded.updated_at > watch_progress.updated_at",
        )
        .bind(user_id)
        .bind(&p.video_id)
        .bind(p.position_s.max(0.0))
        .bind(p.duration_s.max(0.0))
        .bind(progress::is_finished(p.position_s, p.duration_s))
        .bind(p.updated_at)
        .execute(&mut *conn)
        .await?;
        stats.progress += res.rows_affected();
    }

    for h in &b.hidden {
        if !is_video_id(&h.video_id) {
            continue;
        }
        let res = sqlx::query(
            "INSERT INTO hidden_videos (user_id, video_id, created_at) VALUES (?, ?, ?)
             ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(&h.video_id)
        .bind(h.created_at)
        .execute(&mut *conn)
        .await?;
        stats.hidden += res.rows_affected();
    }

    // Settings: keys from the backup replace the current ones.
    if let Some(imported) = b.settings.as_object().filter(|o| !o.is_empty()) {
        let current: String = sqlx::query_scalar("SELECT settings FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&mut *conn)
            .await?;
        let mut merged: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&current).unwrap_or_default();
        merged.extend(imported.clone());
        sqlx::query("UPDATE users SET settings = ? WHERE id = ?")
            .bind(serde_json::Value::Object(merged).to_string())
            .bind(user_id)
            .execute(&mut *conn)
            .await?;
    }

    Ok(stats)
}

async fn channel_exists(conn: &mut Conn, id: &str) -> DbResult<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channels WHERE id = ?)")
            .bind(id)
            .fetch_one(&mut *conn)
            .await?,
    )
}

async fn video_exists(conn: &mut Conn, id: &str) -> DbResult<bool> {
    crate::videos::exists(&mut *conn, id).await
}
