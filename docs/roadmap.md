# Roadmap

Work proceeds bottom-up, adding features piecemeal once the end-to-end basis works.

## Phases

1. **Scaffolding** — done. Nix flake (dev shell, build, fmt, check), Cargo workspace, Vite/Preact app, frontend embedded and served by the server, placeholder WebSocket connection.
2. **`lyrica-mpd` basics** — done. Connect, greeting/version, command/response framing, errors (`ACK`), and the minimum commands for the first milestone (password, status, current song, play/pause, previous, next, idle).
3. **Server + WebSocket** — next. Design is in `architecture.md` → "MPD bridge". Steps: CLI args; idle task + `watch` snapshot; command task + `mpsc`; JSON WS protocol replacing the greeting; reconnect/backoff. The frontend may display raw messages until phase 4. Done when: a WS client gets the state (including `elapsed`/`duration`) on connect and within ~100 ms of any playback change (including changes made by `mpc`, and seeks), `toggle`/`next`/`previous` work, and the server survives MPD connection loss and idle connection timeouts. Verify against the local MPD per `AGENTS.md`; simulate connection loss with a killable proxy (e.g. `socat`) in front of it, never by restarting MPD.
4. **Frontend** — connects to the WebSocket, shows state, sends commands.

## Milestone 1 (first deliverable)

End-to-end, bare basics only: the currently playing song title and basic playback controls (previous, play/pause, next). Nothing more.

## Backlog (after milestone 1, roughly in order)

- Album art and full song info in now playing; smooth progress bar (design in `architecture.md`; the phase 3 protocol already carries `elapsed`/`duration`).
- Volume, random and single toggles.
- Live queue view: windowed loading, re-centering, album grouping, lazy cover art.
- Album art via the HTTP library mirror.
- Settings menu: httpd stream playback, stored playlists (list, clear queue, load).
- PWA polish: icons, offline shell.
