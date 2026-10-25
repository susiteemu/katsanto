-- Accounts, credentials and sessions. Timestamps are unix seconds.

CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    -- Stable WebAuthn user handle (UUID bytes); never changes, unlike the username.
    webauthn_id   BLOB    NOT NULL UNIQUE,
    password_hash TEXT,
    is_admin      INTEGER NOT NULL DEFAULT 0,
    disabled      INTEGER NOT NULL DEFAULT 0,
    settings      TEXT    NOT NULL DEFAULT '{}',
    created_at    INTEGER NOT NULL,
    last_login_at INTEGER
);

CREATE TABLE passkeys (
    id            INTEGER PRIMARY KEY,
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credential_id BLOB    NOT NULL UNIQUE,
    -- webauthn-rs `Passkey`, serialized as JSON
    passkey       TEXT    NOT NULL,
    name          TEXT    NOT NULL,
    created_at    INTEGER NOT NULL,
    last_used_at  INTEGER
);
CREATE INDEX passkeys_user ON passkeys(user_id);

CREATE TABLE recovery_codes (
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash BLOB    NOT NULL,
    used_at   INTEGER,
    PRIMARY KEY (user_id, code_hash)
);

CREATE TABLE invites (
    id         INTEGER PRIMARY KEY,
    token_hash BLOB    NOT NULL UNIQUE,
    note       TEXT    NOT NULL DEFAULT '',
    created_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    used_by    INTEGER REFERENCES users(id) ON DELETE SET NULL,
    used_at    INTEGER
);

CREATE TABLE reset_links (
    id         INTEGER PRIMARY KEY,
    token_hash BLOB    NOT NULL UNIQUE,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at    INTEGER
);

CREATE TABLE sessions (
    id           INTEGER PRIMARY KEY,
    token_hash   BLOB    NOT NULL UNIQUE,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_agent   TEXT    NOT NULL DEFAULT '',
    created_at   INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    -- Last time the user re-authenticated, for sensitive actions.
    reauth_at    INTEGER
);
CREATE INDEX sessions_user ON sessions(user_id);

-- Short-lived WebAuthn ceremony state (registration / authentication).
CREATE TABLE auth_challenges (
    id         TEXT    PRIMARY KEY,
    kind       TEXT    NOT NULL,
    state      TEXT    NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE TABLE instance_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
