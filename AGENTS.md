# lyrica

Mobile-first PWA client for MPD, single user, very large library (~200k-song queues). One Rust binary embeds and serves the frontend; configured only via CLI args; no disk state.

## Stack

- `crates/mpd` (`lyrica-mpd`): MPD protocol library — tokio TCP, nom 8. Only commands we need.
- `crates/lyrica`: server binary — tokio, axum, WebSockets; embeds `web/dist` via rust-embed.
- `web/`: Preact + TypeScript PWA, Vite, Vitest, npm.
- Nix flake: `nix develop`, `nix build`, `nix fmt` (treefmt), `nix flake check` (all tests, clippy, formatting).

## Rules

- Never fetch large data (queue, playlists, library) wholesale; window/page it.
- Keep docs in `docs/` current when behavior or direction changes.
- `git add` new files before running nix commands.

## Docs (`docs/`)

- `goals.md` — goals, constraints, non-goals, stack
- `architecture.md` — components, data flow, design direction
- `roadmap.md` — phases, current milestone, backlog
- `development.md` — commands, layout, embedding, dependencies
