# Roadmap

Work proceeds bottom-up in small steps, each usable on its own. Design details live in `architecture.md`.

## Current state

Now playing and the live queue are done; the live queue's phone checks and design refinements are open (see "Open work"). There are no stored playlists yet.

- **Foundation:** Nix flake (dev shell, build, fmt, check, NixOS module); `lyrica-mpd` with the commands lyrica needs; server bridging WebSocket clients to MPD (idle, command and queue connections, reconnect with backoff, offline state); frontend embedded in the binary.
- **Now playing:** title, artist (album artist as fallback), album, album art from the HTTP mirror, smooth progress bar with the time beside it, tap / drag seeking on the bar (full mode), previous / play-pause / next, random / single / repeat, MPD volume slider, bouncing marquee for long lines, fixed-height layout pinned to the bottom, animated full / mini mode switch.
- **Live queue (mini mode):** the whole queue in a virtual list of unbounded length under an empty toolbar; rows (track number, title, artist · album, duration) loaded on demand while shown, two lists ahead on each side; a separator at each album change and the album's cover on its first row, probed two lists ahead, also while scrolling; centred on the current song when opened and on each song change (paused for 15 s after the user scrolls), which is highlighted; the visible rows keep their old text across a queue change until their refetch lands; tapping a row plays it. Nothing loads for it in full mode.
- **Play here:** plays MPD's `httpd` stream in the browser, with title, artist, album and cover on the lock screen.
- **iOS fit:** fixed viewport (no bounce, no zoom, no selection), full height in home-screen apps, safe areas.
- **Dark only:** gruvbox dark with its accent colours, regardless of the system setting.
- **PWA:** app icon and favicon, screen kept on while visible, portrait on Android.

## Open work

### Live queue phone checks

What headless Chromium can't check, to do on an iPhone (home-screen app):

- **Taps:** a tap plays the row; a tap that stops a fling doesn't; no grey tap flash.
- **Look:** the scroll indicator stays hidden; the album separator is visible but subtle; the song-change jump and the fade-in's first frame look right.
- **Covers while scrolling:** covers are already showing as rows scroll into view during ordinary drags and flings, during a hard fling's tail and after it stops; no stutter while they load, also flinging past the biggest covers (13–14 MB, near queue positions 137,507 and 137,526). If they pop in after hard flings, raise `AHEAD_ROWS_PER_S` (100 rows/s; headless touch flings peak at 67–105 rows/s, so iOS speeds are unmeasured). If big covers stutter, consider server-side thumbnails (`architecture.md` → "Image sizes").
- **Re-anchoring:** one hard fling never reaches the scroll space's edge before scrolling stops (re-anchoring waits for the stop; `architecture.md` → "Re-anchoring").

### Live queue design refinements

The row layout and album styling are open to iteration from daily use (`architecture.md` → "Rows").

## Next phase: stored playlists

A button in the mini-mode toolbar opens the list; there is no general settings menu. Listings go over the queue connection. Loading replaces the queue in one version change, so the list's auto-update shows the new queue without an empty step (`architecture.md` → "Auto-update"). Every step keeps the responsiveness checks (`architecture.md` → "Responsiveness").

1. **Playlist protocol in `lyrica-mpd`:** `listplaylists`, `clear`, `load`, and command lists, so `clear` and `load` run as one.
2. **Playlists over the WS:** list request and reply; a load command that clears the queue and loads the playlist.
3. **Playlists button and list:** a toolbar button opens a list of stored playlists; tapping one replaces the queue.

## Later (to design)

Ideas to revisit once the phases above are done; how they look and work is open.

- **Queue actions:** convenience buttons in the mini-mode toolbar. Example: play the current album from the start — jump to the first track of the current song's album in the queue, play it, and turn random off (done by hand today). The set of actions and how to find album boundaries without fetching the whole queue are open.
