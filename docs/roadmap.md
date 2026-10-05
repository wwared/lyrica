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
2. **`lyrica-mpd`: one command list.** In `crates/mpd/src/client.rs`:
   - `pub async fn play_in_order(&mut self, id: Option<u32>) -> Result<()>`: one `run_list` of `random "0"`, `single "0"`, then `playid "<id>"` when `id` is `Some`. Doc comment: turns random and single off, then plays entry `id` from its start; not a transaction (a gone id fails at `ACK` index 2, the modes stay off).
   - Built like `replace_queue`; the id's string as in `play_id`. Not wired to the server yet (step 3).
   - Test with `fake`, like `replace_queue`'s: both forms, then `playid` rejected (`ACK [50@2] {playid} No such song` → `Error::Ack` with that index) and a `ping` still answered.
   - Smoke: a throwaway program (not committed) calling it on the real MPD, with and without an id (both allowed: modes and playback); `status` then shows random and single off, and the id playing from 0:00.
   - Docs (`architecture.md` → "`lyrica-mpd`"): the commands in "Currently" and "Command lists".
3. **Server: `{"type":"albumstart"}`.** A `Command` on the command connection, in order with the others (a `next` sent just before is seen), inside the retry helper:
   - Fresh `status`: play state, current position. No current song: nothing.
   - Walk back from the current position in windows of 100, converting with `Entry::new` and comparing with `same_group`, to the group's first entry. Typically one window; worst case, one group spanning the queue, ~1,000 windows (~0.8 s); no cap.
   - Current song is the first entry and playing: send `random 0` and `single 0`, so it carries on. Otherwise (another entry, or the first one paused or stopped): `random 0`, `single 0` and `playid` with the first entry's id, which starts it from the beginning. Always both mode commands, whatever their state: a no-op costs nothing, and the list re-centres locally even without a push.
   - No reply: the state push confirms. By id, so a queue replaced meanwhile makes MPD reject it (logged).
   - Tests: the walk with a fake window source (group starting at 0, on a window edge, current entry first, spanning windows, `album` `None`); the commands sent: first entry playing, first entry paused, first entry stopped, another entry.
   - Docs (`architecture.md`): WS protocol, a "Queue actions" section.
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
