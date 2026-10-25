-- Video/channel metadata (kept for anything a user interacts with, so lists render
-- without asking YouTube) and per-user watch progress.

CREATE TABLE channels (
    id               TEXT PRIMARY KEY,           -- UC…
    name             TEXT NOT NULL,
    handle           TEXT,
    avatar_url       TEXT,                       -- upstream URL; the API proxies it
    subscriber_count INTEGER,
    updated_at       INTEGER NOT NULL
);

CREATE TABLE videos (
    id            TEXT PRIMARY KEY,
    channel_id    TEXT NOT NULL REFERENCES channels(id),
    title         TEXT NOT NULL,
    thumbnail_url TEXT,
    duration_s    INTEGER,
    published_at  INTEGER,
    is_short      INTEGER NOT NULL DEFAULT 0,
    is_live       INTEGER NOT NULL DEFAULT 0,
    updated_at    INTEGER NOT NULL
);
CREATE INDEX videos_channel ON videos(channel_id, published_at DESC);

CREATE TABLE watch_progress (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    video_id   TEXT    NOT NULL REFERENCES videos(id),
    position_s REAL    NOT NULL,
    duration_s REAL    NOT NULL,
    finished   INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, video_id)
);
CREATE INDEX watch_progress_recent ON watch_progress(user_id, updated_at DESC);
