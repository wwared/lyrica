# Roadmap

Work proceeds bottom-up in small steps, each usable on its own. Design details live in `architecture.md`.

## Current state

Now playing, the live queue and stored playlists are done and checked on phones.

- **Foundation:** Nix flake (dev shell, build, fmt, check, NixOS module); `lyrica-mpd` with the commands lyrica needs; server bridging WebSocket clients to MPD (idle, command and queue connections, reconnect with backoff, offline state, stored playlist listing and loading); frontend embedded in the binary.
- **Now playing:** title, artist (album artist as fallback), album, album art from the HTTP mirror, smooth progress bar with the time beside it, tap / drag seeking on the bar (full mode), previous / play-pause / next, random / single / repeat, MPD volume slider, bouncing marquee for long lines, fixed-height layout pinned to the bottom, animated full / mini mode switch.
- **Live queue (mini mode):** the whole queue in a virtual list of unbounded length under the toolbar; rows (track number, title, artist · album, duration) on alternating backgrounds, loaded on demand while shown, two lists ahead on each side; a separator at each album change and the album's cover beside its top visible row, pinned while the album scrolls, probed two lists ahead, also while scrolling; centred on the current song when opened, on each song change (paused for 15 s after the user scrolls) and on a tap on now playing's cover; the current song is highlighted; the visible rows keep their old text across a queue change until their refetch lands; tapping a row plays it. Nothing loads for it in full mode.
- **Stored playlists:** a toolbar button lists them afresh on each tap (a toast when there are none or listing failed) in a modal; "Load" replaces the queue, with a modal "Loading…" box (seen only after 300 ms) until MPD is done, then the list at its top, or MPD's error in a small modal.
- **Play here:** plays MPD's `httpd` stream in the browser, with title, artist, album and cover on the lock screen.
- **iOS fit:** fixed viewport (no bounce, no zoom, no selection), full height in home-screen apps, safe areas.
- **Dark only:** gruvbox dark with its accent colours, regardless of the system setting.
- **PWA:** app icon and favicon, screen kept on while visible, portrait on Android.

## Current milestone: album from the start

A toolbar button, at its right, that plays the current song's album from its first entry in the queue with random and single off (done by hand today). Repeat is left alone; the queue isn't changed. One commit per step; docs move with each.

1. **Done: grouping rule on the server, one copy.** `same_group` in `bridge.rs` sets each window entry's `start` flag, read with the entry before the window; the frontend requests exact blocks and only reads the flag (`architecture.md` → "Album grouping").
2. **Done: `lyrica-mpd` command list.** `Client::play_in_order(id: Option<u32>)`: `random 0`, `single 0`, then `playid` when given an id, in one command list (`architecture.md` → "Command lists").
3. **Done: server, `{"type":"albumstart"}`.** On the command connection: `status`, then `group_start` walks back from the current song in windows of 100 with `same_group`, then `play_in_order`, leaving out `playid` when the first entry is already playing (`architecture.md` → "Queue actions").
4. **Previous / next re-centre.** Frontend only, no server change. Today only the cover tap and a tap on the current row re-centre at once; previous / next while the list is paused (15 s after a scroll) leave it where it is.
   - **Change** (`web/src/app.tsx`, the transport's previous and next buttons): send, then `if (mini) store.recentre()`, like the cover's `onClick`. `recentre` (the list's listener in `queue.tsx`) halts a running fling (`overflow-y: hidden` for two frames), ends the pause and its timer, and centres the current song at once; the push's song change then centres the new one through the `current` pass, since nothing is paused any more. In random mode the old song is centred first and the far-away new one a push later (`next` 19 ms, `previous` 3 ms: "MPD bridge" → "No replies to commands"); accepted, since MPD may reject the command (e.g. `previous` with nothing before) and the list still ends up centred. Play / pause and the mode toggles don't re-centre. Full mode: nothing (the list is hidden; opening centres). Natural song changes keep respecting the pause.
   - **Docs:** `queue-store.ts`'s `recentre` doc comment (now "A tap on the mini player's cover"); `architecture.md` → "Auto-centre" → "Cover tap" becomes "Cover, previous and next taps"; the frontend "Queue actions" bullet drops previous / next from the planned part; this file's "Current state" (live queue: centred on a tap on the cover, previous or next).
   - **Tests:** none new: it only wires the buttons to the cover tap's path. `npm run typecheck`, `npm test`, then the check.
   - **Smoke** (`development.md` → headless Chromium; `npm run build`, debug server, 390×844 touch emulation, mini mode; `next` / `previous` are allowed on the real MPD), recording `scrollTop` and the current row's rect per frame: swipe away (paused), tap next → centred on the new song within a frame of its push, highlight on it; the same for previous; tap next mid-fling → the fling stops, centred; full mode, next → `scrollTop` unchanged, no window request; paused, `mpc next` → not centred until the 15 s resume. Results under `architecture.md` → "Measurements" ("Auto-centre").
5. **The button.** `{type: "albumstart"}` in `ws.ts`'s `Command`. At the toolbar's right, 44 × 44 px, Material's `album` icon, muted like the playlists button, its icon ending at the rows' 20 px right inset. Disabled offline or without a current song. A tap sends and calls `store.recentre()`.
6. **Smoke** against the real MPD (playback control, random and single are allowed):
   - mid-album, random and single on: the first entry plays, both off, one push;
   - on the first entry, playing, random on: random off, the song carries on;
   - on the first entry, paused: it plays from the beginning, random and single off;
   - on the first entry, playing, both off: nothing changes, list centred;
   - an album over 100 entries; one starting at a block edge;
   - scrolled away or flinging, then the button, previous, next: centred;
   - a natural song change while paused: not centred.

   Then on the iPhone; this file's "Current state" gets the feature.

## Open work

- **Live queue design:** the row layout and album styling are open to iteration from daily use (`architecture.md` → "Rows").

## Later (to design)

Ideas to revisit; how they look and work is open.

- **More queue actions:** further toolbar buttons beside the album one; which ones is open.
