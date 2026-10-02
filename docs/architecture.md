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
- Implements only the commands lyrica needs, added incrementally. Currently: `password`, `ping`, `status`, `currentsong`, `play`, `pause`, `previous`, `next`, `idle`.
- **`Client`** is one connection running one command at a time; responses are framed by line and each line parsed with nom (`OK`, `ACK …`, `key: value`). Typed results (`Status`, `Song`) keep only the fields used so far and ignore unknown keys.
  - `ACK` errors leave the connection usable. I/O and protocol errors, or a command future dropped mid-response, leave it out of sync: later commands fail with `Error::Broken` and the caller reconnects.
  - `idle` blocks its connection, so idling needs its own `Client`. Arguments are always quoted; newlines are rejected.
  - Not yet handled: MPD's `connection_timeout` closing unused connections, binary responses, command lists.
- **Connection multiplexing (direction, not final):** several MPD connections behind one combined API, so slow work never delays interactive work. For example:
  - a _control_ connection for low-latency commands (play/pause/seek/volume) and reacting to state changes (`idle`);
  - a _bulk_ connection (or pool) for long-running, non-essential work: queue/playlist windows, extended song info, searches.
  - the exact number of connections and the scheduling policy are open.
- Large results (queue, playlists) are always requested in ranges/windows, never wholesale.

## Server (crates/lyrica)

Thin bridge between the frontend and `lyrica-mpd`.

- axum on tokio; serves the embedded frontend (rust-embed) and the frontend WebSocket at `ws` (relative to the page). Until phase 3 lands, the server only sends a `lyrica <version>` greeting; the frontend shows connection status and reconnects after drops.
- Configuration exclusively via CLI arguments (clap); no disk state.
- Plain HTTP; intended to sit behind Caddy. All frontend URLs are relative so a path prefix works, provided the page URL ends in `/`.
- Setup-specific extensions live here or in `lyrica-mpd` as appropriate, e.g. resolving album art from the HTTP mirror of the music library (looking beyond the song's own folder, unlike MPD's `albumart`). Whether the frontend loads art directly from the mirror URL or the backend fetches and forwards it is undecided.

### MPD bridge (phase 3, decided)

- **CLI:** `--mpd <host:port>` (default `127.0.0.1:6600`), optional `--mpd-password`, sent with `password` after every connect.
- **Idle connection:** a task owns one `Client`. It fetches `status` + `currentsong`, publishes the snapshot, then loops `idle(player)` → refetch → publish. Snapshot fetches run on this connection, so they never queue behind commands.
- **Command connection:** a task owns a second `Client` and serves an `mpsc` of commands from all WS clients, serialized. MPD closes connections unused for `connection_timeout` (default 60 s), so on `Io`/`Broken` it reconnects and retries the command once; the closed connection never executed it. `Ack` is logged, not retried.
- **Snapshot fan-out:** `tokio::sync::watch`. Each WS client gets the current snapshot on connect, then every change.
- **MPD unreachable:** the idle task publishes an `offline` snapshot and reconnects with backoff (1 s doubling to 10 s). Commands sent while offline are dropped with a log line.
- **WS protocol:** JSON text messages (serde). Revisit only if size or speed proves a problem.
  - Server → client: `{"type":"state","state":"play"|"pause"|"stop"|"offline","title":string|null,"elapsed":number|null,"duration":number|null}`. `title` is the `Title` tag, falling back to the file name; `null` when there is no current song. `elapsed`/`duration` are seconds (fractional) from `status`.
  - MPD sends no progress events (the `player` event fires only on play/pause/stop, seek and song change), so the server does not stream progress; the frontend extrapolates it (see Frontend).
  - Client → server: `{"type":"previous"}`, `{"type":"toggle"}`, `{"type":"next"}`. Invalid messages are logged and ignored.
  - No per-command replies: the state push triggered by `idle` is the confirmation. Measured on the local MPD: the `player` event arrives < 1 ms after `pause`, 7–18 ms after `next`/`previous`.
- **`toggle`:** `play` → `pause(true)`, `pause` → `pause(false)`, `stop` → `play()`, using the latest snapshot. Never send a command that changes nothing: MPD emits no idle event for it, so the frontend would get no update.

## Frontend (web/)

Preact + TypeScript PWA built by Vite into `web/dist`, which is embedded into the server binary.

UI direction:

- **Top:** album art (empty placeholder when missing), song info, progress bar; controls: previous, play/pause, next, volume (MPD-side), random toggle, single toggle.
- **Progress bar:** updates smoothly and continuously during playback. Each `state` message anchors `elapsed` to the local receive time (`performance.now()`); while `play`, the bar shows `elapsed + (now − anchor)`, clamped to `duration`, redrawn every animation frame. Re-anchored by every state push (seek, pause, song change) and by the fresh snapshot after a reconnect; also resync when the page becomes visible again, since iOS suspends background pages and their sockets. No polling.
- **Rest of screen:** live queue (current MPD playlist), auto-scrolling and re-centering on the current song when it changes. Rendered virtually/windowed and loaded lazily to cope with 200k entries.
  - Adjacent tracks from the same album are visually grouped (small gap/separator), with one album-art icon per group; no icon if the album has no art.
  - Per-track info: album, album/track artist, title, track number. Exact layout to be iterated on.
- **Hidden settings menu:** optional listening to MPD's httpd audio stream; stored-playlist view (list server playlists, clear the queue, load a playlist).
- Slow data (cover art, queue windows, extended info) is fetched lazily and asynchronously; controls and now-playing never wait on it.
- Light/dark mode follows the system preference.
