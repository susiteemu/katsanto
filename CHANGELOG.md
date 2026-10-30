# Changelog

## 1.2.0

- A video's full title is shown when hovering over it in video cards.

Fixes.

- The watch page no longer grows wider than the screen on phones: a long unbreakable link in
  the description and the player's own size once the video loads could both widen it.

## 1.1.1

- View counts are only shown in search results. Elsewhere they came from the channel's
  feed, which only lists the newest uploads, so they were often out of date.

## 1.1.0

- Share a video's YouTube link: a "Share YouTube link" button on the watch page opens the
  system share sheet or copies the link, and video card menus have "Copy YouTube link".
- Videos can be removed from Continue watching through their "⋮" menu, which also works on
  touch screens; the remove button there only showed on hover.

Fixes.

- Thumbnails no longer fail to load after a creator changes a video's custom thumbnail: the
  image proxy falls back to the video's standard thumbnail.

## 1.0.0

Subtitles.

- Subtitles no longer hug the left edge: a new Subtitles card in settings places them bottom
  left, center (the default) or right, with a small margin.
- Preferred subtitle languages are listed first in the player's menu, and languages written
  in non-Latin scripts can be hidden.

Security fixes.

- Adding a passkey, and admin changes to users and instance settings, now need a recent
  re-authentication, so a stolen session can't be turned into lasting access.
- Login rate limits are per client behind a reverse proxy (new `trusted_proxies` setting);
  previously everyone behind the proxy shared one limit.
- Proxied images must be raster images and are served with `nosniff` and a sandboxing CSP.
- Security headers on all responses: Content-Security-Policy, `X-Content-Type-Options`,
  `X-Frame-Options`, `Referrer-Policy`.
- Video description links are only links for `http(s)` URLs.

New look.

- A new logo, a retro TV, replacing the play-button mark that looked too much like
  YouTube's. The default accent is now teal instead of raspberry red
  (Raspberry is still available in the appearance settings).

Fixes.

- The server is rebuilt when the web app changes. Previously a server built before
  `npm run build` kept answering "The web app is not built" even after the web app was
  built, until its code changed.
- "⋮" menus are no longer cut off inside scrolling rows such as Continue watching, and open
  upwards when there's no room below.

## [0.1.0] - 2026-10-25

First release.

- Accounts: passkeys with password fallback, invite-only registration by default, recovery
  codes, admin reset links, sessions, re-authentication for sensitive changes; no guest access.
- YouTube search (videos, channels, links) with suggestions; channel pages; a watch page with
  description, chapters and related videos.
- Playback through the server's own YouTube player client (TV client, yt-dlp's ejs signature
  solver, PO tokens from rustypipe-botguard), streamed through the server; Video.js 10 player
  with captions, chapters, audio tracks, quality and speed, keyboard shortcuts and media keys.
- Resume where you left off, continue watching, watched markers.
- Subscriptions with background RSS refresh (adaptive intervals, backoff; the channel page
  stands in when YouTube's RSS feeds fail) and a live feed; optional WebSub push.
- Tags and saved feeds with filters (tags, channels, title words and regex, length, watched,
  Shorts, live, age).
- Watch later and custom lists: reordering, sorting, moving, play all and shuffle with autoplay.
- SponsorBlock (per-category skip, skip button or timeline marks) and DeArrow titles and
  thumbnails, looked up with hash prefixes through the server.
- Appearance: themes (light, dark, black, high contrast), accent color, corners, font, density,
  grid/list layout, card details, custom CSS, theme export/import.
- English and Finnish.
- Per-user backup and restore.
- PWA with an offline shell; a Dockerfile and compose example for building and running your own image.
