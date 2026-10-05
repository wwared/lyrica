# Roadmap

Work proceeds bottom-up in small steps, each usable on its own. Design details live in `architecture.md`.

## Current state

Now playing, the live queue, stored playlists, the album button and the toolbar's player toggles are done and checked on phones, except the volume pill, the queue marquees and the toolbar's even spacing, not yet tried by hand.

- **Foundation:** Nix flake (dev shell, build, fmt, check, NixOS module); `lyrica-mpd` with the commands lyrica needs; server bridging WebSocket clients to MPD (idle, command and queue connections, reconnect with backoff, offline state, stored playlist listing and loading, album start); frontend embedded in the binary.
- **Now playing:** title, artist (album artist as fallback), album, album art from the HTTP mirror, smooth progress bar with the time beside it, tap / drag seeking on the bar (full mode), previous / play-pause / next, random / single / repeat, MPD volume in a vertical pill growing up from its toggle, bouncing marquee for long lines, fixed-height layout pinned to the bottom, animated full / mini mode switch.
- **Live queue (mini mode):** the whole queue in a virtual list of unbounded length under the toolbar; rows (track number, title, artist · album, duration) on alternating backgrounds, long titles and artist · album lines bouncing in marquees while on screen, loaded on demand while shown, two lists ahead on each side; a separator at each album change and the album's cover beside its top visible row, pinned while the album scrolls, probed two lists ahead, also while scrolling; centred on the current song when opened, on each song change (paused for 15 s after the user scrolls) and on a tap on now playing's cover; the current song is highlighted; the visible rows keep their old text across a queue change until their refetch lands; tapping a row plays it. Nothing loads for it in full mode.
- **Stored playlists:** a toolbar button lists them afresh on each tap (a toast when there are none or listing failed) in a modal; "Load" replaces the queue, with a modal "Loading…" box (seen only after 300 ms) until MPD is done, then the list at its top, or MPD's error in a small modal.
- **Album from the start:** a toolbar button, its last, plays the current song's album from its first entry with random and single off (repeat and the queue untouched) and centres the list on it.
- **Player toggles in mini mode:** play here, volume, random, single and repeat, in the toolbar's evenly spaced row between the playlists and album buttons; the volume pill grows down from there, the same size as in full mode.
- **Play here:** plays MPD's `httpd` stream in the browser, with title, artist, album and cover on the lock screen.
- **iOS fit:** fixed viewport (no bounce, no zoom, no selection), full height in home-screen apps, safe areas, content below the system fade at the top of home-screen apps.
- **Dark only:** gruvbox dark with its accent colours, regardless of the system setting.
- **PWA:** app icon and favicon, screen kept on while visible, portrait on Android.

## Open work

- **Search** (design in `architecture.md` → "Search (planned)"): find matches in the queue and step through them, centred, without playing them. Each step lands as its own commits, with tests, measurements and docs.
  1. **Toolbar row** (done): one evenly spaced row of 44 px buttons (playlists, toggles, album); positions checked at 390 and 375 px, with and without play here.
  2. **`lyrica-mpd` search** (done): `search_queue` (`status` + `playlistsearch FILTER window START:END` in one command list) returns the queue version, length and the matches' positions; `Filter` renders MPD's filter syntax with its quoting. Timings on the real MPD under "Queue measurements" → "Queue search".
  3. **Query, phase 1:** `crates/lyrica/src/query.rs`, a nom parser for plain words, each compiled to a `(*UTF)` literal regex over the default fields (file first, `OR` as `!(… !~ … AND …)`), built as a `lyrica_mpd::Filter`. Tests for escaping (quotes, backslashes, regex metacharacters, non-ASCII) and the filter text; checked against the real MPD (the `BJÖRK` / `Σ` cases, a word with `'`).
  4. **Server search:** the `search` / `matches` / `searchfailed` / `searchend` messages; chunk 0, the first match after `from` by interpolation probes with bisection fallback, further chunks on request; steps interleaved with windows and listings in the queue task; one search per connection; stale on a version change. Over the WS: probe counts and times, and how long windows and state pushes wait during a search, against the 65 ms target and 100 ms ceiling.
  5. **Search mode UI:** the search button; `[✕][input][🔍]` with Return / 🔍 submitting and replacing the matches; the pill with ▲▼, wrap and the spinner; first match after the current song; `reveal` centring, auto-centring off, room for the pill; match and focused-match styles; toasts for errors and "No matches"; rerun on a queue change; leaving by ✕, Escape or full mode. `search.ts` under Vitest; headless Chromium checks of each behaviour and the scroll / render cost.
  6. **Count:** the total by the same probes after the first chunk, paced; the pill's `…` replaced by it. Measured like step 4; over the ceiling, the pill shows `500+` instead.
  7. **iPhone check (phase 1):** keyboard, no zoom on focus, pill reach and placement, room for the pill, centring, toasts, leaving.
  8. **Query, phase 2:** the whole grammar (quotes, `-`, `^`, `/re/`, `field:` for title, artist, albumartist, album, file, any; plain-text fallbacks; errors for an unclosed `"` or `/`), thorough nom tests, positive terms first. Measure the costly shapes, negated-only queries (140–263 ms) and `any` (142 ms), and try them by hand: if the longer searches don't hurt the experience much, keep them for the more powerful language; otherwise reject, simplify or remove what stays over 100 ms.
  9. **iPhone check (phase 2)** and the query language's reference in the docs updated to what shipped.
