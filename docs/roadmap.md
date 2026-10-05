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
3. **Server: `{"type":"albumstart"}`.** In `crates/lyrica/src/bridge.rs`, on the command connection, in order with the other commands (a `next` sent just before is seen) and inside the retry helper (`execute`; a retry reruns it whole, fresh `status` included):
   - **Wiring:** `Command::AlbumStart` (serde gives `albumstart`; add it to `Command`'s doc comment) → `Action::AlbumStart` in `Action::new`, so it's dropped offline like the others. `Action::run` gains `music_url: Option<&str>` for it. `execute_commands` gains `music_url: Option<String>`, the same value `serve_reads` gets: `same_group` compares `folder`, set only with `--music-url`, so the walk must convert entries exactly as windows do. Callers: `main.rs` (`music_url.clone()`), the loads test (`None`).
   - **`async fn album_start(client, music_url) -> Result<()>`:** `status`; no `song`: nothing (`Ok`). Otherwise `group_start`, then `client.play_in_order(id)` with `None` when the first entry is the current one and `state` is `Play` (it carries on), else `Some(first id)` (`playid` starts it from 0:00, also when it's the current one paused or stopped: measured, see `architecture.md` → "Command lists"). Both modes always go, whatever their state: a no-op costs nothing, and the frontend re-centres locally even without a push.
   - **`async fn group_start(client, music_url, position) -> Result<(u32, u32)>`:** the position and id of the first entry of `position`'s group. Reads `queue_range(start..end)` backwards in windows of 100 (a const), the first ending at `position + 1`; converts with `Entry::new`; compares neighbours with `same_group`, carrying the window's first entry to compare with the previous window's last. Stops at the first non-match or at position 0. Typically one window; worst case, one group spanning the queue, ~1,000 windows (~0.8 s); no cap. A missing `Id` (MPD always sends it) is `Error::Protocol`.
   - **Races:** no reply; the state push confirms. A queue changed between `status` and the reads may `ACK` (logged by `execute_commands`) or pick another group; `playid` is by id, so it never plays an entry that's gone.
   - **Tests** (bridge `tests`, fake MPD via `accept`/`reply`; `playlistinfo` replies generated, e.g. `file: a/N.flac\nAlbum: x\nPos: N\nId: N\n` per entry; command lists read line by line like the loads test):
     - `group_start`: the group starts at 0; exactly at a window's first entry (needs the next window to know: current 150, start 51); the current entry is first (one window); spanning three windows; current `album` `None` (its own group).
     - `album_start`: the lists sent for the first entry playing (no `playid`), first entry paused, first entry stopped, another entry playing (with `playid` and the first id); no current song: only `status`.
     - `Command` parses `{"type":"albumstart"}`.
   - **Smoke** on the real MPD with a debug build and a throwaway WS client (allowed: playback, random, single): mid-album with random and single on, then on the first entry playing, then paused; `status` after each.
   - **Docs** (`architecture.md`): WS protocol (client → server list), a "Queue actions" section (the rule, the walk, the races), the smoke results under "Measurements".
4. **Active song changes re-centre.** In mini mode, taps on previous and next call `store.recentre()` after sending, like the cover tap: the pause ends, a fling halts, the current song centres at once, and the push's song change centres the new one. Natural song changes keep respecting the pause. Full mode: nothing (opening centres).
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
