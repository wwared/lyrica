# Roadmap

Work proceeds bottom-up, adding features piecemeal once the end-to-end basis works.

## Phases

1. **Scaffolding** — done. Nix flake (dev shell, build, fmt, check), Cargo workspace, Vite/Preact app, frontend embedded and served by the server.
2. **`lyrica-mpd` basics** — connect, greeting/version, command/response framing, errors (`ACK`), and the minimum commands for the first milestone (status, current song, play/pause, previous, next, idle).
3. **Server + WebSocket** — server connects to MPD (`--mpd`, `--mpd-password` CLI args), exposes a WebSocket API to the frontend, pushes state changes.
4. **Frontend** — connects to the WebSocket, shows state, sends commands.

## Milestone 1 (first deliverable)

End-to-end, bare basics only: the currently playing song title and basic playback controls (previous, play/pause, next). Nothing more.

## Backlog (after milestone 1, roughly in order)

- Album art and full song info in now playing; live progress.
- Volume, random and single toggles.
- Live queue view: windowed loading, re-centering, album grouping, lazy cover art.
- Album art via the HTTP library mirror.
- Settings menu: httpd stream playback, stored playlists (list, clear queue, load).
- PWA polish: icons, offline shell.
