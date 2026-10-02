# Development

## Environment

`nix develop` (or direnv via `.envrc`) provides cargo/rustc/clippy/rustfmt, rust-analyzer, Node.js/npm, `treefmt`, and `mpc` for poking at MPD.

Flakes only see git-tracked files: `git add` new files before `nix build`/`nix flake check`.

## Testing against MPD

`lyrica-mpd` tests run against an in-process fake MPD (scripted request/response pairs), so `nix flake check` needs no server. The local MPD (`127.0.0.1:6600`) holds real data: probe it with read-only commands only (`status`, `currentsong`, `ping`, `idle`). Playback/queue changes are verified against the fake server. To exercise `idle` without touching playback, `subscribe <channel>` from a second connection that stays open triggers `changed: subscription`.

## Commands

| Command                          | What it does                                                                                 |
| -------------------------------- | -------------------------------------------------------------------------------------------- |
| `nix build`                      | Builds the frontend, then the `lyrica` binary with it embedded                               |
| `nix fmt`                        | Formats the whole repo (nixfmt, rustfmt, taplo, prettier) via treefmt                        |
| `nix flake check`                | Package build, clippy (`-D warnings`), cargo tests, frontend typecheck + tests, format check |
| `cargo run -p lyrica -- --help`  | Run the server from the workspace                                                            |
| `npm run dev` (in `web/`)        | Vite dev server with hot reload; proxies `/ws` to the server on `127.0.0.1:8080`             |
| `npm run build` (in `web/`)      | Build the frontend into `web/dist`                                                           |
| `npm run typecheck` / `npm test` | TypeScript check / Vitest                                                                    |

## Layout

```
flake.nix, treefmt.nix   Nix outputs and formatter config
Cargo.toml               Rust workspace (shared deps in [workspace.dependencies])
crates/mpd/              lyrica-mpd: MPD protocol client library
crates/lyrica/           lyrica: server binary, embeds web/dist
web/                     Preact + TypeScript PWA (Vite)
docs/                    Project documentation
```

## Frontend embedding

`crates/lyrica/build.rs` picks the directory embedded by rust-embed:

1. `LYRICA_WEB_DIST` if set (the Nix build sets it to the frontend derivation; must exist);
2. else `web/dist` if it has been built;
3. else an empty directory (with a cargo warning), so Rust-only work never needs a frontend build.

Debug builds read embedded files from disk at runtime, so `npm run build` is picked up without recompiling Rust; release builds include the bytes. Files under `assets/` (content-hashed by Vite) are served as immutable; everything else with `no-cache`.

## Dependencies

- Rust: add to `[workspace.dependencies]` in the root `Cargo.toml`, reference with `workspace = true` from crates.
- npm: `npm install <pkg>` in `web/` and commit `package-lock.json`. Nix uses `importNpmLock`, so there is no hash to update.
