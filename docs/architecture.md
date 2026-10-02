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

- axum on tokio; serves the embedded frontend (rust-embed) and the frontend WebSocket at `ws` (relative to the page).
- Configuration exclusively via CLI arguments (clap); no disk state.
- Plain HTTP; intended to sit behind Caddy. All frontend URLs are relative so a path prefix works, provided the page URL ends in `/`.
- Setup-specific extensions live here or in `lyrica-mpd` as appropriate, e.g. resolving album art from the HTTP mirror of the music library (looking beyond the song's own folder, unlike MPD's `albumart`). Whether the frontend loads art directly from the mirror URL or the backend fetches and forwards it is undecided.

### MPD bridge (`bridge.rs`, `ws.rs`)

- **CLI:** `--mpd <host:port>` (default `127.0.0.1:6600`), optional `--mpd-password`, sent with `password` after every connect.
- **Idle connection:** a task owns one `Client`. It fetches `status` + `currentsong`, publishes the snapshot, then loops `idle(player)` → refetch → publish. Snapshot fetches run on this connection, so they never queue behind commands.
- **Command connection:** a task owns a second `Client` and serves an `mpsc` of commands from all WS clients, serialized. MPD closes connections unused for `connection_timeout` (default 60 s), so on `Io`/`Broken` it reconnects and retries the command once; the closed connection never executed it. `Ack` is logged, not retried.
- **Snapshot fan-out:** `tokio::sync::watch`. Each WS client gets the current snapshot on connect, then every change. While playing, `elapsed` is advanced to the send time, so late subscribers get the current position.
- **MPD unreachable:** the idle task publishes an `offline` snapshot and reconnects with backoff (1 s doubling to 10 s). Commands sent while offline are dropped with a log line.
- **WS protocol:** JSON text messages (serde). Revisit only if size or speed proves a problem.
  - Server → client: `{"type":"state","state":"play"|"pause"|"stop"|"offline","title":string|null,"elapsed":number|null,"duration":number|null}`. `title` is the `Title` tag, falling back to the file name; `null` when there is no current song. `elapsed`/`duration` are seconds (fractional) from `status`.
  - MPD sends no progress events (the `player` event fires only on play/pause/stop, seek and song change), so the server does not stream progress; the frontend extrapolates it (see Frontend).
  - Client → server: `{"type":"previous"}`, `{"type":"toggle"}`, `{"type":"next"}`. Invalid messages are logged and ignored.
  - No per-command replies: the state push triggered by `idle` is the confirmation. Measured on the local MPD (WS client to state message): `toggle` 0.4–1.8 ms, `next` 19 ms, `previous` 3 ms; via `mpc` (including process spawn): `pause` 1.5 ms, `play` 40 ms, `seek` 8 ms. After killing the proxy, `offline` arrives in 40 ms; the first command after a lost connection or a 65 s idle gap reconnects and succeeds in < 2 ms. MPD does not time out connections in `idle`.
- **`toggle`:** `play` → `pause(true)`, `pause` → `pause(false)`, `stop` → `play()`, using the latest snapshot. Never send a command that changes nothing: MPD emits no idle event for it, so the frontend would get no update.

## Frontend (web/)

Preact + TypeScript PWA built by Vite into `web/dist`, which is embedded into the server binary.

UI direction:

- **Now playing, pinned to the bottom:** album art, title, artist, progress bar, then controls at the very bottom (previous, play/pause, next; later volume (MPD-side), random toggle, single toggle). Every element has a fixed height so nothing moves when the song changes.
- **Progress bar:** updates smoothly and continuously during playback. Each `state` message anchors `elapsed` to the local receive time (`performance.now()`); while `play`, the bar shows `elapsed + (now − anchor)`, clamped to `duration`, redrawn every animation frame. Re-anchored by every state push (seek, pause, song change) and by the fresh snapshot after a reconnect. No polling.
- **Rest of screen (above now playing):** live queue (current MPD playlist), auto-scrolling and re-centering on the current song when it changes. Rendered virtually/windowed and loaded lazily to cope with 200k entries.
  - Adjacent tracks from the same album are visually grouped (small gap/separator), with one album-art icon per group; no icon if the album has no art.
  - Per-track info: album, album/track artist, title, track number. Exact layout to be iterated on.
- **Hidden settings menu:** optional listening to MPD's httpd audio stream; stored-playlist view (list server playlists, clear the queue, load a playlist).
- Slow data (cover art, queue windows, extended info) is fetched lazily and asynchronously; controls and now-playing never wait on it.
- Light/dark mode follows the system preference.

### Now playing (implemented)

Built in phase 4 (milestone 1) and extended since. Covers title, play state, progress bar, previous / play-pause / next.

- **`ws.ts`:** owns the socket and the protocol types (`Snapshot`: a `state` message stamped with its `performance.now()` receive time; `Command`). Parses `state` messages (ignores anything else), exposes `send(command)` (dropped unless open). Reconnects 1 s after a close. On `visibilitychange` to visible, closes and reopens the socket: the server sends a fresh snapshot on connect (with `elapsed` advanced), which resyncs the bar and avoids zombie sockets after iOS suspends the page; `performance.now()` may not advance while the device sleeps.
- **Offline:** a closed socket is displayed exactly like `state: "offline"`. Buttons are disabled while offline.
- **No optimistic UI:** buttons only send; the UI changes when the state push arrives (< 20 ms on the local MPD).
- **Play/pause button** shows pause while `play`, play otherwise; sends `toggle`.
- **Title:** `title`, or a placeholder when `null`. One line of fixed height; overflow scrolls sideways by touch (hidden scrollbar) instead of wrapping. Keyed by the text, so a new song starts scrolled to the beginning.
- **Progress:** the pure `position(snapshot, now)` (`progress.ts`, Vitest) computes the displayed seconds. A `requestAnimationFrame` loop runs only while playing and writes the bar (`transform: scaleX`) and the `m:ss / m:ss` text directly through refs, the text only when it changes, so Preact does not re-render per frame. `duration` `null` (streams): empty bar, elapsed text only. No seeking by tapping the bar (the protocol has no seek).
- **Layout:** single column for iPhone portrait; `.app` is a `100dvh` flex column and the now-playing section sits at the bottom (`margin-top: auto`), so progress bar and controls stay at fixed positions. Touch targets ≥ 44 px, safe-area padding, colours from the CSS variables (light/dark).

Phase 4 verification, in headless Chromium (390×844) against the local MPD, timed from the action to the DOM change: `mpc` pause/play/seek/next/prev 2–4 ms; button toggle/next/previous 2 ms. The bar advances every frame (62 samples/s, monotonic) and holds still while paused. Killing the proxy or the server shows offline with disabled buttons; after a server restart the UI recovers within the 1 s retry (353 ms observed). After freezing the page (Chrome closed the socket) and seeking meanwhile, the visibility event opened one new socket and the bar matched MPD within 3 ms.

Bottom layout: title, progress bar and controls kept identical positions across a song change, and an 880 px wide title stayed 28 px tall and scrollable.
