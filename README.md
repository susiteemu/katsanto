# Katsanto

A lightweight, self-hostable YouTube front-end written in Rust, with its own accounts,
subscriptions and watch lists. No Google account, no ads, no tracking: everything goes
through your server. See [PLAN.md](PLAN.md) for the design.

- **Subscriptions** from search (videos, channels, or a pasted link) with a live-updating feed
- **Multiple feeds**: tag channels and save filters (tags, channels, title words, length,
  watched state, Shorts/live, age) as tabs
- **Player** (Video.js 10) that works on phones and desktops: quality, speed, captions, chapters,
  audio tracks, keyboard shortcuts, media keys, picture-in-picture, double-tap seek
- **Resume where you left off** on any device, "continue watching", watched markers
- **Watch lists**: Watch later plus your own lists, drag & drop, play all / shuffle with autoplay
- **SponsorBlock** (skip or offer to skip segments) and **DeArrow** (better titles and
  thumbnails), looked up privately through the server
- **Make it yours**: themes, accent color, density, grid/list layout, custom CSS, English and Finnish
- **Accounts** with passkeys (password fallback), invites, recovery codes, admin tools,
  and a per-user **backup** for moving between servers
- **Installable** as an app (PWA)

Katsanto has no YouTube login and imports nothing from YouTube: you build your subscriptions
and lists in Katsanto.

## Building and running

