# Katsanto — Implementation Plan

A lightweight, self-hostable, privacy-friendly YouTube front-end written in Rust, inspired by
[Invidious](https://invidious.io/), with a modern, themeable frontend.

---

## 1. What we learn from Invidious

| Aspect | Invidious | Katsanto |
|---|---|---|
| Language | Crystal (Kemal web framework) | Rust (axum + tokio) |
| YouTube access | Own Innertube extractor + **Invidious Companion** (Deno, YouTube.js) for streams / attestation | [`rustypipe`](https://codeberg.org/ThetaDev/rustypipe) + `rustypipe-botguard` for PO tokens |
| Database | PostgreSQL (required) | SQLite by default (single file, zero ops); Postgres optional later |
| Frontend | Server-rendered templates, Video.js player | SPA (SvelteKit), design-token theming, Vidstack player |
| Feed refresh | Background jobs polling channels + optional PubSubHubbub | Same idea: RSS polling worker + optional WebSub |
| Streams | Direct from googlevideo or proxied through instance (`local=true`) | **Proxied by default**, direct optional |
| Accounts | Local accounts, import from YouTube/NewPipe/FreeTube | Local accounts only, **no link to YouTube/Google accounts** |
| Deployment | 3 services (Invidious + Postgres + Companion) | **One binary** (frontend embedded) + `rustypipe-botguard` helper |

Things Invidious does that we deliberately keep: no Google account required, documented REST API,
video proxying, SponsorBlock.

Things we change: a single lightweight binary, a client-side app with a real design system,
first-class feed filtering, resume-where-you-left-off as a core feature, and no YouTube account
import/sync at all — subscriptions and watch lists live only in Katsanto and are built via search.

---

## 2. Key technology choices

### Backend (Rust)
- **HTTP:** `axum` + `tokio` + `tower-http` (compression, CORS, tracing, static files).
- **YouTube:** `rustypipe` (features: `rss`, `rustls-tls-*`; **no `userdata`** — all requests are anonymous).
  - `search()` (with filters) + search suggestions → search; `player()` → stream URLs, subtitles;
    `channel_videos()` / `channel_rss()` → feeds; URL resolver for pasted links.
  - PO tokens: ship `rustypipe-botguard` next to the binary (rustypipe auto-detects it).
  - rustypipe cache (client versions, deobfuscation code) → custom `CacheStorage` backed by our DB/data dir.
- **DB:** `sqlx` with SQLite (WAL mode), compile-time checked queries, migrations in `migrations/`.
- **Caching:** `moka` in-memory cache for video metadata, player responses (stream URLs expire ~6 h), channel info.
- **Background jobs:** tokio tasks + a small job table in DB (no external queue).
- **Auth:** local Katsanto accounts — **passkeys first** (`webauthn-rs`, discoverable credentials /
  conditional UI), **password fallback** (`argon2id`), `tower-sessions` (sessions in SQLite,
  `HttpOnly` + `Secure` + `SameSite=Lax` cookies), login rate limiting (`tower_governor`). See §5.0.
- **API docs:** `utoipa` → OpenAPI spec → generated TypeScript client for the frontend.
- **Config:** `figment` (TOML file + env vars).
- **Frontend embedding:** `rust-embed` so the release is one binary (built from `web/` with `adapter-static`).
- **License:** rustypipe is **GPL-3.0** → Katsanto must be GPL-3.0-compatible. Recommend **AGPL-3.0** (same as Invidious; fits a network service).

### Frontend
- **Framework:** **SvelteKit** (Svelte 5 runes) with `adapter-static` (SPA mode) + TypeScript. Small bundles, fast on mobile.
- **Styling:** Tailwind CSS v4 driven entirely by **CSS custom properties (design tokens)**.
- **Components:** headless primitives (Bits UI / Melt UI) styled by our tokens → accessible & themeable.
- **Player:** [Vidstack](https://vidstack.io/) (now continuing as Video.js v10) — modern, touch-friendly, PiP,
  keyboard shortcuts, chapters, captions, playback speed. DASH via `dash.js` (or Shaka Player) provider.
- **Passkeys:** `@simplewebauthn/browser` (JSON ↔ WebAuthn conversion, feature detection, autofill UI).
- **Data:** TanStack Query (Svelte) for caching/invalidations; generated API client.
- **PWA:** installable, offline shell, Media Session API (lock screen controls), background audio.
- **Lists:** virtualized infinite scroll for feeds.

---

## 3. Architecture

```
┌──────────────────────── katsanto (single binary) ────────────────────────┐
│                                                                          │
│  SPA (embedded)  ◄──  axum router                                        │
│                        ├── /api/v1/*        REST JSON (OpenAPI)          │
│                        ├── /proxy/video/*   range-request stream proxy   │
│                        ├── /proxy/img/*     thumbnail / avatar proxy     │
│                        └── /api/v1/manifest/{id}.mpd  generated DASH     │
│                                                                          │
│  services: yt (rustypipe adapter) │ search │ feeds │ subscriptions │   │
│            progress │ playlists │ users/auth │ backup                    │
│                                                                          │
│  workers:  feed refresher │ metadata enricher │ cache/GC │ WebSub (opt)  │
│                                                                          │
│  storage:  SQLite (sqlx)  +  moka in-memory cache                        │
└────────────────────────────────┬─────────────────────────────────────────┘
                                 │ Innertube / RSS / googlevideo
                     rustypipe-botguard (PO tokens, subprocess)
```

### Cargo workspace layout

```
katsanto/
├── Cargo.toml                 # workspace
├── crates/
│   ├── katsanto-server/       # axum app, routes, auth, config, main.rs
│   ├── katsanto-core/         # domain models, services, feed filter engine
│   ├── katsanto-db/           # sqlx queries + migrations
│   └── katsanto-yt/           # trait `YoutubeSource` + rustypipe impl (+ mock for tests)
├── web/                       # SvelteKit app
├── migrations/
├── docker/                    # Dockerfile (multi-stage, distroless/alpine), compose example
└── PLAN.md
```

`katsanto-yt` hides rustypipe behind a trait so YouTube breakage is isolated to one crate and
tests can run against fixtures without the network.

---

## 4. Data model (initial)

```sql
users(id, username UNIQUE, webauthn_user_id UUID UNIQUE, password_hash NULL, is_admin, disabled,
      settings_json, created_at, last_login_at)
passkeys(id, user_id, credential_id UNIQUE, passkey_json /* webauthn-rs Passkey */, name,
         created_at, last_used_at)
recovery_codes(user_id, code_hash, used_at NULL)
invites(token_hash PK, created_by, expires_at, used_by NULL)
reset_links(token_hash PK, user_id, created_by, expires_at, used_at NULL)

channels(id TEXT PK /* UC… */, name, handle, avatar_url, banner_url,
         subscriber_count, last_fetched_at, next_fetch_at, fetch_error_count)

videos(id TEXT PK, channel_id FK, title, description, published_at,
       duration_s NULL, view_count, is_short, is_live, is_upcoming,
       thumbnail_url, fetched_at)
  INDEX (channel_id, published_at DESC)

subscriptions(user_id, channel_id, created_at, PK(user_id, channel_id))
search_history(user_id, query, searched_at)       -- optional, per-user setting

tags(id, user_id, name, color)                    -- "Programming", "Entertainment" …
channel_tags(user_id, channel_id, tag_id)         -- a channel can have many tags

feeds(id, user_id, name, icon, position, filter_json, sort, is_default)

watch_progress(user_id, video_id, position_s, duration_s,
               finished BOOL, updated_at, PK(user_id, video_id))

playlists(id, user_id, name, description, kind /* 'watch_later'|'custom' */,
          created_at, updated_at)
playlist_items(playlist_id, video_id, position, added_at, note)

hidden_videos(user_id, video_id)                  -- "not interested"
sessions(...)                                     -- tower-sessions store
jobs(id, kind, payload, run_at, attempts, last_error)
```

---

## 5. Feature design

### 5.0 Accounts
Katsanto has its **own user accounts with no link to YouTube or Google**. All YouTube traffic from the
server is anonymous (rustypipe without authentication); a user's subscriptions, watch lists, feeds and
progress exist only in Katsanto's database.

- Username only, **no email required**. Credentials: one or more **passkeys** and/or a **password**.

#### Authentication: passkeys first, password fallback
- **Passkeys (primary):** WebAuthn via `webauthn-rs`. Phishing-resistant, nothing to remember, works with
  Face ID / Touch ID / Android / Windows Hello / security keys; synced passkeys (iCloud Keychain, Google
  Password Manager, 1Password, Bitwarden …) work on all the user's devices.
  - **Signup** (from invite link): choose username → create passkey. "Use a password instead" link below.
  - **Login:** username-less — the login page offers passkeys via browser autofill (conditional UI) and a
    "Sign in with passkey" button; username + password form shown as the fallback option.
  - Users can add several passkeys (phone, laptop, security key), name, and remove them in settings.
  - Challenge state kept server-side in the session (5 min TTL); signature counter / backup flags updated
    after each login.
- **Password (fallback):** argon2id. Used when the device/browser has no passkey support, or when the
  instance cannot use passkeys (see below). Users with a passkey can still add a password, and vice versa.
  Min length 12, checked against a small common-password list; no composition rules.
- **Instance setting** `auth.passwords = "enabled" | "disabled"` (default `enabled`). Admin can disable passwords
  once everyone has a passkey.
- **Passkeys need a secure context and a domain name:** `public_url` must be `https://` with a hostname
  (not a bare IP), or `http://localhost` for development. RP ID = hostname of `public_url`.
  If that's not met (e.g. `http://192.168.1.10:3000`), Katsanto automatically runs **password-only**
  and shows a warning in the admin UI explaining how to enable passkeys (domain + TLS, e.g. reverse proxy
  with Let's Encrypt or Tailscale HTTPS). Changing the domain later invalidates existing passkeys →
  documented; passwords/recovery codes still work.
- **Account must always keep ≥1 credential** — removing the last passkey without a password is refused.
- **Recovery:**
  - 10 single-use **recovery codes** shown once at signup (downloadable), stored hashed; can be regenerated.
    Logging in with one prompts the user to add a new passkey/password.
  - **Admin reset link:** single-use, 24 h link that lets the user register a new passkey or set a password
    (no admin ever sees or sets the user's password).
- **Re-authentication** (passkey or password) required for sensitive actions: removing credentials,
  regenerating recovery codes, deleting the account.
- **Sessions:** 30-day sliding expiry; "Sessions" list in settings (device, last seen) with revoke /
  "sign out everywhere".
- **First user to register becomes admin.** Registration mode in config/admin UI: `invite` (**default on a
  fresh install** — admin generates single-use, expiring invite links), `open`, or `closed` (admin creates users).
  On a fresh install the registration page is open only until the first (admin) account exists.
- Admin UI: list users, create/disable/delete users, generate reset links, instance settings (registration
  mode, passwords on/off, proxy on/off, concurrency limits).
- Users: manage passkeys and password, recovery codes, sessions, delete account (removes all their data), **backup export/import** of their
  Katsanto data as JSON (subscriptions, tags, feeds, watch lists, progress, settings) — for moving between
  instances, not a YouTube import.
- **No guest access:** every page and API endpoint (search, playback, proxy, thumbnails) requires a session;
  unauthenticated requests get `401` / redirect to `/login`. Only `/login`, `/register?invite=…`,
  `/reset?token=…`, the auth API endpoints and static assets are public. This also keeps the stream proxy from being used as an open relay.
- CSRF: `SameSite=Lax` cookies + required custom header (`X-Katsanto: 1`) on state-changing API calls.

### 5.1 Search (new)
Search is the main way to discover content, build subscriptions and fill watch lists.

- **Search box** always visible in the header (`/` keyboard shortcut); on mobile an expandable header
  search. Debounced **suggestions** as you type (rustypipe search suggestions), plus the user's recent
  searches (optional, can be disabled/cleared).
- **Results page** `/search?q=…` with tabs/filters: *Videos* (default), *Channels*;
  filters for upload date (hour/today/week/month/year), duration (<4 min, 4–20 min, >20 min),
  sort (relevance/date/views), and "hide shorts" / "hide live".
- **Pasting a YouTube URL** (video, channel, `@handle`, shorts) into search resolves directly via the
  rustypipe URL resolver and opens the video/channel page in Katsanto.
- **Video result card** shows thumbnail, duration, title, channel (avatar + name), views, age, and the
  user's own watch progress if any. Actions:
  - **Play** — click card → `/watch/{id}` (resumes from saved position).
  - **Add to watch list** — quick "+ Watch later" button and a menu with all lists / "+ New list".
  - **Subscribe to channel** — button/menu item on the card; toggles to "Subscribed" (and offers
    *unsubscribe* / *set tags*). State comes from the user's subscription set, so it's correct everywhere.
- **Channel result card**: avatar, name, handle, subscriber count, **Subscribe** button, link to the
  channel page.
- Infinite scroll using rustypipe's continuation tokens (opaque `cursor` passed back to the API).
- **Backend:** `GET /api/v1/search` maps rustypipe results into the same `VideoCard`/`ChannelCard` DTOs used by
  feeds, enriched with per-user state (subscribed, in which lists, progress) in one batched DB query.
  Results cached in `moka` for ~10 min keyed by `(query, filters, cursor)` — shared between users.
  Videos/channels are only written to the DB when the user acts on them (subscribe, add to list, watch).
- Channel page reuses the same cards/actions (videos/shorts/live tabs, subscribe button).

### 5.2 Subscriptions
- Created only in Katsanto: from search results, video page, channel page, or by pasting a channel URL /
  `@handle`. No import from YouTube accounts and no sync.
- Unsubscribe from the same places or the **Manage subscriptions** page (search, sort by name/last upload,
  bulk select, bulk tag, bulk unsubscribe).
- Assign tags (e.g. *Programming*, *Entertainment*) right when subscribing — tags drive the feeds (§5.4).
- On subscribe: fetch the channel immediately (RSS + latest videos) so the feed is populated right away.

### 5.3 Feed refresher (background worker)
- Per channel, poll `channel_rss()` (cheap, latest ~15 videos) on a schedule.
- Adaptive interval: active channels (uploaded recently) every 15–30 min, dormant channels every few hours;
  jitter + global concurrency limit (e.g. 4) + exponential backoff on errors.
- Only channels with ≥1 subscriber are polled; channel data is shared across users.
- **Enricher:** RSS lacks duration / shorts / live flags → periodically call `channel_videos()`
  (and shorts/live tabs) for channels with new items to fill `duration_s`, `is_short`, `is_live`.
- Optional **WebSub (PubSubHubbub)** push for instances reachable from the internet.
- Push "new videos" to open clients via SSE (`/api/v1/events`) so feeds update live.

### 5.4 Multiple feeds & filtering
A **feed** is a saved filter over the user's subscription videos. Stored as JSON, compiled to SQL:

```json
{
  "channels": { "tags_any": ["programming"], "include": [], "exclude": ["UCxyz"] },
  "title":    { "include_any": ["rust", "linux"], "exclude_any": ["#shorts"], "regex": null },
  "duration": { "min_s": 300, "max_s": null },
  "types":    { "shorts": false, "live": false, "upcoming": false, "videos": true },
  "watched":  "hide_finished",      // all | hide_finished | only_unwatched | in_progress
  "published_within_days": 30
}
```

- Default feeds: **All**, **Continue watching**; user creates e.g. *Programming*, *Entertainment*.
- Feeds appear as tabs/sidebar items, reorderable, each with icon/color.
- Ad-hoc quick filter bar on top of any feed (search title, toggle shorts, duration chips) — "Save as feed".
- Per-video actions (same card component as search): *play*, *add to watch list*, *hide*, *mark watched*.
- Regex/keyword matching applied in Rust after SQL prefilter (SQLite has no native regex) — fine at this scale.

### 5.5 Video playback
- `GET /api/v1/videos/{id}` → metadata + formats (via rustypipe `player()`), cached until URL expiry.
- **Adaptive streaming:** YouTube serves separate video/audio streams. Backend generates a **DASH MPD**
  (like Invidious) from the format list (itag, init/index ranges, codecs) → played via dash.js/Shaka in Vidstack.
  Fallback: muxed MP4 (≤360p) and HLS for live streams.
- **Proxy mode — ON by default** (admin can disable instance-wide): googlevideo URLs
  are bound to the server's IP and proxying hides the viewer's IP from Google.
  - `/proxy/video/{video_id}/{itag}` with Range passthrough; streaming `reqwest` body → axum `Body`
    (no buffering, backpressure via tokio), HTTP/2 to clients.
  - Fetch upstream in ≤10 MB chunks (`range=` param) — googlevideo throttles large ranges.
  - Refresh expired stream URLs transparently (re-run `player()` on 403, keep same proxy URL).
  - Per-user/concurrency limits and bandwidth metrics; manifests point at proxy URLs so the player
    never sees googlevideo.
  - Thumbnails/avatars also proxied (`/proxy/img/…`) with on-disk cache + `Cache-Control`.
- Player features: quality & speed selection, captions (YT timedtext → WebVTT), chapters from description,
  PiP, theater/fullscreen, keyboard shortcuts, double-tap seek and swipe gestures on mobile,
  Media Session API, audio-only mode, autoplay next from watch list.
- SponsorBlock segments (skip/mark), optional DeArrow titles/thumbnails — later milestone.

### 5.6 Resume where you left off
- Player sends progress **every ~10 s while playing** plus on `pause`, `seeked`, `ended`,
  and `visibilitychange`/`pagehide` (using `navigator.sendBeacon` so it survives tab close / app switch on mobile).
- `PUT /api/v1/progress/{video_id} { position_s, duration_s }` → upsert; `finished = position ≥ 90%` or
  `remaining < 30 s`.
- On load: player seeks to saved position (with a "Resume from 12:34 / Start over" toast).
- Video cards show a progress bar; **Continue watching** shelf/feed = `position > 30 s AND NOT finished`,
  sorted by `updated_at`.
- Offline/flaky network: queue updates in IndexedDB and flush when online (last-write-wins by timestamp).
- Works across devices because it is server-side per user.

### 5.7 Watch lists
- Built-in **Watch later** list per user + unlimited custom lists.
- "Add to list" menu on every card (search results, feeds, channel pages) and in the player (with "+ New list").
- Reorder with drag & drop (fractional `position` keys to avoid renumbering), remove, move between lists,
  sort by added/published/duration, show progress per item, hide finished.
- **Play list mode:** autoplay next, shuffle, auto-remove finished items from Watch later (option).
- Lists are Katsanto-only (no YouTube playlist import/sync). Video metadata is stored when added, so lists
  render instantly without hitting YouTube.

### 5.8 Theming & customization
- All visual decisions are **design tokens** as CSS variables:
  `--color-bg`, `--color-surface`, `--color-text`, `--color-accent`, `--radius`, `--font-sans`,
  `--density` (spacing scale), `--card-thumb-ratio`, etc.
- Built-in themes: Light, Dark, AMOLED black, High-contrast, a few accent palettes; follows OS preference.
- **Theme editor** in settings: color pickers with live preview, accent generation (OKLCH),
  export/import theme JSON, share themes; optional custom CSS box for power users.
- Layout options: grid vs. list feed, card size/density, thumbnails on/off, sidebar vs. bottom-nav (mobile),
  which metadata to show on cards.
- Settings stored server-side per user (`settings_json`), cached in localStorage for instant first paint.
- i18n from day one (`paraglide-js` or `svelte-i18n`); English + Finnish initially.

---

## 6. REST API (v1 sketch)

```
GET    /api/v1/auth/config                 -- passkeys available? passwords enabled? registration mode
POST   /api/v1/auth/register/passkey/start {username, invite}   POST .../register/passkey/finish
POST   /api/v1/auth/register/password {username, password, invite}
POST   /api/v1/auth/login/passkey/start    POST /api/v1/auth/login/passkey/finish
POST   /api/v1/auth/login/password {username, password}
POST   /api/v1/auth/login/recovery {username, code}
POST   /api/v1/auth/reset/{token}/...      POST /api/v1/auth/logout
POST   /api/v1/auth/reauth                 -- step-up before sensitive actions
GET    /api/v1/me/passkeys   POST /api/v1/me/passkeys/start|finish   PATCH|DELETE /api/v1/me/passkeys/{id}
PUT|DELETE /api/v1/me/password             POST /api/v1/me/recovery-codes (regenerate)
GET    /api/v1/me/sessions   DELETE /api/v1/me/sessions[/{id}]
GET    /api/v1/me            PATCH /api/v1/me/settings      DELETE /api/v1/me
GET    /api/v1/me/backup     POST  /api/v1/me/backup (restore Katsanto JSON)
GET|POST|PATCH|DELETE /api/v1/admin/users[/{id}]   POST /api/v1/admin/users/{id}/reset-link
GET|POST|DELETE /api/v1/admin/invites[/{id}]

GET    /api/v1/search?q=&type=video|channel&date=&duration=&sort=&cursor=
GET    /api/v1/search/suggestions?q=        GET|DELETE /api/v1/search/history
GET    /api/v1/resolve?url=
GET    /api/v1/videos/{id}                 GET /api/v1/videos/{id}/manifest.mpd
GET    /api/v1/videos/{id}/captions/{lang}.vtt
GET    /api/v1/channels/{id}               GET /api/v1/channels/{id}/videos?tab=&cursor=

GET    /api/v1/subscriptions               POST /api/v1/subscriptions {channel}
DELETE /api/v1/subscriptions/{channel_id}

GET|POST|PATCH|DELETE /api/v1/tags[/{id}]   PUT /api/v1/channels/{id}/tags
GET|POST|PATCH|DELETE /api/v1/feeds[/{id}]  GET /api/v1/feeds/{id}/videos?cursor=
POST   /api/v1/feeds/preview {filter}      -- live preview in feed editor

GET    /api/v1/progress?ids=…              PUT /api/v1/progress/{video_id}
GET    /api/v1/continue-watching           DELETE /api/v1/progress/{video_id}

GET|POST|PATCH|DELETE /api/v1/playlists[/{id}]
POST   /api/v1/playlists/{id}/items        PATCH|DELETE /api/v1/playlists/{id}/items/{video_id}

POST   /api/v1/videos/{id}/hide | /watched
GET    /api/v1/events (SSE)
```
Cursor-based pagination (`published_at,id`) everywhere for feeds.

---

## 7. Milestones

**M0 — Spike / de-risk (≈1 week)**
- Minimal Rust binary: rustypipe `search()` → `player()` for a result → generate DASH MPD → play in a bare
  Vidstack page through the proxy. Confirm PO token flow with `rustypipe-botguard`.
- Measure: request latency, memory footprint, RSS polling cost for ~200 channels.
- Passkey round-trip with `webauthn-rs` + `@simplewebauthn/browser` on iOS Safari, Android Chrome and a desktop
  browser behind HTTPS (e.g. Tailscale or a reverse proxy).
- *Exit criteria:* passkey login works on those devices; search returns results and 1080p playback with seeking **through the proxy** works on
  desktop Firefox/Chrome and iOS Safari / Android Chrome.

**M1 — Skeleton**
- Workspace, config, tracing, SQLite + migrations, OpenAPI, CI (fmt, clippy, tests).
- Local accounts: first-user admin bootstrap, invite links, **passkey registration/login (autofill UI)**,
  password fallback, auto password-only mode without HTTPS/domain, recovery codes, admin reset links,
  session management, basic admin UI.
- SvelteKit app shell: routing, layout (sidebar/bottom-nav), token-based theme system with Light/Dark, API client.

**M2 — Search & watch**
- Search with suggestions, filters, infinite scroll, URL resolving; shared video/channel card components.
- Video page with player, metadata, captions; channel page; thumbnail proxy.
- Watch progress saving/resume + Continue watching.

**M3 — Subscriptions & feed**
- Subscribe/unsubscribe from search results, video and channel pages; manage subscriptions page.
- Feed refresher + enricher workers, SSE updates, "All" feed with infinite scroll.

**M4 — Multiple feeds & filtering**
- Tags on channels, feed editor with live preview, quick filter bar, hide/mark-watched.

**M5 — Watch lists**
- Watch later + custom lists, "add to list" from search/feeds/player, drag-drop reorder, play-list mode.

**M6 — Customization & polish**
- Theme editor, layout/density options, PWA, Media Session, keyboard shortcuts, gestures, i18n,
  accessibility pass (keyboard nav, focus, contrast, reduced motion).

**M7 — Extras & release**
- Backup export/import, SponsorBlock, DeArrow, WebSub, Docker image (multi-arch), docs, v0.1 release.

Later ideas: comments, related videos/recommendations from subscriptions, downloads (`rustypipe-downloader`),
YouTube Music mode, Postgres backend, federated/shared instances, Android TV layout.

---

## 8. Testing strategy
- `katsanto-yt` mock implementation + recorded JSON fixtures → deterministic service tests.
- Feed filter engine: table-driven unit tests (filter JSON → expected video set) and property tests.
- API integration tests with `axum::Router` + in-memory SQLite.
- Nightly "canary" job hitting real YouTube via rustypipe to detect breakage early.
- Auth: passkey ceremonies with `webauthn-rs` soft-token test authenticator; last-credential rule, recovery
  codes, reset links, rate limiting, password-only fallback when `public_url` is not HTTPS.
- Search: fixture-based tests for result mapping (videos, channels, shorts, live, missing fields).
- Frontend: Vitest for logic (progress queue, theme generation), Playwright for key flows on desktop + mobile viewport.

---

## 9. Risks & mitigations

| Risk | Mitigation |
|---|---|
| YouTube changes break extraction | Track rustypipe releases closely; isolate in `katsanto-yt`; canary job; Renovate/dependabot |
| Server IP rate-limited/blocked by YouTube | Prefer RSS for polling; adaptive intervals; concurrency limits; caching; optional outbound proxy |
| PO token / attestation changes | Rely on `rustypipe-botguard`; keep it a separate updatable binary |
| Stream URLs IP-bound | Proxy mode with range passthrough |
| GPL licensing | License the project AGPL-3.0 |
| Search traffic triggers YouTube rate limits | Debounced suggestions, shared result cache, per-user search rate limit |
| Instance abused as open video proxy | Login required for everything; registration closed/invite by default; per-user limits |
| Weak passwords / brute force | Passkeys by default; argon2id, length + common-password check, login rate limiting; admin can disable passwords |
| Passkeys unavailable (no HTTPS/domain, old browser) | Automatic password fallback; clear admin warning + setup docs |
| Lost device / passkey | Synced passkeys, multiple passkeys per user, recovery codes, admin reset link |
| Domain change breaks passkeys (RP ID bound to hostname) | Documented; passwords/recovery codes/reset links still work to re-enroll |
| Proxy bandwidth / server load | Streaming without buffering, chunked upstream ranges, per-user limits, metrics |
| Scope creep | Ship M0–M5 as MVP before extras |
| ToS/legal | Personal self-hosting focus; no ads stripping claims beyond what the client does; document clearly |

---

## 10. Decisions
1. **Accounts:** Katsanto's own local accounts, multi-user, no link to YouTube/Google. rustypipe runs anonymously.
2. **No YouTube import or sync** of subscriptions, watch lists or history. Users build everything in Katsanto,
   primarily through **search**.
3. **Streaming:** proxied through the server by default.
4. **Frontend:** SvelteKit.
5. **Registration:** invite-only by default on a fresh install.
6. **Guest access:** none — all features require logging in.

7. **Authentication:** passkeys first, password fallback (+ recovery codes and admin reset links).

## 11. Remaining open questions
- None currently. Next step: start M0.
