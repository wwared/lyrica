# Goals and constraints

lyrica is a mobile-first PWA client for [MPD](https://www.musicpd.org/), primarily used from an iPhone. It has a single user (the author) and is tailored to one specific setup; generality is not a goal.

## Goals

- **Fast on a huge library.** Must stay responsive with queues/playlists of ~200k songs and ~10k albums.
- **Immediately responsive UI.** Now-playing info and controls work instantly; the queue, cover art and other slow data load lazily and asynchronously without blocking anything.
- **Simple player UI.** Now playing (art, song info, live progress) and controls on top; an auto-scrolling live queue below, centered on the current song. Light/dark mode, clean controls. Everything else lives in a normally-hidden settings menu.
- **Single self-contained binary.** One Rust binary serves the embedded frontend and talks to MPD.

## Constraints

- **Never fetch large data unnecessarily.** No "list the whole queue/library" requests unless explicitly needed; page/window everything. This applies to MPD queries and to backend→frontend traffic alike.
- **No disk state.** The binary reads/writes no files. All settings are command-line arguments (no config files, no environment variables).
- **Plain HTTP only.** TLS is terminated by a Caddy reverse proxy (outside this repo). The app must work behind it, including under a path prefix: use relative URLs everywhere, including the WebSocket endpoint.
- **MPD over TCP only** (`host:port`), optional password via CLI argument. MPD is reachable only over an internal VPN.
- **Never modify files through MPD.** No database updates, stored-playlist edits, sticker writes or mounts. Reading stored playlists and loading them into the queue is fine.
- **Partial protocol coverage.** The MPD crate implements only what the frontend needs; commands are added as features require them.
- **Setup-specific features are welcome.** E.g. every music library path is also readable over HTTP(S) from a known base URL, which lyrica can use instead of MPD (cover art, file listings) to work around MPD protocol limitations.

## Non-goals

- Supporting arbitrary MPD setups, multiple users, or authentication in lyrica itself.
- Full MPD protocol coverage.
- Persisting anything server-side.

## Tech stack

- **Nix flake**: dev shell, packaging (`nix build`), formatting (`nix fmt` via treefmt), all tests (`nix flake check`).
- **`crates/mpd` (`lyrica-mpd`)**: Rust, tokio TCP sockets, nom 8 parsers.
- **`crates/lyrica`**: Rust server; tokio, axum, WebSockets (tungstenite via axum); embeds the frontend.
- **`web/`**: Preact + TypeScript PWA, built with Vite (npm), tested with Vitest.
