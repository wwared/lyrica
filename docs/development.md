# Development

## Environment

`nix develop` (or direnv via `.envrc`) provides cargo/rustc/clippy/rustfmt, rust-analyzer, Node.js/npm, `treefmt`, and `mpc` for poking at MPD.

Entering the dev shell also copies the Iosevka Aile woff2 files from the flake into `web/src/fonts` (gitignored), where `style.css` and Vite pick them up; `nix build` does the same in the web derivation. Outside the dev shell the build still works, with the fallback system font.

Flakes only see git-tracked files: `git add` new files before `nix build`/`nix flake check`.

## Testing against MPD

`lyrica-mpd` tests run against an in-process fake MPD (scripted request/response pairs), so `nix flake check` needs no server. Stateful behaviour is verified against the local MPD (`127.0.0.1:6600`, audio output disabled) within the limits in `AGENTS.md`: read-only commands, playback control and the random/repeat/single options; no restoring needed. `mpc` in the dev shell is handy for that. To wake `idle` without touching playback, `subscribe <channel>` from a second connection that stays open triggers `changed: subscription`.

MPD emits no idle event for commands that change nothing (e.g. `play` while playing), so a test waiting on `idle` after such a command hangs.

Features the local MPD can't or mustn't exercise are tested end to end against a scripted fake MPD (a small TCP server answering the commands involved and firing `idle` events) or a throwaway MPD in place of `--mpd`: volume (the local MPD has no mixer, and its volume must not change), queue changes (auto-update) and queue replacement from stored playlists (its queue must not change).

### Throwaway MPD

A second MPD with its own library, database and queue under `/tmp`, so its queue can change freely; real `idle`, version and `playlistinfo` behaviour. Delete `/tmp/lyrica-mpd` afterwards.

- Library: tagged 1 s Opus files from `ffmpeg`, e.g. 20 albums × 10 tracks: `ffmpeg -loglevel error -f lavfi -i anullsrc=r=8000:cl=mono -t 1 -c:a libopus -b:a 6k -metadata title=… -metadata artist=… -metadata album=… -metadata track=N /tmp/lyrica-mpd/music/albumA/NN.opus` (1.5 s for all 200).
- `/tmp/lyrica-mpd/mpd.conf`: `music_directory`, `playlist_directory` and `db_file` under `/tmp/lyrica-mpd`, `bind_to_address "127.0.0.1"`, `port "6602"`, an `audio_output` of `type "null"`, `max_playlist_length "200000"` (the default caps the queue at 16,384).
- `mpd --no-daemon --stderr /tmp/lyrica-mpd/mpd.conf` builds the database on start.
- A ~175k-entry queue: one command list of `add ""` repeated (each adds the whole library), sent over TCP in one write; returns in ~50 ms. Then `mpc -p 6602 …` for changes, and lyrica with `--mpd 127.0.0.1:6602`. Tracks last 1 s: pause, or `repeat` + `single`, to keep the current song. Deleting most of the queue (`delete 87003:`) blocks MPD for ~9 s; `clear` doesn't.
- Stored playlists: `.m3u` files of library-relative paths (lines may repeat) in `playlist_directory`, listed and loaded without a restart. Loading is fast in this small library (~200k lines in ~0.1 s); MPD's per-song lookup slows with directory width, so to mimic the real MPD's multi-second loads, hard-link one file into 4,000 sibling directories (`music/wide/d0001/01.opus` …; delete `db` and restart to rescan) and point the playlist at them (~200k lines: 2.7 s).

To test connection loss, run lyrica against a proxy and kill it, never MPD itself: `nix shell nixpkgs#socat -c socat TCP-LISTEN:6601,fork,reuseaddr TCP:127.0.0.1:6600`, start lyrica with `--mpd 127.0.0.1:6601`, then `pkill -f TCP-LISTEN:6601` (kills the forked per-connection children too).

## Testing the frontend

Run the server (`cargo run -p lyrica -- --music-url https://music.example.com/ --stream-url https://stream.example.com/`, defaults to MPD at `127.0.0.1:6600` and HTTP on `127.0.0.1:8080`; drop `--music-url` to test without album art, `--stream-url` without the play-here toggle) and either `npm run dev` in `web/` (proxies `/ws` to it) or `npm run build` (served by the debug server without recompiling). The local MPD has no `httpd` output, so a stream from another MPD plays audio that doesn't match the local state.

Headless Chromium (`nix shell nixpkgs#chromium`): **always** start it with exactly these flags; without `--password-store=basic` navigation hangs (`Page.navigate` never returns, pages stay blank, the renderer looks frozen):

```sh
chromium --headless=new --no-sandbox --password-store=basic --use-mock-keychain --remote-debugging-port=9333 --user-data-dir=/tmp/lyrica-chrome
```

Drive it over CDP. Useful calls: `Emulation.setDeviceMetricsOverride` (390×844, `mobile: true`) for the iPhone viewport, `Emulation.setTouchEmulationEnabled` plus `Input.dispatchTouchEvent` for swipes and flings, `Runtime.evaluate` to read the DOM and click, `Page.setWebLifecycleState` (`frozen` then `active`) or `Emulation.setFocusEmulationEnabled` plus dispatching `visibilitychange` to simulate the page being hidden and shown. A Bun or Node WebSocket client is handy for raw protocol checks.

Queue checks (`architecture.md` → "Responsiveness"):

