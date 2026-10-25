-- WebSub (PubSubHubbub) push subscriptions at YouTube's hub, one per channel.

CREATE TABLE websub (
    channel_id       TEXT    PRIMARY KEY REFERENCES channels(id),
    -- HMAC key the hub signs notifications with; kept across renewals.
    secret           TEXT    NOT NULL,
    requested_at     INTEGER NOT NULL,
    -- Set once the hub has verified the subscription.
    lease_expires_at INTEGER
);
