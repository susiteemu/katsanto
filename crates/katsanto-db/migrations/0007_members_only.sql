-- Videos only paying channel members can watch, such as "Members first" uploads,
-- are left out of feeds. When one becomes public, `went_public_at` records when, and
-- that time replaces its publish time so it shows up in feeds as new.
ALTER TABLE videos ADD COLUMN members_only   INTEGER NOT NULL DEFAULT 0;
ALTER TABLE videos ADD COLUMN went_public_at INTEGER;

-- Look recent videos up on their channel's page again, to find the members-only ones
-- already in feeds.
UPDATE videos SET enriched = 0 WHERE published_at > unixepoch() - 14 * 24 * 3600;
