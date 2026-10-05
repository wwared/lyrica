# Roadmap

Design details live in `architecture.md`.

## Current state

All features below are done and checked on phones, except the volume pill, the queue marquees and the toolbar's even spacing, not yet tried by hand.

- **Foundation:** Nix flake (dev shell, build, fmt, check, NixOS module); `lyrica-mpd` with the commands lyrica needs; server bridging WebSocket clients to MPD (idle, command and queue connections, reconnect with backoff, offline state, stored playlist listing and loading, album start, queue search); frontend embedded in the binary.
- **Now playing:** title, artist (album artist as fallback), album, album art from the HTTP mirror, smooth progress bar with the time beside it, tap / drag seeking on the bar (full mode), previous / play-pause / next, random / single / repeat, MPD volume in a vertical pill growing up from its toggle, bouncing marquee for long lines, fixed-height layout pinned to the bottom, animated full / mini mode switch.
- **Live queue (mini mode):** the whole queue in a virtual list of unbounded length under the toolbar; rows (track number, title, artist · album, duration) on alternating backgrounds, long titles and artist · album lines bouncing in marquees while on screen, loaded on demand while shown, two lists ahead on each side; a separator at each album change and the album's cover beside its top visible row, pinned while the album scrolls, probed two lists ahead, also while scrolling; centred on the current song when opened, on each song change (paused for 15 s after the user scrolls) and on a tap on now playing's cover; the current song is highlighted; the visible rows keep their old text across a queue change until their refetch lands; tapping a row plays it. Nothing loads for it in full mode.
- **Stored playlists:** a toolbar button lists them afresh on each tap (a toast when there are none or listing failed) in a modal; "Load" replaces the queue, with a modal "Loading…" box (seen only after 300 ms) until MPD is done, then the list at its top, or MPD's error in a small modal.
- **Album from the start:** a toolbar button, its last, plays the current song's album from its first entry with random and single off (repeat and the queue untouched) and centres the list on it.
- **Search mode:** a toolbar button turns the toolbar into a search field with a clear button; the query language (words, quoted text, `^`, `-`, `/regex/`, `field:`) finds matches in the queue, the first after the current song centred, ▲▼ in a pill over the list's bottom step through them (wrapping), with their position and count, matches marked in the list; auto-centring held meanwhile; leaving by ✕, Escape or full mode.
- **Player toggles in mini mode:** play here, volume, random, single and repeat, in the toolbar's evenly spaced row between the playlists and album buttons; the volume pill grows down from there, the same size as in full mode.
- **Play here:** plays MPD's `httpd` stream in the browser, with title, artist, album and cover on the lock screen.
- **iOS fit:** fixed viewport (no bounce, no zoom, no selection), full height in home-screen apps, safe areas, content below the system fade at the top of home-screen apps.
- **Dark only:** gruvbox dark with its accent colours, regardless of the system setting.
- **PWA:** app icon and favicon, screen kept on while visible, portrait on Android.

## Open work

None planned. New work lands in small steps, each usable on its own, with tests, measurements and docs.
