# Goals and constraints

lyrica is a mobile-first PWA client for [MPD](https://www.musicpd.org/), primarily used from an iPhone. It has a single user (the author) and is tailored to one specific setup; generality is not a goal.

## Goals

- **Fast on a huge library.** Must stay responsive with queues/playlists of ~200k songs and ~10k albums.
- **Immediately responsive UI.** Now-playing info and controls work instantly; the queue, cover art and other slow data load lazily and asynchronously, on demand and in the background, without blocking anything. Slow data must never make anything slower: interactivity, loading and state updates stay as fast as they are without it.
- **Simple player UI.** Now playing (art, song info, live progress) and controls at the bottom; an auto-scrolling live queue above, centred on the current song, under a toolbar holding the stored playlists and search buttons, the player toggles and queue actions. Dark gruvbox colours, clean controls. No settings menu.
- **Read-only queue.** The queue is viewed and played from, never edited: no reordering, adding, removing or consume mode. The only change lyrica makes to it is replacing it with a stored playlist.
- **Single self-contained binary.** One Rust binary serves the embedded frontend and talks to MPD.

## Constraints

- **Never fetch large data unnecessarily.** No "list the whole queue/library" requests unless explicitly needed; page/window everything. This applies to MPD queries and to backend→frontend traffic alike.
- **No disk state.** The binary reads/writes no files. All settings are command-line arguments (no config files); the one exception is the MPD password, also read from an environment variable to keep it out of the process list.
- **Plain HTTP only.** TLS is terminated by a Caddy reverse proxy (outside this repo). The app must work behind it, including under a path prefix: use relative URLs everywhere, including the WebSocket endpoint.
- **MPD over TCP only** (`host:port`), optional password. MPD is reachable only over an internal VPN.
- **Never modify files through MPD.** No database updates, stored-playlist edits, sticker writes or mounts. Reading stored playlists and loading them into the queue is fine.
- **Partial protocol coverage.** The MPD crate implements only what the frontend needs; commands are added as features require them.
- **Setup-specific features are welcome.** E.g. every music library path is also readable over HTTP(S) from a known base URL, which lyrica can use instead of MPD (cover art, file listings) to work around MPD protocol limitations.

## Non-goals

- Supporting arbitrary MPD setups, multiple users, or authentication in lyrica itself.
- Full MPD protocol coverage.
- Persisting anything server-side.
- Offline use: without the server and MPD there is nothing to show or control.

## Tech stack

- **Nix flake**: dev shell, packaging (`nix build`), formatting (`nix fmt` via treefmt), all tests (`nix flake check`).
- **`crates/mpd` (`lyrica-mpd`)**: Rust, tokio TCP sockets, nom 8 parsers.
- **`crates/lyrica`**: Rust server; tokio, axum, WebSockets (tungstenite via axum); embeds the frontend.
- **`web/`**: Preact + TypeScript PWA, built with Vite (npm), tested with Vitest.
