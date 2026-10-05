# Roadmap

Work proceeds bottom-up in small steps, each usable on its own. Design details live in `architecture.md`.

## Current state

Now playing, the live queue and stored playlists are done and checked on phones.

- **Foundation:** Nix flake (dev shell, build, fmt, check, NixOS module); `lyrica-mpd` with the commands lyrica needs; server bridging WebSocket clients to MPD (idle, command and queue connections, reconnect with backoff, offline state, stored playlist listing and loading); frontend embedded in the binary.
- **Now playing:** title, artist (album artist as fallback), album, album art from the HTTP mirror, smooth progress bar with the time beside it, tap / drag seeking on the bar (full mode), previous / play-pause / next, random / single / repeat, MPD volume slider, bouncing marquee for long lines, fixed-height layout pinned to the bottom, animated full / mini mode switch.
- **Live queue (mini mode):** the whole queue in a virtual list of unbounded length under the toolbar; rows (track number, title, artist · album, duration) on alternating backgrounds, loaded on demand while shown, two lists ahead on each side; a separator at each album change and the album's cover beside its top visible row, pinned while the album scrolls, probed two lists ahead, also while scrolling; centred on the current song when opened, on each song change (paused for 15 s after the user scrolls) and on a tap on now playing's cover, previous or next; the current song is highlighted; the visible rows keep their old text across a queue change until their refetch lands; tapping a row plays it. Nothing loads for it in full mode.
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
4. **Done: previous / next re-centre.** In mini mode, `App`'s previous and next send, then call `store.recentre()`, like the cover tap; the push's song change then centres the new song (`architecture.md` → "Auto-centre").
5. **Done: the button.** `"albumstart"` in `ws.ts`'s `Command`; in `App`'s toolbar, at its right, Material's `album` icon, disabled offline or without a current song; a tap sends and calls `store.recentre()` (`architecture.md` → "Queue actions").
6. **Smoke, then the iPhone.** No code expected; a bug found becomes its own fix commit before this one. Against the real MPD (playback control, `playid` by tapping a row, random and single are allowed; never the queue).
   - **Harness:** `npm run build`, debug server on 127.0.0.1:8099 with `--music-url https://webmusic.wwared.com/`, headless Chromium per `development.md`, 390 × 844, `mobile: true`, touch emulation. A page script (`Page.addScriptToEvaluateOnNewDocument`) wraps `WebSocket` to log outgoing commands and incoming `state` messages with `performance.now()`, and a `requestAnimationFrame` loop records `.queue-list`'s `scrollTop` and `.queue-row.current`'s offset from the list's centre. Selectors: `[aria-label=Queue]` (to mini), `.album-start`, `[aria-label=Previous]`, `[aria-label=Next]`. Swipes and flings: `Input.dispatchTouchEvent` at x 195, starting inside the list (y 480). A reload needs a changed URL (`/?n`): a hash-only `Page.navigate` doesn't reload. Set modes with `mpc random on` / `mpc single on`, pause with `mpc pause`, move with `mpc next` or a row tap.
   - **Already measured in step 5** (`architecture.md` → "Measurements" → "Queue actions"): mid-album with both modes on; a first entry playing with both on; a first entry playing with both off; disabled states. Not repeated unless something changed.
   - **To do, each recording sent commands, pushes, `mpc status` before and after, and frames:**
     - on a first entry, paused (`mpc pause`), random and single on: plays from 0:00, both off, one push, centred;
     - an album over 100 entries, tapped near its end: the walk crosses windows; the group's first entry plays. Find one, and a group starting exactly at a block edge (position a multiple of 100), with a read-only scan: a Bun WS client requesting every block (`{"type":"queue",…}`, as in `ws.ts`) and reading the `start` flags, 1,759 blocks;
     - a group starting at a block edge, tapped from its second entry: that edge entry plays;
     - mid-fling (as step 4: fling from y 480, tap 120 ms in), the album button: the fling stops, the first entry centred; then previous and next the same way;
     - centring paused by a swipe, then a natural song change (`mpc seek` to 3 s before the end, then wait): not centred until 15 s after the swipe.
   - **Results:** append to `architecture.md` → "Measurements" → "Queue actions".
   - **iPhone** (the user, home-screen app on the LAN; give them a checklist): the button's place and look beside the playlists button, the pressed and disabled looks; a tap mid-album plays the first entry with both modes off and centres it; the button, previous, next and a cover tap mid-fling stop the fling and centre (in Chromium a cover tap mid-fling was left ~28 px off, `architecture.md` → "Previous / next re-centre"; on iOS unchecked).
   - **Then:** "Current state" gets the feature (a toolbar button bullet, and the live-queue line's re-centring), the milestone section is removed and "Open work" picks the next one.

## Open work

- **Live queue design:** the row layout and album styling are open to iteration from daily use (`architecture.md` → "Rows").

## Later (to design)

Ideas to revisit; how they look and work is open.

- **More queue actions:** further toolbar buttons beside the album one; which ones is open.
