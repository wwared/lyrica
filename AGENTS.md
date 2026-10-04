# lyrica

Mobile-first PWA client for MPD, single user, very large library (~200k-song queues). One Rust binary embeds and serves the frontend; configured only via CLI args; no disk state.

## Stack

- `crates/mpd` (`lyrica-mpd`): MPD protocol library — tokio TCP, nom 8. Only commands we need.
- `crates/lyrica`: server binary — tokio, axum, WebSockets; embeds `web/dist` via rust-embed.
- `web/`: Preact + TypeScript PWA, Vite, Vitest, npm.
- Nix flake: `nix develop`, `nix build`, `nix fmt` (treefmt), `nix flake check` (all tests, clippy, formatting).

## Rules

- Never read, search or list files outside this repository and `/tmp/`. Other directories (home, other repos, system config) are off limits, even when debugging deployment.
- The program never fetches large data (queue, playlists, library) wholesale; it windows/pages it. Fetching it whole once while testing or measuring is fine.
- Never implement or send MPD commands that modify or delete files on disk: `update`/`rescan`, stored-playlist edits (`save`, `rm`, `rename`, `playlistadd`, `playlistclear`, `playlistdelete`, `playlistmove`), `sticker set`/`delete`, `mount`/`unmount`.
- The MPD at `127.0.0.1:6600` is a real server with real data; its audio output is disabled. Allowed: read-only commands, playback control (`play`, `playid`, `pause`, `stop`, `next`, `previous`, `seek`) and the random/repeat/single options; no need to restore them afterwards. Never change outputs, volume, consume, the queue, stored playlists or anything stored on disk.
- Keep docs in `docs/` current when behavior or direction changes.
- `git add` new files before running nix commands.
- Before testing in headless Chromium, read "Testing the frontend" in `docs/development.md` and use its exact flags. `--password-store=basic` is mandatory: without it every page hangs blank.
- Naming: MPD's current playlist is the "queue", in code, UI and docs; "playlist" means a stored playlist only. MPD protocol names (`playlistinfo`, `playlist`, `playlistlength`) stay as they are at the protocol boundary.

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
- `roadmap.md` — current state, next phases as small steps
- `development.md` — commands, layout, embedding, dependencies
