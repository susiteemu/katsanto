-- Live streams and premieres. `is_live` means "upcoming or live now"; once a stream
-- has ended it's a regular video. `live_at` is the scheduled start while upcoming,
-- then the actual start; `live_ended_at` is set once it has ended.
ALTER TABLE videos ADD COLUMN is_upcoming   INTEGER NOT NULL DEFAULT 0;
ALTER TABLE videos ADD COLUMN live_at       INTEGER;
ALTER TABLE videos ADD COLUMN live_ended_at INTEGER;
