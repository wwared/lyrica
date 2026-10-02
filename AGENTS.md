# lyrica

Mobile-first PWA client for MPD, single user, very large library (~200k-song queues). One Rust binary embeds and serves the frontend; configured only via CLI args; no disk state.

## Stack

- `crates/mpd` (`lyrica-mpd`): MPD protocol library — tokio TCP, nom 8. Only commands we need.
- `crates/lyrica`: server binary — tokio, axum, WebSockets; embeds `web/dist` via rust-embed.
- `web/`: Preact + TypeScript PWA, Vite, Vitest, npm.
- Nix flake: `nix develop`, `nix build`, `nix fmt` (treefmt), `nix flake check` (all tests, clippy, formatting).

## Rules

- Never fetch large data (queue, playlists, library) wholesale; window/page it.
- Never implement or send MPD commands that modify or delete files on disk: `update`/`rescan`, stored-playlist edits (`save`, `rm`, `rename`, `playlistadd`, `playlistclear`, `playlistdelete`, `playlistmove`), `sticker set`/`delete`, `mount`/`unmount`.
- The MPD at `127.0.0.1:6600` is a real server with real data. Send it read-only commands only (no playback, queue, option or output changes); test everything else against the fake server in `lyrica-mpd`'s tests.
- Keep docs in `docs/` current when behavior or direction changes.
- `git add` new files before running nix commands.

## Writing style

**Be succinct, direct and to the point.** This applies to everything you write: docs, doc comments, code comments, commit messages, replies.

- Use the fewest words that stay clear. No filler, no preamble, no restating the question, no hedging.
- Code comments explain _why_ only when the code can't. Never narrate what the code does.
- Evidence and statistics belong in `docs/`, not in `src/`.

## Commits

Commit often, without asking, during autonomous work: each commit is one self-contained unit of work (builds, `nix flake check` passes, docs updated), with a message saying what changed and why. Don't batch unrelated changes. Never commit secrets or build artifacts.

## Docs (`docs/`)

- `goals.md` — goals, constraints, non-goals, stack
- `architecture.md` — components, data flow, design direction
- `roadmap.md` — phases, current milestone, backlog
- `development.md` — commands, layout, embedding, dependencies
