# Roadmap

Work proceeds bottom-up, adding features piecemeal once the end-to-end basis works.

## Phases

1. **Scaffolding** — done. Nix flake (dev shell, build, fmt, check), Cargo workspace, Vite/Preact app, frontend embedded and served by the server, placeholder WebSocket connection.
2. **`lyrica-mpd` basics** — done. Connect, greeting/version, command/response framing, errors (`ACK`), and the minimum commands for the first milestone (password, status, current song, play/pause, previous, next, idle).
3. **Server + WebSocket** — done. `--mpd`/`--mpd-password`; idle and command connections; JSON WS protocol; reconnect/backoff. Verified against the local MPD (see `architecture.md` → "MPD bridge" for timings) including connection loss via a killed proxy and command-connection timeouts.
4. **Frontend** — next. Design is in `architecture.md` → "Milestone 1 frontend". Steps: protocol types and parsing in `ws.ts` plus reconnect-on-visible; `position()` with Vitest tests; now-playing view with controls; rAF progress bar. Done when, at an iPhone-sized viewport (390×844) against the local MPD: the UI reflects changes made by `mpc` (pause, play, next, seek) within ~100 ms; the buttons work; the bar moves smoothly while playing, stops on pause, jumps on seek and song change; offline is shown when the server or MPD is unreachable (killed `socat` proxy) and clears on recovery; the bar is correct after the page is hidden and shown again. Verify in headless Chromium (see `development.md`), and restore MPD afterwards per `AGENTS.md`.

## Milestone 1 (first deliverable)

End-to-end, bare basics only: the currently playing song title, a smooth progress bar, and basic playback controls (previous, play/pause, next). Nothing more.

## Backlog (after milestone 1, roughly in order)

- Album art and full song info in now playing.
- Volume, random and single toggles.
- Live queue view: windowed loading, re-centering, album grouping, lazy cover art.
- Album art via the HTTP library mirror.
- Settings menu: httpd stream playback, stored playlists (list, clear queue, load).
- PWA polish: icons, offline shell.
