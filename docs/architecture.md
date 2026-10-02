# Architecture

```
iPhone (PWA) ──HTTPS/WSS── Caddy ──HTTP/WS── lyrica (crates/lyrica) ──TCP── MPD
                                               │  embeds web/ build
                                               └─ uses lyrica-mpd (crates/mpd)
```

Only the general direction is fixed here; details are decided while implementing and recorded back into this file.

## `lyrica-mpd` (crates/mpd)

Async MPD client library, independent of the web server.

- tokio TCP connections; responses parsed with nom 8 (`Parser` trait API).
- Implements only the commands lyrica needs, added incrementally.
- **Connection multiplexing (direction, not final):** several MPD connections behind one combined API, so slow work never delays interactive work. For example:
  - a _control_ connection for low-latency commands (play/pause/seek/volume) and reacting to state changes (`idle`);
  - a _bulk_ connection (or pool) for long-running, non-essential work: queue/playlist windows, extended song info, searches.
  - the exact number of connections and the scheduling policy are open.
- Large results (queue, playlists) are always requested in ranges/windows, never wholesale.

## Server (crates/lyrica)

Thin bridge between the frontend and `lyrica-mpd`.

- axum on tokio; serves the embedded frontend (rust-embed) and the frontend WebSocket at `ws` (relative to the page). The protocol is not yet designed; currently the server only sends a `lyrica <version>` greeting and the frontend shows connection status, reconnecting after drops.
- Configuration exclusively via CLI arguments (clap); no disk state.
- Plain HTTP; intended to sit behind Caddy. All frontend URLs are relative so a path prefix works, provided the page URL ends in `/`.
- Setup-specific extensions live here or in `lyrica-mpd` as appropriate, e.g. resolving album art from the HTTP mirror of the music library (looking beyond the song's own folder, unlike MPD's `albumart`). Whether the frontend loads art directly from the mirror URL or the backend fetches and forwards it is undecided.

## Frontend (web/)

Preact + TypeScript PWA built by Vite into `web/dist`, which is embedded into the server binary.

UI direction:

- **Top:** album art (empty placeholder when missing), song info, live progress; controls: previous, play/pause, next, volume (MPD-side), random toggle, single toggle.
- **Rest of screen:** live queue (current MPD playlist), auto-scrolling and re-centering on the current song when it changes. Rendered virtually/windowed and loaded lazily to cope with 200k entries.
  - Adjacent tracks from the same album are visually grouped (small gap/separator), with one album-art icon per group; no icon if the album has no art.
  - Per-track info: album, album/track artist, title, track number. Exact layout to be iterated on.
- **Hidden settings menu:** optional listening to MPD's httpd audio stream; stored-playlist view (list server playlists, clear the queue, load a playlist).
- Slow data (cover art, queue windows, extended info) is fetched lazily and asynchronously; controls and now-playing never wait on it.
- Light/dark mode follows the system preference.
