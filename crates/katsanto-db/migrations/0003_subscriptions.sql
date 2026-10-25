-- Subscriptions and the background refresh of subscribed channels.

CREATE TABLE subscriptions (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    channel_id TEXT    NOT NULL REFERENCES channels(id),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, channel_id)
);
CREATE INDEX subscriptions_channel ON subscriptions(channel_id);

-- Refresh state, shared by everyone subscribed to the channel.
ALTER TABLE channels ADD COLUMN last_video_at INTEGER;              -- newest upload seen
ALTER TABLE channels ADD COLUMN fetched_at    INTEGER;
ALTER TABLE channels ADD COLUMN next_fetch_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE channels ADD COLUMN fetch_errors  INTEGER NOT NULL DEFAULT 0;  -- consecutive
ALTER TABLE channels ADD COLUMN fetch_error   TEXT;
CREATE INDEX channels_due ON channels(next_fetch_at);

-- RSS gives exact publish times but no duration or video type; `enriched` marks
-- videos whose duration/short/live flags came from the channel page.
ALTER TABLE videos ADD COLUMN view_count INTEGER;
ALTER TABLE videos ADD COLUMN enriched   INTEGER NOT NULL DEFAULT 0;
CREATE INDEX videos_published ON videos(published_at DESC, id DESC);
