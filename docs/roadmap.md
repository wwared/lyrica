# Roadmap

Work proceeds bottom-up in small steps, each usable on its own. Design details live in `architecture.md`.

## Current state

Now playing is done, including seeking on the bar in full mode. The queue area above it (mini mode) is an empty placeholder and there are no stored playlists.

- **Foundation:** Nix flake (dev shell, build, fmt, check, NixOS module); `lyrica-mpd` with the commands now playing needs; server bridging WebSocket clients to MPD (idle and command connections, reconnect with backoff, offline state); frontend embedded in the binary.
- **Now playing:** title, artist (album artist as fallback), album, album art from the HTTP mirror, smooth progress bar with the time beside it, tap / drag seeking on the bar (full mode), previous / play-pause / next, random / single / repeat, MPD volume slider, bouncing marquee for long lines, fixed-height layout pinned to the bottom, animated full / mini mode switch.
- **Play here:** plays MPD's `httpd` stream in the browser, with title, artist, album and cover on the lock screen.
- **iOS fit:** fixed viewport (no bounce, no zoom, no selection), full height in home-screen apps, safe areas.
- **Dark only:** gruvbox dark with its accent colours, regardless of the system setting.
- **PWA:** app icon and favicon, screen kept on while visible, portrait on Android.

## Next phases (in order)

Each item is one self-contained step. Order within a phase is the intended build order.

### Live queue

Read-only: no reordering, adding, removing or consume mode. The only way the queue changes from lyrica is loading a stored playlist (next phase). Design: `architecture.md` → "Queue" (server) and "Live queue" (frontend).

**Hard requirement:** the queue must not make anything slower. Now-playing updates, controls, the mode switch and scrolling stay as fast as they are now. Every queue operation, server and client, runs in the background, on demand and in small pieces; every step checks this in headless Chromium against the real queue (`mpc` play/pause/next to DOM within today's 2–4 ms while the list loads and scrolls, no long tasks during a fling, no dropped frames in the mode switch).

1. **Queue reads in `lyrica-mpd`:** `Status` gains `playlist` (version), `playlistlength`, `song` and `songid`; `playlistinfo START:END` returns entries with position, id and the tags the list shows; `playid`. Every `Client` sets `TCP_QUICKACK` before each read (Linux), against the 40 ms delayed-ACK stalls. Measure window latency with the Rust client against the real queue, with and without it.
2. **Queue fields in the state message:** version, length, current position and id; the idle loop also waits on `playlist`.
3. **Queue windows over the WS:** a third MPD connection serving window requests from all clients; the request / reply messages with a per-request size cap; the WS loop hands requests off without awaiting and prefers state pushes over window replies; `TCP_NODELAY` on accepted HTTP / WS connections. Tested with the fake MPD: a slow window never delays a state push or a command.
4. **Mini layout:** the playlist area becomes the toolbar (empty, fixed height) above the queue's scroll container; the mini player stays at the bottom.
5. **Virtual list:** a bounded scroll space mapped onto the queue through an anchor, re-anchored when scrolling stops, so the queue length has no height limit; only rows near the viewport in the DOM, recycled; empty rows where nothing is loaded; queue data in its own store, not in `App`'s snapshot state, so state pushes don't re-render the list.
6. **On-demand loading:** blocks requested only while the queue is shown, nearest the viewport first, at most two in flight; far blocks dropped; replies for an older version dropped.
7. **Auto-centre and highlight:** opens centred on the current song, its row in an accent colour, and re-centres when it changes. Scrolling by the user pauses it; 15 s after scrolling stops it resumes and re-centres.
8. **Auto-update:** a new queue version refetches the visible blocks, keeping old rows shown until replaced, and drops the rest.
9. **Album grouping:** adjacent entries from the same folder and album form a group with a small separator, row heights unchanged.
10. **Lazy cover art per group:** one cover per group from the HTTP mirror, probed only for groups visible once scrolling settles, sharing the now-playing cover cache.
11. **Tap to play:** tapping a row plays it (`playid`).
12. **Background preloading (conditional):** in full mode, keep the blocks around the current song loaded and current, and their covers probed, at idle priority, so switching to mini shows a filled list at once. Kept only if the hard-requirement checks show no measurable impact; otherwise removed.

### Stored playlists

A button in the mini-mode toolbar opens the list; there is no general settings menu. Listings go over the queue connection.

1. **Playlist protocol in `lyrica-mpd`:** `listplaylists`, `clear`, `load`.
2. **Playlists over the WS:** list request and reply; a load command that clears the queue and loads the playlist.
3. **Playlists button and list:** a toolbar button opens a list of stored playlists; tapping one replaces the queue.

## Later (to design)

Ideas to revisit once the phases above are done; how they look and work is open.

- **Queue actions:** convenience buttons in the mini-mode toolbar. Example: play the current album from the start — jump to the first track of the current song's album in the queue, play it, and turn random off (done by hand today). The set of actions and how to find album boundaries without fetching the whole queue are open.
