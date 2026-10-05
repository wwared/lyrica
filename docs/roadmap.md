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
6. **Done: smoke against the real MPD.** Paused first entry, a 1,163-entry group (12 windows), groups starting at block edges, scrolled away, mid-fling, a natural song change while paused: all as designed (`architecture.md` → "Measurements" → "Queue actions, more cases"). It found one bug, fixed in its own commit: in Chromium a mid-fling album, previous, next or cover tap could leave the list 26–28 px off centre for good; now every pass re-centres from the halt until the list settles (`architecture.md` → "Cover, previous and next taps").
7. **iPhone** (the user, home-screen app on the LAN, mini mode):
   - the album button at the toolbar's right, its icon lined up with the durations' right edge, muted like the playlists button; the pressed look; dimmed offline;
   - mid-album with random and single on: a tap plays the album's first entry from 0:00, both modes off, the list centred on it;
   - on the first entry, playing: a tap turns the modes off without restarting the song;
   - during a fling, one tap on the album button, previous, next or the cover: it acts (in Chromium that touch only stops the fling, with no `click`; iOS unchecked), the fling stops, the list rests centred, not a row's half off;
   - a swipe right after any of these taps follows the finger.

   Results go to `architecture.md` → "Measurements", as an "**Album button by hand**" paragraph after "Pinned covers by hand". If a tap during a fling only stops it, that's a design question (the toolbar is outside the list), not part of this milestone.

8. **Final docs pass** (one commit, no code):
   - "Current state": the opening line becomes "Now playing, the live queue, stored playlists and the album button are done and checked on phones."; after "Stored playlists", a bullet "**Album from the start:** a toolbar button, at its right, plays the current song's album group from its first entry with random and single off (repeat and the queue untouched) and centres it.";
   - remove this milestone section;
   - `architecture.md` already describes the feature as built, with no milestone or "planned" mentions left for it; only the iPhone paragraph above lands there;
   - the next milestone is the user's pick from "Open work" and "Later".

## Open work

- **Live queue design:** the row layout and album styling are open to iteration from daily use (`architecture.md` → "Rows").

## Later (to design)

Ideas to revisit; how they look and work is open.

- **More queue actions:** further toolbar buttons beside the album one; which ones is open.
