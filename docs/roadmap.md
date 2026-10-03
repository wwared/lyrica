# Roadmap

Work proceeds bottom-up, adding features piecemeal once the end-to-end basis works.

## Phases

1. **Scaffolding** — done. Nix flake (dev shell, build, fmt, check), Cargo workspace, Vite/Preact app, frontend embedded and served by the server, placeholder WebSocket connection.
2. **`lyrica-mpd` basics** — done. Connect, greeting/version, command/response framing, errors (`ACK`), and the minimum commands for the first milestone (password, status, current song, play/pause, previous, next, idle).
3. **Server + WebSocket** — done. `--mpd`/`--mpd-password`; idle and command connections; JSON WS protocol; reconnect/backoff. Verified against the local MPD (see `architecture.md` → "MPD bridge" for timings) including connection loss via a killed proxy and command-connection timeouts.
4. **Frontend** — done. Now-playing view (title, play state, smooth progress bar, previous / play-pause / next), offline display, reconnect on page show. Verified in headless Chromium at 390×844 against the local MPD (see `architecture.md` → "Now playing" for results).

## Milestone 1 (first deliverable) — done

End-to-end, bare basics only: the currently playing song title, a smooth progress bar, and basic playback controls (previous, play/pause, next). Nothing more.

## After milestone 1

Done: now playing pinned to the bottom with fixed-height lines; artist line; bouncing marquee for long text; Iosevka Aile font; album art (`cover.*` in the song's folder on the HTTP mirror, via `--music-url`; empty while loading); random / single / repeat toggles; animated full / mini mode switch (queue still a placeholder); play-here toggle for MPD's `httpd` stream (via `--stream-url`). Details in `architecture.md` → "Now playing".

## Backlog (roughly in order)

- More song info in now playing (album, …).
- Volume.
- Live queue view: windowed loading, re-centering, album grouping, lazy cover art. Own scroll container with `overscroll-behavior: contain` (the page is fixed; see `architecture.md` → "Fixed viewport").
- Album art beyond the song's own folder on the HTTP mirror.
- Settings menu: stored playlists (list, clear queue, load).
- PWA polish: icons, offline shell.
