# Roadmap

Work proceeds bottom-up in small steps, each usable on its own. Design details live in `architecture.md`.

## Current state

Now playing is done, including seeking on the bar in full mode. The queue area above it (mini mode) has the toolbar and the list's scroll container, both empty; there are no stored playlists.

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

**Hard requirement:** the queue must not make anything slower. Now-playing updates, controls, the mode switch and scrolling stay as fast as they are now. Every queue operation, server and client, runs in the background, on demand and in small pieces; every step checks this in headless Chromium against the real queue: after MPD's own `idle` event, lyrica adds at most ~2–3 ms until the DOM changes, while the list loads and scrolls (measured from the event because `next` takes MPD 3–237 ms to open the next file); `mpc` pause/play within 2–4 ms end to end; no long tasks during a fling; no dropped frames in the mode switch.

1. **Queue reads in `lyrica-mpd`** — done. `Status` has the queue version, length, current position and id; `queue_range` (`playlistinfo START:END`) returns `Song`s with position, id, title, artist, album artist, album, track, duration and file; `play_id`. Every `Client` re-arms `TCP_QUICKACK` before each read: 100-entry windows in 0.43 ms median instead of 41 ms for most (`architecture.md` → "Queue measurements").
2. **Queue fields in the state message** — done. Version, length, current position and id (`queue` object, `null` offline); the idle loop also waits on the queue subsystem (`playlist`); the frontend's `Snapshot` type has the field, nothing renders it yet. Headless Chromium, `mpc` to DOM against the previous build, alternating: pause / play 1.5–2.3 ms either way; for `next`, MPD's own `idle` event comes 3–237 ms after the command (opening the next file), and the DOM changes 1.0–2.3 ms after it, one outlier of 12.8 ms in 32 (before: 1.1–2.3 ms in 32).
3. **Queue windows over the WS** — done. A third MPD connection serves window requests from all clients (`{"type":"queue","version","start","end"}` → `{"type":"queue","version","start","songs"}`), capped at 200 entries, `[]` when MPD rejects the range, `null` when not served; the WS loop hands requests off without awaiting and prefers state pushes over window replies; `TCP_NODELAY` on accepted HTTP / WS connections. Server only: the frontend ignores `queue` messages until the list uses them. Tested with the fake MPD: a stalled window read never delays a state push or a command on the same WS connection. Against the real MPD, with a separate client fetching windows back to back (~2000/s, two in flight), lyrica adds 1.2–2.3 ms after MPD's `idle` event to the DOM (unloaded: 0.2–1.2 ms); most of the increase is MPD contention, the same load sent straight to MPD costs nearly as much (`architecture.md` → "Queue measurements").
4. **Mini layout** — done. The queue area (`.queue`) is a flex column: the toolbar (`.toolbar`, empty, 44 px, `flex: none`) above the list's scroll container (`.queue-list`, `flex: 1`, `min-height: 0`, `overflow-y: auto`, `overscroll-behavior: contain`), empty until the virtual list; the "Queue" placeholder text is gone. Geometry identical in both modes and to the previous build; injected rows scroll inside the list, never the page; mode switches unchanged (no long tasks); pause / play unchanged (`architecture.md` → "Queue layout").
5. **Virtual list:** `queue.tsx` renders the list over a bounded scroll space (18,000 rows of 56 px) mapped onto the queue through an anchor, re-anchored when scrolling stops and the viewport has left the space's middle half, so the queue length has no height limit; a recycled pool of the rows in view ± 15; every row empty (nothing loads until step 6), so the list is blank but sized and scrollable; opens at the top. Only the queue length goes in, via a `useMemo`'d element, so state pushes don't re-render it. Pure arithmetic (space size, anchor clamping, rendered range, re-anchor keeping the on-screen offset, length changes) in `virtual.ts` with Vitest tests at the boundaries (length 0, < and > the space, queue ends, shrinking). Checked in headless Chromium against the real queue (175,872): pool size constant while scrolling; rendered rows cover the viewport at every frame of a fling; sizer ≤ 1,008,000 px; a re-anchor leaves the top row's `aria-posinset` and screen offset unchanged; repeated scrolls reach the last position (bottom row = length, flush with the list's bottom) and back to 1; a state push (play / pause, elapsed) causes no mutation in the list; no long tasks during a fling; mode switch frames and `mpc` pause/play unchanged against the previous build.
6. **On-demand loading:** `ws.ts` parses `queue` replies into a queue store outside `App`'s state; blocks requested only while the queue is shown, nearest the viewport first, at most two in flight; far blocks dropped; replies for an older version dropped.
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
