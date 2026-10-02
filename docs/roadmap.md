# Roadmap

Work proceeds bottom-up, adding features piecemeal once the end-to-end basis works.

## Phases

1. **Scaffolding** — done. Nix flake (dev shell, build, fmt, check), Cargo workspace, Vite/Preact app, frontend embedded and served by the server, placeholder WebSocket connection.
2. **`lyrica-mpd` basics** — done. Connect, greeting/version, command/response framing, errors (`ACK`), and the minimum commands for the first milestone (password, status, current song, play/pause, previous, next, idle).
3. **Server + WebSocket** — done. `--mpd`/`--mpd-password`; idle and command connections; JSON WS protocol; reconnect/backoff. Verified against the local MPD (see `architecture.md` → "MPD bridge" for timings) including connection loss via a killed proxy and command-connection timeouts.
4. **Frontend** — done. Now-playing view (title, play state, smooth progress bar, previous / play-pause / next), offline display, reconnect on page show. Verified in headless Chromium at 390×844 against the local MPD (see `architecture.md` → "Milestone 1 frontend" for results).

## Milestone 1 (first deliverable) — done

End-to-end, bare basics only: the currently playing song title, a smooth progress bar, and basic playback controls (previous, play/pause, next). Nothing more.

## Backlog (after milestone 1, roughly in order)

- Album art and full song info in now playing.
- Volume, random and single toggles.
- Live queue view: windowed loading, re-centering, album grouping, lazy cover art.
- Album art via the HTTP library mirror.
- Settings menu: httpd stream playback, stored playlists (list, clear queue, load).
- PWA polish: icons, offline shell.
