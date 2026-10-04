# Roadmap

Work proceeds bottom-up in small steps, each usable on its own. Design details live in `architecture.md`.

## Current state

Now playing and the live queue are done and checked on an iPhone. Stored playlists are done and checked in headless Chromium; the check on the phones is left.

- **Foundation:** Nix flake (dev shell, build, fmt, check, NixOS module); `lyrica-mpd` with the commands lyrica needs; server bridging WebSocket clients to MPD (idle, command and queue connections, reconnect with backoff, offline state, stored playlist listing and loading); frontend embedded in the binary.
- **Now playing:** title, artist (album artist as fallback), album, album art from the HTTP mirror, smooth progress bar with the time beside it, tap / drag seeking on the bar (full mode), previous / play-pause / next, random / single / repeat, MPD volume slider, bouncing marquee for long lines, fixed-height layout pinned to the bottom, animated full / mini mode switch.
- **Live queue (mini mode):** the whole queue in a virtual list of unbounded length under the toolbar; rows (track number, title, artist · album, duration) on alternating backgrounds, loaded on demand while shown, two lists ahead on each side; a separator at each album change and the album's cover beside its top visible row, pinned while the album scrolls, probed two lists ahead, also while scrolling; centred on the current song when opened and on each song change (paused for 15 s after the user scrolls), which is highlighted; the visible rows keep their old text across a queue change until their refetch lands; tapping a row plays it. Nothing loads for it in full mode.
- **Stored playlists:** a toolbar button lists them afresh on each tap (a toast when there are none or listing failed) in a modal; "Load" replaces the queue, with a modal "Loading…" box (seen only after 300 ms) until MPD is done, then the list at its top, or MPD's error in a small modal.
- **Play here:** plays MPD's `httpd` stream in the browser, with title, artist, album and cover on the lock screen.
- **iOS fit:** fixed viewport (no bounce, no zoom, no selection), full height in home-screen apps, safe areas.
- **Dark only:** gruvbox dark with its accent colours, regardless of the system setting.
- **PWA:** app icon and favicon, screen kept on while visible, portrait on Android.

## Open work

- **Live queue design:** the row layout and album styling are open to iteration from daily use (`architecture.md` → "Rows").

## Next phase: stored playlists

A load button at the left of the mini-mode toolbar lists the server's stored playlists on each tap and opens them in a modal; selecting one and tapping "Load" closes it and replaces the queue, with a "Loading…" box until MPD is done (`architecture.md` → "Stored playlists"). No general settings menu. Loading never touches the real MPD's queue during development: throwaway MPD or scripted fake (`development.md`). Every step keeps the responsiveness checks (`architecture.md` → "Responsiveness").

1. **Playlist protocol in `lyrica-mpd`:** done. `Client::playlists()` and `Client::replace_queue(name)` (`clear` + `load` in one private command list); measurements and failure modes in `architecture.md` → "Queue measurements". Main finding: a load blocks MPD for every client while it runs, up to ~20 s for the real MPD's large playlists (estimated from `playlistlength`).
2. **Playlists over the WS:** done. `{"type":"playlists"}` (queue connection) and `{"type":"load","name"}` (command connection, checked against a fresh listing first), each answered to its client alone; protocol, errors and measurements in `architecture.md` → "MPD bridge" → "Stored playlists".
3. **Load button and dialogs:** done. `playlists.tsx`, `Connection.playlists()` / `load(name)` in `ws.ts`, `QueueStore.top()`; design and measurements in `architecture.md` → "Stored playlists" and "Measurements".
4. **By hand on the phones** (iPhone home-screen app, Android): dialog layout and scrolling, backdrop tap, the Loading box and the jump to top, the error modal, the toast. The user's call: on the deployed server a load replaces the real queue.

## Later (to design)

Ideas to revisit once the phases above are done; how they look and work is open.

- **Queue actions:** convenience buttons in the mini-mode toolbar. Example: play the current album from the start — jump to the first track of the current song's album in the queue, play it, and turn random off (done by hand today). The set of actions and how to find album boundaries without fetching the whole queue are open.
