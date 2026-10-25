-- Tags on channels, saved feeds (filters over the subscription feed), hidden videos.

CREATE TABLE tags (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    color      TEXT    NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX tags_user_name ON tags(user_id, name COLLATE NOCASE);

-- Kept when unsubscribing, so re-subscribing restores a channel's tags.
CREATE TABLE channel_tags (
    tag_id     INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    channel_id TEXT    NOT NULL REFERENCES channels(id),
    PRIMARY KEY (tag_id, channel_id)
);
CREATE INDEX channel_tags_channel ON channel_tags(channel_id);

CREATE TABLE feeds (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    icon       TEXT    NOT NULL DEFAULT '',
    position   INTEGER NOT NULL,
    filter     TEXT    NOT NULL,            -- JSON, see the server's FeedFilter
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX feeds_user ON feeds(user_id, position);

CREATE TABLE hidden_videos (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    video_id   TEXT    NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, video_id)
);