Katsanto is distributed as source: you build your own copy, either as a native binary or
as a container image. Both include the web app; the server also needs
[`rustypipe-botguard`](https://codeberg.org/ThetaDev/rustypipe-botguard) to mint PO tokens
(without them YouTube stops serving a stream after a few megabytes).

```sh
git clone <repository-url> katsanto
cd katsanto
```

### Native binary

Needs [Rust](https://rustup.rs) (stable) and [Node.js](https://nodejs.org) 24, on Linux or
macOS.

```sh
# 1. The web app (embedded into the server binary at compile time)
(cd web && npm ci && npm run build)

# 2. The server
cargo build --release -p katsanto-server

# 3. The PO token helper, into data/tools/bin where the server finds it
cargo install rustypipe-botguard --locked --root data/tools

# 4. Run
./target/release/katsanto-server
# open http://localhost:3000
```

The binary (`target/release/katsanto-server`) is self-contained apart from botguard; copy it
anywhere and point it at a config file and data directory (see
[Configuration](#configuration)). To run it as a service, a systemd unit along these lines works:

```ini
# /etc/systemd/system/katsanto.service
[Unit]
Description=Katsanto
After=network-online.target

[Service]
User=katsanto
WorkingDirectory=/var/lib/katsanto
Environment=KATSANTO_CONFIG=/etc/katsanto.toml
ExecStart=/usr/local/bin/katsanto-server
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

### Container image

Needs Docker (or Podman). The [Dockerfile](Dockerfile) builds the web app, the server and
botguard in build stages, so nothing else is required on the host:

```sh
docker build -t katsanto .
docker run -d --name katsanto -p 3000:3000 -v katsanto-data:/data \
  -e KATSANTO_PUBLIC_URL=https://tv.example.com katsanto
```

The image runs as an unprivileged user and keeps everything — the SQLite database and caches
— in the `/data` volume. Configure it with `KATSANTO_*` environment variables. The first
build takes a while (botguard bundles V8); later builds reuse the cached layers.

[compose.yaml](compose.yaml) builds the image and runs it behind
[Caddy](https://caddyserver.com), which takes care of HTTPS: set your domain in it, then
`docker compose up -d --build`.

### First start

Open the URL and create the first account, which becomes the admin. After that,
registration is by invite (Admin → Invites).

### Updating

```sh
git pull
# native: rebuild steps 1–2 and restart the server
# container: docker compose up -d --build (or docker build … and recreate the container)
```

The database is migrated automatically on start. Back up `data_dir` before updating (see
[Backups](#backups)).

## HTTPS and passkeys

Passkeys are the primary login; passwords are a fallback that admins can switch off once
everyone has a passkey. Browsers only allow passkeys on `https://` with a domain name (or
`http://localhost`), so put Katsanto behind a reverse proxy with TLS and set `public_url` to
the address people use:

```
# Caddyfile
tv.example.com {
	reverse_proxy localhost:3000
}
```

Behind a reverse proxy, also set `trusted_proxies` to the proxy's address (`["127.0.0.1"]`
for the Caddyfile above) so logins are rate-limited per client rather than for everyone at
once.

Tailscale HTTPS (`https://<machine>.<tailnet>.ts.net`) works too. On a plain
`http://192.168.x.x` address Katsanto runs with passwords only. Changing the domain later
invalidates existing passkeys (passwords, recovery codes and admin reset links still work).

## Configuration

`katsanto.toml` (see [katsanto.example.toml](katsanto.example.toml); another file can be named
with `KATSANTO_CONFIG`), overridden by `KATSANTO_*` environment variables (`public_url` →
`KATSANTO_PUBLIC_URL`).

| Key | Default | |
|---|---|---|
| `bind` | `127.0.0.1:3000` | Listen address (`0.0.0.0:3000` in Docker) |
| `public_url` | `http://localhost:3000` | The URL people open; decides cookies and passkeys |
| `data_dir` | `data` | Database and caches |
| `botguard_bin` | `<data_dir>/tools/bin/…`, then `PATH` | `rustypipe-botguard` executable |
| `proxy_video` | `true` | Stream video through the server (googlevideo URLs are bound to its IP anyway) |
| `sponsorblock_api` | `https://sponsor.ajay.app` | SponsorBlock/DeArrow API; empty turns both off |
| `dearrow_thumbnails` | `https://dearrow-thumb.ajay.app` | DeArrow thumbnail service; empty turns DeArrow thumbnails off |
| `websub` | `false` | Push notifications for new uploads (below) |
| `websub_hub` | `https://pubsubhubbub.appspot.com/subscribe` | WebSub hub |
| `trusted_proxies` | `[]` | Reverse proxies (addresses or networks) whose `X-Forwarded-For` gives the client address for rate limiting |

Registration mode, password logins and invites are managed on the admin page.

### New uploads: polling and WebSub

Katsanto polls each subscribed channel's RSS feed — every 20 minutes for active channels,
up to every 6 hours for quiet ones — and reads the channel page instead when YouTube's
feeds are down. If the server is reachable from the internet at
`public_url`, set `websub = true`: YouTube's hub then announces uploads within a minute, and
polling slows to a 6-hourly safety net.

### Privacy

All YouTube traffic (search, metadata, video, images) comes from the server; viewers'
browsers never talk to Google. SponsorBlock and DeArrow lookups also go through the server
and send only the first 4 characters of the video id's SHA-256 hash, so that API learns
neither who is watching nor exactly what.

## Backups

Each user can download their data (Settings → Backup: subscriptions, tags, feeds, lists,
watch progress, settings) and restore it on any Katsanto server; restoring merges and never
deletes. For the whole instance, back up `data_dir` (stop the server first, or use
`sqlite3 katsanto.db ".backup copy.db"`).

## Development

```sh
# terminal 1: API server, with the Vite dev server as public origin
KATSANTO_PUBLIC_URL=http://localhost:5173 cargo run -p katsanto-server

# terminal 2: web app with hot reload (proxies /api and /proxy to :3000)
cd web && npm run dev
```

- `cargo test --workspace` — Rust tests, including end-to-end API tests with a software passkey
- `cd web && npm run check && npm test` — type check and unit tests
- UI text lives in `web/src/lib/i18n/` (`en.ts` is the source; other languages must have the same
  shape, so a missing translation fails `npm run check`)
- `cd web && npm run api` — regenerate the typed API client after changing server routes
  (CI fails if it is out of date)

Layout: `crates/katsanto-yt` (YouTube: rustypipe for metadata, own player client + yt-dlp's ejs
solver for streams), `crates/katsanto-db` (SQLite), `crates/katsanto-server` (axum API, auth,
media proxy, SponsorBlock/DeArrow, WebSub, embedded web app), `web/` (SvelteKit app).

## License

[AGPL-3.0-only](LICENSE). Vendored ejs solver: Unlicense (bundled meriyah: ISC, astring: MIT).
Katsanto is not affiliated with YouTube or Google. SponsorBlock and DeArrow data come from the
SponsorBlock community under CC BY-NC-SA 4.0.
