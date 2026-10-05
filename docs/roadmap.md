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

1. **Grouping rule on the server, one copy.** The album walk (step 3) and the list's separators and covers must agree by construction, so the rule moves out of `queue-store.ts`:
   - `same_group(a: &Entry, b: &Entry)` in `bridge.rs`: `album` equal and not `None`, and `folder` equal. On converted entries (`Entry::new`, with its `album()` / `folder()` fallbacks), so the windows and the walk share the conversion too. The only copy.
   - Window entries gain `"start": bool`: position 0, or not in the same group as the entry before. `serve_window` reads `start − 1 … end` (from `start` at 0) in one `playlistinfo` (one queue version) and drops the leading entry; the 200-entry cap counts requested entries. Only the leading entry back (`start` = length): `[]`, like a rejected range.
   - Frontend: a block request is exactly the block (`start` = 100b, no leading entry; the reply's block is `start / 100`); `starts()` returns `entry.start`; the comparison in `queue-store.ts` goes, and so does the leading-entry handling. Old blocks across a version change keep their own flags, so versions still never mix.
   - Tests: flags for windows starting at position 0, at a group start and inside a group; `album` `None`; `folder` `None`. Queue-store tests for the new ranges.
   - Smoke: the "Album grouping" check (separators against `playlistinfo` at 9 spots, including block edges) still matches.
   - Docs (`architecture.md`): "Windows", "Entries", "Blocks", "Album grouping".
2. **`lyrica-mpd`: one command list.** A `Client` method sending `random 0` and `single 0`, then `playid` when given an id, as one command list. Unit test against the mock server.
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