- Compare against the previous build: build both (`git stash` the change, build, copy the binary to `/tmp`; unstash, build again) and run them side by side on two ports.
- Per-frame state after layout, before paint: a `ResizeObserver` on a hidden element whose size a `requestAnimationFrame` loop toggles fires once per frame after layout; sample the visible rows there. Long tasks: a `PerformanceObserver` for `longtask`, added with `Page.addScriptToEvaluateOnNewDocument`.
- Window traffic: wrap `WebSocket` in the page (same injected script) to count, delay or hold back `queue` replies.
- Taps during a fling: an emulated touch on a button only stops the fling, with no `click` (iOS clicks); call the element's `click()` mid-fling instead.
- Cover bytes: the mirror sends no `Timing-Allow-Origin`, so resource timing reports 0 bytes; count `Network.loadingFinished`'s `encodedDataLength` instead. `Network.clearBrowserCache` before each run.
- Real queue regions: dispatch a `wheel` event on the list first (pauses auto-centring, so song changes don't move it), then jump by setting `scrollTop`, up to 8,000 rows at a time (the space's half; re-anchoring recentres between jumps). Most of the queue has a few folders per 100 rows, almost all with covers; some stretches have a folder per row, without covers.
- Main-thread cost (animations, idle work): `Performance.enable`, then `Performance.getMetrics` before and after a fixed idle period or fling sequence; compare the deltas of `TaskDuration`, `RecalcStyleDuration` and `LayoutCount`. `document.getAnimations()` counts what's still animating (`playState`).

iOS home-screen layout: add `standalone` to `<html>`'s classes in the page (as `index.html` does when `navigator.standalone`) to get its `--app-height` and `--top`; the safe-area insets stay 0 in Chromium.

## Frontend gotchas

- Preact 11 doesn't append `px` to numeric style values: `style={{ height: 100 }}` is ignored; write `` `${n}px` ``.

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
web/icons/lyrica.png     App icon source (not shipped); icons in web/public/ are made from it
docs/                    Project documentation
LICENSE, CREDITS.md      The Unlicense; third-party assets and their licenses
```

## Frontend embedding

`crates/lyrica/build.rs` picks the directory embedded by rust-embed:

1. `LYRICA_WEB_DIST` if set (the Nix build sets it to the frontend derivation; must exist);
2. else `web/dist` if it has been built;
3. else an empty directory (with a cargo warning), so Rust-only work never needs a frontend build.

Debug builds read embedded files from disk at runtime, so `npm run build` is picked up without recompiling Rust; release builds include the bytes. Files under `assets/` (content-hashed by Vite) are served as immutable; everything else with `no-cache`.

## App icon

Regenerate the icons in `web/public/` after changing `web/icons/lyrica.png` (square, full-bleed, opaque):

```sh
cd web
for s in 512 192; do magick icons/lyrica.png -filter Lanczos -resize ${s}x$s -strip public/icon-$s.png; done
magick icons/lyrica.png -filter Lanczos -resize 180x180 -strip public/apple-touch-icon.png
magick icons/lyrica.png -alpha set -filter Lanczos -resize 512x512 \
  \( -size 512x512 xc:none -fill white -draw "roundrectangle 0,0,511,511,102,102" \) \
  -compose DstIn -composite -resize 32x32 -strip public/favicon.png
nix shell nixpkgs#oxipng -c oxipng -o max --strip all public/*.png
```

`magick` is `nixpkgs#imagemagick`. The favicon gets transparent rounded corners (20 % radius, masked at 512 px for smooth edges) so it doesn't sit as a hard square in desktop tabs; the app icons stay square, iOS and Android round them.

## NixOS module

The flake exports `nixosModules.default`, a `lyrica` systemd service (`DynamicUser`, hardened, no state):

```nix
services.lyrica = {
  enable = true;
  settings = {
    listen = "127.0.0.1:8080"; # default; point the reverse proxy here
    mpd = "127.0.0.1:6600"; # default
    music-url = "https://music.example.com";
    stream-url = "https://stream.example.com";
  };
  environmentFile = "/run/secrets/lyrica"; # LYRICA_MPD_PASSWORD=...
};
```

`settings` maps 1:1 to the command-line flags (`lyrica --help`); `null` omits one. The MPD password goes in the environment file (`LYRICA_MPD_PASSWORD`, also read when running by hand), not in `settings`, which would put it in the Nix store and the process list.

## Dependencies

- Rust: add to `[workspace.dependencies]` in the root `Cargo.toml`, reference with `workspace = true` from crates.
- npm: `npm install <pkg>` in `web/` and commit `package-lock.json`. Nix uses `importNpmLock`, so there is no hash to update.

### Updating everything

Inside `nix develop`, from the repo root:

1. Nix inputs (nixpkgs, so also Rust and Node; crane; treefmt-nix): `nix flake update`.
2. Rust: `nix shell nixpkgs#cargo-edit -c cargo upgrade --incompatible` raises the requirements in `Cargo.toml`, majors included; then `cargo update` for the rest of `Cargo.lock`.
3. npm, in `web/`: `nix shell nixpkgs#npm-check-updates -c ncu -u` raises the ranges in `package.json`; then `npm install && npm update` for `package-lock.json`.
4. Iosevka: the webfont release in `flake.nix` (`iosevkaAile`) is pinned by URL and hash. Change the version in the URL (twice), set `hash = "";`, run `nix build` and copy the hash it reports.
5. `nix fmt && nix flake check`, fix breaking changes, then try the app.
