# Roadmap

Work proceeds bottom-up in small steps, each usable on its own. Design details live in `architecture.md`.

## Current state

Now playing is done apart from seeking. The queue area above it (mini mode) is an empty placeholder, there are no stored playlists, and the PWA has no icon.

- **Foundation:** Nix flake (dev shell, build, fmt, check, NixOS module); `lyrica-mpd` with the commands now playing needs; server bridging WebSocket clients to MPD (idle and command connections, reconnect with backoff, offline state); frontend embedded in the binary.
- **Now playing:** title, artist (album artist as fallback), album, album art from the HTTP mirror, smooth progress bar with the time beside it, previous / play-pause / next, random / single / repeat, MPD volume slider, bouncing marquee for long lines, fixed-height layout pinned to the bottom, animated full / mini mode switch.
- **Play here:** plays MPD's `httpd` stream in the browser, with title, artist, album and cover on the lock screen.
- **iOS fit:** fixed viewport (no bounce, no zoom, no selection), full height in home-screen apps, safe areas, light / dark mode.

## Next phases (in order)

Each item is one self-contained step. Order within a phase is the intended build order.

### Seeking

1. **Seek protocol:** `seekcur` in `lyrica-mpd` (absolute and relative); `seek` (to a position) and relative seek commands in the WS protocol.
2. **Seek on the progress bar:** tap or drag anywhere on the bar sets the position, with a touch target taller than the bar; the bar follows the finger while dragging and seeks on release.
3. **Seek buttons:** backward / forward by a fixed step in the control strip. Step size and placement to decide (at 375 px the strip has no free room beside the transport).

### Live queue

Read-only: no reordering, adding, removing or consume mode. The only way the queue changes from lyrica is loading a stored playlist (next phase).

1. **Queue protocol in `lyrica-mpd`:** `playlistinfo` for a position range, with the per-track fields the list shows (title, artist, album artist, album, track number, duration, id).
2. **Queue position in the state message:** current song position and id, queue length and queue version.
3. **Queue windows over the WS:** the client requests a position range; the server answers with those entries. Fetched off the idle connection so now playing never waits on it.
4. **Virtual list:** the queue area becomes its own scroll container sized to the whole queue, rendering only the rows near the viewport, fixed row height.
5. **Lazy loading while scrolling:** windows load ahead in both directions with enough margin that scrolling never shows missing rows; far-away windows are dropped.
6. **Auto-centre:** opens centred on the current song and re-centres when it changes; leaves the user's scroll alone while they are scrolling.
7. **Auto-update:** idle on `playlist`; a new queue version invalidates loaded windows and reloads the visible ones.
8. **Album grouping:** adjacent tracks from the same album form a group with a small separator.
9. **Lazy cover art per group:** one cover per album group from the HTTP mirror, loaded only when visible, sharing the now-playing cover cache.
10. **Tap to play:** tapping a track plays it (`playid`).

### Stored playlists

A dedicated button opens the list; there is no general settings menu.

1. **Playlist protocol in `lyrica-mpd`:** `listplaylists`, `clear`, `load`.
2. **Playlists over the WS:** list request and reply; a load command that clears the queue and loads the playlist.
3. **Playlists button and list:** opens a list of stored playlists; tapping one replaces the queue.

### PWA polish

There is no app icon or favicon yet. No service worker or offline shell (caching the app so it launches without a network): it is useless without the server and MPD anyway.

1. **Icon artwork:** one design, exported at the sizes the manifest and iOS need.
2. **Icon wiring:** manifest `icons`, `apple-touch-icon` and favicon.
3. **Launch colour:** the manifest's `background_color` is fixed to the dark background; check what iOS shows while launching in light mode and match it.
