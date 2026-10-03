# Development

## Environment

`nix develop` (or direnv via `.envrc`) provides cargo/rustc/clippy/rustfmt, rust-analyzer, Node.js/npm, `treefmt`, and `mpc` for poking at MPD.

Entering the dev shell also copies the Iosevka Aile woff2 files from the flake into `web/src/fonts` (gitignored), where `style.css` and Vite pick them up; `nix build` does the same in the web derivation. Outside the dev shell the build still works, with the fallback system font.

Flakes only see git-tracked files: `git add` new files before `nix build`/`nix flake check`.

## Testing against MPD

`lyrica-mpd` tests run against an in-process fake MPD (scripted request/response pairs), so `nix flake check` needs no server. Stateful behaviour is verified against the local MPD (`127.0.0.1:6600`, audio output disabled) within the limits in `AGENTS.md`: read-only commands, playback control and the random/repeat/single options; no restoring needed. `mpc` in the dev shell is handy for that. To wake `idle` without touching playback, `subscribe <channel>` from a second connection that stays open triggers `changed: subscription`.

MPD emits no idle event for commands that change nothing (e.g. `play` while playing), so a test waiting on `idle` after such a command hangs.

To test connection loss, run lyrica against a proxy and kill it, never MPD itself: `nix shell nixpkgs#socat -c socat TCP-LISTEN:6601,fork,reuseaddr TCP:127.0.0.1:6600`, start lyrica with `--mpd 127.0.0.1:6601`, then `pkill -f TCP-LISTEN:6601` (kills the forked per-connection children too).

## Testing the frontend

Run the server (`cargo run -p lyrica -- --music-url https://webmusic.wwared.com/`, defaults to MPD at `127.0.0.1:6600` and HTTP on `127.0.0.1:8080`; drop `--music-url` to test without album art) and either `npm run dev` in `web/` (proxies `/ws` to it) or `npm run build` (served by the debug server without recompiling).

Headless Chromium (`nix shell nixpkgs#chromium`) needs `--headless=new --no-sandbox --password-store=basic --use-mock-keychain --remote-debugging-port=9333`; drive it over CDP. Without `--password-store=basic`, navigation hangs (`Page.navigate` never returns, pages stay blank). Useful CDP calls: `Emulation.setDeviceMetricsOverride` (390×844, `mobile: true`) for the iPhone viewport, `Runtime.evaluate` to read the DOM and click, `Page.setWebLifecycleState` (`frozen` then `active`) or `Emulation.setFocusEmulationEnabled` plus dispatching `visibilitychange` to simulate the page being hidden and shown. A Bun or Node WebSocket client is handy for raw protocol checks.

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
nix/module.nix           NixOS module (services.lyrica)
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

## NixOS module

The flake exports `nixosModules.default`, a `lyrica` systemd service (`DynamicUser`, hardened, no state):

```nix
services.lyrica = {
  enable = true;
  settings = {
    listen = "127.0.0.1:8080"; # default; point the reverse proxy here
    mpd = "127.0.0.1:6600"; # default
    music-url = "https://music.example.com";
  };
  environmentFile = "/run/secrets/lyrica"; # LYRICA_MPD_PASSWORD=...
};
```

`settings` maps 1:1 to the command-line flags (`lyrica --help`); `null` omits one. The MPD password goes in the environment file (`LYRICA_MPD_PASSWORD`, also read when running by hand), not in `settings`, which would put it in the Nix store and the process list. Verified in a NixOS VM test (not part of `nix flake check`): flags in `ExecStart`, the page served, the password reaching a fake MPD and absent from `ps`.

## Dependencies

- Rust: add to `[workspace.dependencies]` in the root `Cargo.toml`, reference with `workspace = true` from crates.
- npm: `npm install <pkg>` in `web/` and commit `package-lock.json`. Nix uses `importNpmLock`, so there is no hash to update.
