# Roadmap

Work proceeds bottom-up in small steps, each usable on its own. Design details live in `architecture.md`.

## Current state

Now playing and the live queue are done and checked on an iPhone. There are no stored playlists yet.

- **Foundation:** Nix flake (dev shell, build, fmt, check, NixOS module); `lyrica-mpd` with the commands lyrica needs; server bridging WebSocket clients to MPD (idle, command and queue connections, reconnect with backoff, offline state); frontend embedded in the binary.
- **Now playing:** title, artist (album artist as fallback), album, album art from the HTTP mirror, smooth progress bar with the time beside it, tap / drag seeking on the bar (full mode), previous / play-pause / next, random / single / repeat, MPD volume slider, bouncing marquee for long lines, fixed-height layout pinned to the bottom, animated full / mini mode switch.
- **Live queue (mini mode):** the whole queue in a virtual list of unbounded length under an empty toolbar; rows (track number, title, artist · album, duration) on alternating backgrounds, loaded on demand while shown, two lists ahead on each side; a separator at each album change and the album's cover beside its top visible row, pinned while the album scrolls, probed two lists ahead, also while scrolling; centred on the current song when opened and on each song change (paused for 15 s after the user scrolls), which is highlighted; the visible rows keep their old text across a queue change until their refetch lands; tapping a row plays it. Nothing loads for it in full mode.
- **Play here:** plays MPD's `httpd` stream in the browser, with title, artist, album and cover on the lock screen.
- **iOS fit:** fixed viewport (no bounce, no zoom, no selection), full height in home-screen apps, safe areas.
- **Dark only:** gruvbox dark with its accent colours, regardless of the system setting.
- **PWA:** app icon and favicon, screen kept on while visible, portrait on Android.

## Open work

- **Live queue design:** the row layout and album styling are open to iteration from daily use (`architecture.md` → "Rows").

## Next phase: stored playlists

A load button at the left of the mini-mode toolbar lists the server's stored playlists on each tap and opens them in a modal; selecting one and tapping "Load" closes it and replaces the queue, with a "Loading…" box until MPD is done (`architecture.md` → "Stored playlists"). No general settings menu. Loading never touches the real MPD's queue during development: throwaway MPD or scripted fake (`development.md`). Every step keeps the responsiveness checks (`architecture.md` → "Responsiveness").

1. **Playlist protocol in `lyrica-mpd`:** done. `Client::playlists()` and `Client::replace_queue(name)` (`clear` + `load` in one private command list); measurements and failure modes in `architecture.md` → "Queue measurements". Main finding: a load blocks MPD for every client while it runs, up to ~20 s for the real MPD's large playlists (estimated from `playlistlength`).
2. **Playlists over the WS:**
   - **Listing:** `{"type":"playlists"}` → `{"type":"playlists","names":[…]|null}`, to that client alone. The queue task's channel carries windows or listing requests, each with the connection's reply sender; `try_send`, full: `null` at once, so the WS loop never waits. `playlists()` on the queue connection through `execute`, sorted. `null` also when offline (snapshot, no MPD round trip) or on an MPD error.
   - **Loading:** `{"type":"load","name":s}` → `{"type":"loaded","name":s,"error":s|null}`, to that client alone, once MPD answers. The command task's channel carries commands or loads (a load with its reply sender; `Command` stays `Copy`); loads go with `try_send`, full: error `"Busy"` at once. On the command connection through `execute`: `playlists()` first, a name not listed → `"No such playlist"` without touching the queue; then `replace_queue`. Offline → `"MPD offline"` at once; `ACK` → its message; other errors → their text.
   - **During a load** every MPD connection waits, so commands from any client queue behind it, and the WS loop's `commands.send().await` blocks once 16 are queued. Accepted: no state can arrive meanwhile anyway, and the loading client's modal box (step 3) stops its taps.
   - **Rust tests:** parsing (`playlists`; `load` with a name; `load` without one ignored like other invalid messages); listing sorted, `null` offline without MPD; load success (exact MPD lines: `listplaylists`, then the command list), a name not listed (no command list sent), `ACK [50@1]` → its message, offline → error at once.
   - **Smoke:** a Bun WS client against lyrica on the throwaway MPD (`development.md`): listing; a load of each size, and of the 4,000-directory playlist (seconds, like the real MPD); a second WS client getting one state push with the new length after the load; a missing name; an over-limit playlist.
   - **Docs:** `architecture.md` → "MPD bridge" (both messages, which connection serves each).
3. **Load button and modal:** the toolbar button (disabled only while offline, like the other controls); on tap a listing request, then the `<dialog>` with the names (each a `Marquee`), or a "No playlists found" toast for 3 s; selection, Load, close by X, backdrop or Escape; after Load the dialog closes and a "Loading…" box shows until the reply, replaced on error by a small modal with MPD's message and OK; the list jumping to the top after a load. Headless Chromium against the throwaway MPD (layout at 390 and 375 px, a long name scrolling, an empty playlist directory, every close path, a load of each size with now playing and the list during it, a failing load, the responsiveness checks); then by hand on the phones.

## Later (to design)

Ideas to revisit once the phases above are done; how they look and work is open.

- **Queue actions:** convenience buttons in the mini-mode toolbar. Example: play the current album from the start — jump to the first track of the current song's album in the queue, play it, and turn random off (done by hand today). The set of actions and how to find album boundaries without fetching the whole queue are open.
