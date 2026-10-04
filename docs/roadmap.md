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

A load button at the left of the mini-mode toolbar opens a modal list of the server's stored playlists; selecting one and tapping "Load" replaces the queue with it (`architecture.md` → "Stored playlists"). No general settings menu. Loading never touches the real MPD's queue during development: throwaway MPD or scripted fake (`development.md`). Every step keeps the responsiveness checks (`architecture.md` → "Responsiveness").

1. **Playlist protocol in `lyrica-mpd`:**
   - `Client::playlists() -> Result<Vec<String>>`: `listplaylists`, the `playlist` values in MPD's order (`Last-Modified` ignored).
   - `Client::replace_queue(name: &str) -> Result<()>`: `command_list_begin`, `clear`, `load "name"`, `command_list_end`, sent in one write, read as one response (one `OK`, or the failing command's `ACK`, whose index says which). Command lists go through a private encoder taking `&[(&str, &[&str])]`, reusing `encode`'s quoting and newline check; `run` reads the response as now. No public generic command-list API.
   - Tests (fake server in `client.rs`): both commands' exact bytes and results; a name with quotes and a backslash; an empty listing; `ACK [50@1] {load} No such playlist` → `Error::Ack` with index 1, connection still usable (`ping` after); a newline in the name → `InvalidArgument`, nothing sent.
   - Throwaway MPD (`development.md`), with `.m3u` files of 1,000, 19,000 and 195,000 lines written into its `playlist_directory`: `replace_queue` time per size; whether a second connection's `status` waits during the load; what an `idle playlist` client sees (events, and `status` right after: no length 0 between the old and new queue). Real MPD: `listplaylists` only.
   - Docs: `architecture.md` → `lyrica-mpd` command list, "Not implemented" line, measurements under "Queue measurements".
2. **Playlists over the WS:** the idle task also waits on `stored_playlist`, fetching `listplaylists` on each connect and event into its own `watch` of sorted names (empty while offline); the WS sends `{"type":"playlists","names":[…]}` after `config` and on every change. `{"type":"load","name"}` runs `replace_queue` on the command connection and answers that client alone with `{"type":"loaded","name","error":string|null}` (offline: an error at once). Rust tests: message parsing, names on connect and on change (fake MPD event), the reply on success, `ACK` and offline. Smoke: a Bun WS client against the throwaway MPD.
3. **Load button and modal:** the toolbar button (disabled while there are no names or offline), the `<dialog>` with its list, selection, Load, close by X, backdrop or Escape, the busy and error states, the list jumping to the top after a load. Headless Chromium against the throwaway MPD (layout at 390 and 375 px, every close path, a load of each size with now playing and the list during it, the responsiveness checks); then by hand on the phones.

## Later (to design)

Ideas to revisit once the phases above are done; how they look and work is open.

- **Queue actions:** convenience buttons in the mini-mode toolbar. Example: play the current album from the start — jump to the first track of the current song's album in the queue, play it, and turn random off (done by hand today). The set of actions and how to find album boundaries without fetching the whole queue are open.
