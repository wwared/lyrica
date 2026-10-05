# Lyrica

A mobile-first web client for [MPD](https://www.musicpd.org/).

## Features

- Now playing: cover art, title, artist, album, live progress, seeking.
- Controls: previous, play/pause, next, random, single, repeat, volume.
- Live queue of any length, centred on the current song, grouped by album with covers.
- Search the queue with a small query language: words, `"phrases"`, `^prefix`, `-negation`, `field:`, `/regex/`.
- Load stored playlists.
- Play the current album from its start.
- Play MPD's HTTP stream in the browser, with lock-screen controls.

## Goals

- Instant response on huge libraries. Queues of ~200k songs scroll and search smoothly.
- Slow data never blocks. Covers, queue rows and searches load in the background, in small pieces.
- Nothing is fetched whole. The queue is read in windows, searches in chunks.

## Opinionated choices

- Read-only queue: no reordering, adding or removing. Only replaced by a stored playlist.
- Never modifies files through MPD.
- Cover art comes from an HTTP mirror of the music directory, not from MPD.
- Dark gruvbox theme only. No settings menu.
- No authentication or TLS. Run it behind a reverse proxy.
- MPD over TCP only.

## Building

lyrica is a single self-contained binary. It embeds all frontend assets, keeps no state and reads no files. All configuration is command-line arguments (`lyrica --help`).

```sh
nix build
./result/bin/lyrica --mpd 127.0.0.1:6600 --music-url https://music.example.com/
```

The repo is a Nix flake. Add it as an input:

```nix
inputs.lyrica.url = "<url of this repo>";
```

Then use its NixOS module, which runs lyrica as a hardened systemd service:

```nix
imports = [ inputs.lyrica.nixosModules.default ];
services.lyrica = {
  enable = true;
  settings.music-url = "https://music.example.com/";
  environmentFile = "/run/secrets/lyrica"; # LYRICA_MPD_PASSWORD=...
};
```

Development notes and design docs are in [`docs/`](docs/README.md).

## License

[Unlicense](LICENSE). Third-party assets: [CREDITS.md](CREDITS.md).
