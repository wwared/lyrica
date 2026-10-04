# Roadmap

Work proceeds bottom-up in small steps, each usable on its own. Design details live in `architecture.md`.

## Current state

Now playing and the live queue are done and checked on an iPhone. Stored playlists can be listed and loaded over the WebSocket; the button and dialogs to do it are next.

- **Foundation:** Nix flake (dev shell, build, fmt, check, NixOS module); `lyrica-mpd` with the commands lyrica needs; server bridging WebSocket clients to MPD (idle, command and queue connections, reconnect with backoff, offline state, stored playlist listing and loading); frontend embedded in the binary.
- **Now playing:** title, artist (album artist as fallback), album, album art from the HTTP mirror, smooth progress bar with the time beside it, tap / drag seeking on the bar (full mode), previous / play-pause / next, random / single / repeat, MPD volume slider, bouncing marquee for long lines, fixed-height layout pinned to the bottom, animated full / mini mode switch.
- **Live queue (mini mode):** the whole queue in a virtual list of unbounded length under an empty toolbar; rows (track number, title, artist · album, duration) on alternating backgrounds, loaded on demand while shown, two lists ahead on each side; a separator at each album change and the album's cover beside its top visible row, pinned while the album scrolls, probed two lists ahead, also while scrolling; centred on the current song when opened and on each song change (paused for 15 s after the user scrolls), which is highlighted; the visible rows keep their old text across a queue change until their refetch lands; tapping a row plays it. Nothing loads for it in full mode.
- **Play here:** plays MPD's `httpd` stream in the browser, with title, artist, album and cover on the lock screen.
- **iOS fit:** fixed viewport (no bounce, no zoom, no selection), full height in home-screen apps, safe areas.
- **Dark only:** gruvbox dark with its accent colours, regardless of the system setting.
- **PWA:** app icon and favicon, screen kept on while visible, portrait on Android.

## Open work

- **Live queue design:** the row layout and album styling are open to iteration from daily use (`architecture.md` → "Rows").

## Next phase: stored playlists

A load button at the left of the mini-mode toolbar lists the server's stored playlists on each tap and opens them in a modal; selecting one and tapping "Load" closes it and replaces the queue, with a "Loading…" box until MPD is done (`architecture.md` → "Stored playlists"). No general settings menu. Loading never touches the real MPD's queue during development: throwaway MPD or scripted fake (`development.md`). Every step keeps the responsiveness checks (`architecture.md` → "Responsiveness").

1. **Playlist protocol in `lyrica-mpd`:** done. `Client::playlists()` and `Client::replace_queue(name)` (`clear` + `load` in one private command list); measurements and failure modes in `architecture.md` → "Queue measurements". Main finding: a load blocks MPD for every client while it runs, up to ~20 s for the real MPD's large playlists (estimated from `playlistlength`).
2. **Playlists over the WS:** done. `{"type":"playlists"}` (queue connection) and `{"type":"load","name"}` (command connection, checked against a fresh listing first), each answered to its client alone; protocol, errors and measurements in `architecture.md` → "MPD bridge" → "Stored playlists".
3. **Load button and dialogs** (design: `architecture.md` → "Stored playlists"; server replies: "MPD bridge" → "Stored playlists"):
   - **`ws.ts`:** `Connection.playlists(): Promise<string[] | null>` and `Connection.load(name): Promise<string | null>` (the reply's `error`; `null` on success), each resolved by its reply (`playlists`, `loaded`; parse both). A socket not open resolves at once (`null` / `"Not connected"`); a socket closing resolves what's pending (`null` / `"Connection lost"`): in `onclose` and in `shutdown`, which the iOS visibility reconnect calls with `onclose` already detached. Without that the Loading box would stay forever.
   - **Button:** in `.toolbar` (`app.tsx`), at the left, 44 × 44, a playlist icon (`Icon` with a new path), `disabled` like the transport. A tap while a listing is pending does nothing.
   - **Names:** `[]` or `null` → a toast, "No playlists found" / "Couldn't list playlists", for 3 s (`role="status"`; none exists yet, placement to choose, over the queue area); otherwise the dialog.
   - **Dialog:** `<dialog>` + `showModal()`; title "Playlists", X, names as `role="listbox"` options, one selected (highlighted, cleared on each open), each a `Marquee`; "Load" disabled until a selection. Close by X, Escape (`cancel`), or a tap on the backdrop (a `click` whose target is the `<dialog>` itself, so the content needs a wrapper filling the box).
   - **Loading box:** Load closes the dialog and opens a second modal `<dialog>`, "Loading…", with no close (`cancel` prevented). On the reply: success closes it and jumps the list to its top; an error swaps it for an error modal (the `error` text, OK, Escape).
   - **Jump to top:** nothing exists for it: a new `QueueStore` call reaching the list's listener (like `opening`), placing it at row 0. The `loaded` reply and the state push with the new length can arrive in either order; check both.
   - **Checks**, headless Chromium (`development.md` → "Testing the frontend") against lyrica on the throwaway MPD, recreated per `development.md` with playlists of 1,000 / 19,000 / 195,000 songs, one over the limit and the 4,000-directory one (2.5 s, standing in for the real MPD's seconds-long loads): layout at 390 and 375 px; a long name scrolling; an empty playlist directory (toast); listing while offline (button disabled); every close path; each load size, with now playing and the list during and after it (one push, no empty step, list at the top); the error modal (over-limit playlist; a playlist deleted after listing); a visibility reconnect during the slow load (box replaced by "Connection lost"); the responsiveness checks against the previous build. Then by hand on the phones.
   - **Docs:** `architecture.md` → "Stored playlists" and "Toolbar" lose "planned"; the toast and the jump to top documented; roadmap "Current state".

## Later (to design)

Ideas to revisit once the phases above are done; how they look and work is open.

- **Queue actions:** convenience buttons in the mini-mode toolbar. Example: play the current album from the start — jump to the first track of the current song's album in the queue, play it, and turn random off (done by hand today). The set of actions and how to find album boundaries without fetching the whole queue are open.
