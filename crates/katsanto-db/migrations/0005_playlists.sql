-- Watch lists: a built-in "Watch later" per user plus custom lists.

CREATE TABLE playlists (
    id             INTEGER PRIMARY KEY,
    user_id        INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind           TEXT    NOT NULL CHECK (kind IN ('watch_later', 'custom')),
    name           TEXT    NOT NULL,
    description    TEXT    NOT NULL DEFAULT '',
    -- Drop videos from the list once they've been watched to the end.
    remove_watched INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);
CREATE INDEX playlists_user ON playlists(user_id);
CREATE UNIQUE INDEX playlists_one_watch_later ON playlists(user_id) WHERE kind = 'watch_later';

-- `position` is fractional so moving an item never renumbers the others.
CREATE TABLE playlist_items (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    video_id    TEXT    NOT NULL REFERENCES videos(id),
    position    REAL    NOT NULL,
    added_at    INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, video_id)
);
CREATE INDEX playlist_items_order ON playlist_items(playlist_id, position);
CREATE INDEX playlist_items_video ON playlist_items(video_id);
