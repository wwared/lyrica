# Roadmap

Work proceeds bottom-up in small steps, each usable on its own. Design details live in `architecture.md`.

## Current state

Now playing, the live queue and stored playlists are done and checked on phones.

- **Foundation:** Nix flake (dev shell, build, fmt, check, NixOS module); `lyrica-mpd` with the commands lyrica needs; server bridging WebSocket clients to MPD (idle, command and queue connections, reconnect with backoff, offline state, stored playlist listing and loading); frontend embedded in the binary.
- **Now playing:** title, artist (album artist as fallback), album, album art from the HTTP mirror, smooth progress bar with the time beside it, tap / drag seeking on the bar (full mode), previous / play-pause / next, random / single / repeat, MPD volume slider, bouncing marquee for long lines, fixed-height layout pinned to the bottom, animated full / mini mode switch.
- **Live queue (mini mode):** the whole queue in a virtual list of unbounded length under the toolbar; rows (track number, title, artist · album, duration) on alternating backgrounds, loaded on demand while shown, two lists ahead on each side; a separator at each album change and the album's cover beside its top visible row, pinned while the album scrolls, probed two lists ahead, also while scrolling; centred on the current song when opened, on each song change (paused for 15 s after the user scrolls) and on a tap on now playing's cover, previous or next; the current song is highlighted; the visible rows keep their old text across a queue change until their refetch lands; tapping a row plays it. Nothing loads for it in full mode.
- **Stored playlists:** a toolbar button lists them afresh on each tap (a toast when there are none or listing failed) in a modal; "Load" replaces the queue, with a modal "Loading…" box (seen only after 300 ms) until MPD is done, then the list at its top, or MPD's error in a small modal.
- **Play here:** plays MPD's `httpd` stream in the browser, with title, artist, album and cover on the lock screen.
- **iOS fit:** fixed viewport (no bounce, no zoom, no selection), full height in home-screen apps, safe areas.
- **Dark only:** gruvbox dark with its accent colours, regardless of the system setting.
- **PWA:** app icon and favicon, screen kept on while visible, portrait on Android.

## Current milestone: album from the start

A toolbar button, at its right, that plays the current song's album from its first entry in the queue with random and single off (done by hand today). Repeat is left alone; the queue isn't changed. One commit per step; docs move with each.

1. **Done: grouping rule on the server, one copy.** `same_group` in `bridge.rs` sets each window entry's `start` flag, read with the entry before the window; the frontend requests exact blocks and only reads the flag (`architecture.md` → "Album grouping").
2. **Done: `lyrica-mpd` command list.** `Client::play_in_order(id: Option<u32>)`: `random 0`, `single 0`, then `playid` when given an id, in one command list (`architecture.md` → "Command lists").
3. **Done: server, `{"type":"albumstart"}`.** On the command connection: `status`, then `group_start` walks back from the current song in windows of 100 with `same_group`, then `play_in_order`, leaving out `playid` when the first entry is already playing (`architecture.md` → "Queue actions").
4. **Done: previous / next re-centre.** In mini mode, `App`'s previous and next send, then call `store.recentre()`, like the cover tap; the push's song change then centres the new song (`architecture.md` → "Auto-centre").
5. **The button.** Frontend only; the server takes `{"type":"albumstart"}` since step 3.
   - **`ws.ts`:** `"albumstart"` joins the first `Command` member's types; the doc comment above says what it does (plays the current album group in order from its first entry, random and single off; `architecture.md` → "Queue actions").
   - **`app.tsx`:** in `<div class="toolbar">`, after `{playlists}` and outside its memo (the button changes with the current song): `<button class="album" aria-label="Play album from the start">` with `<Icon path={ALBUM} />`. `disabled={disabled || snapshot.queue?.id == null}` (offline, or no current song). `onClick`: `send({ type: "albumstart" })`, then `store.recentre()`; no `mini` check, since the toolbar is only reachable in mini mode (`.queue` is hidden in full mode). `ALBUM` beside the other icon paths, commented "Material Icons (Apache 2.0): album": `M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 14.5c-2.49 0-4.5-2.01-4.5-4.5S9.51 7.5 12 7.5s4.5 2.01 4.5 4.5-2.01 4.5-4.5 4.5zm0-5.5c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1z` (from `fonts.gstatic.com/s/i/materialicons/album/v12/24px.svg`).
   - **`style.css`:** `.toolbar .album { margin-left: auto; }`. The existing `.toolbar button` rules already give 44 × 44, muted colour, `bg2` on `:active`, 0.5 opacity disabled, no tap highlight; with the toolbar's 10 px padding the icon ends 20 px from the right edge, at the durations' right edge. The toast is absolutely positioned, so it doesn't take part in the flex row. The `.toolbar` comment gains that the last icon lines up with the durations.
   - **Docs (`architecture.md`):** "Toolbar" (queue actions at its right, no longer planned); "Queue actions" loses "(planned…)", names `app.tsx` and gets a "Button" line like the stored playlists' one (icon, place, disabled states, the tap re-centring); "Album grouping" says "the album button" for "the planned album action"; a "Queue actions" entry in "Measurements". This file: step 5 done; "Current state" waits for step 6.
   - **Tests:** none new (a type member and wiring; the server side is tested). `npm run typecheck`, `npm test`, the check.
   - **Smoke** (headless Chromium as in step 4, `npm run build`, debug server with `--music-url`; results under `architecture.md` → "Measurements" → "Queue actions"):
     - layout at 390×844 and 375×667: button x 336–380 / 321–365, y 0–44 (plus the safe-area top), icon ending at x 370 / 355, the same as a row's duration right edge; colour equal to the playlists button's;
     - disabled: offline (stop the debug server) and with no current song: a queue with no current song can't be made on the real MPD, so rewrite incoming `state` messages' `queue.id` / `position` to `null` in a page script (`Page.addScriptToEvaluateOnNewDocument`), as step 4's smoke wrapped `WebSocket`;
     - mid-album with random and single on, scrolled away: one `albumstart` out, one push with the group's first entry, random and single off, that row centred and highlighted;
     - again on that entry while playing: no push, the list stays centred.
6. **Smoke** against the real MPD (playback control, random and single are allowed):
   - mid-album, random and single on: the first entry plays, both off, one push;
   - on the first entry, playing, random on: random off, the song carries on;
   - on the first entry, paused: it plays from the beginning, random and single off;
   - on the first entry, playing, both off: nothing changes, list centred;
   - an album over 100 entries; one starting at a block edge;
   - scrolled away or flinging, then the button, previous, next: centred;
   - a natural song change while paused: not centred.

   Then on the iPhone; this file's "Current state" gets the feature.

## Open work

- **Live queue design:** the row layout and album styling are open to iteration from daily use (`architecture.md` → "Rows").

## Later (to design)

Ideas to revisit; how they look and work is open.

- **More queue actions:** further toolbar buttons beside the album one; which ones is open.
