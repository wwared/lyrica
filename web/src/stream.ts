import { useEffect, useRef, useState } from "preact/hooks";

const RETRY_MS = 1000;

type Stream = { start(url: string): void; stop(): void };

/** What the lock screen and Control Center show while the stream plays. */
export type NowPlaying = {
  title: string;
  artist: string;
  album: string;
  /** Absolute URL; `undefined` shows none. */
  artwork: string | undefined;
};

function show({ title, artist, album, artwork }: NowPlaying) {
  if (!("mediaSession" in navigator)) return;
  navigator.mediaSession.metadata = new MediaMetadata({
    title,
    artist,
    album,
    artwork: artwork === undefined ? [] : [{ src: artwork }],
  });
}

/**
 * MPD's `httpd` output, played by a detached `Audio` (no controls on the
 * page). The stream is live: `stop` unloads it, closing the connection, and
 * `start` opens a new one, so audio never resumes from a stale buffer.
 */
function createStream(
  onChange: (on: boolean) => void,
  onPlaying: () => void,
): Stream {
  const audio = new Audio();
  let url: string | null = null;
  let last: string | null = null;
  let retry: number | undefined;

  // Every start sets `src` again, which also avoids iOS 26 home-screen apps
  // playing silence after a relaunch (WebKit bug 295518).
  const load = () => {
    if (url === null) return;
    audio.src = url;
    audio.play().catch((error: DOMException) => {
      // Not a user gesture (e.g. a retry while in the background).
      if (error.name === "NotAllowedError") stop();
    });
  };

  const start = (next: string) => {
    url = last = next;
    window.clearTimeout(retry);
    load();
    onChange(true);
  };

  const stop = () => {
    if (url === null) return;
    url = null;
    window.clearTimeout(retry);
    audio.removeAttribute("src");
    audio.load();
    onChange(false);
  };

  // The stream ends or fails when MPD stops (closing the output) or the
  // network drops; keep trying while on.
  const reconnect = () => {
    if (url === null) return;
    window.clearTimeout(retry);
    retry = window.setTimeout(load, RETRY_MS);
  };
  audio.addEventListener("error", reconnect);
  audio.addEventListener("ended", reconnect);
  // Paused by the system (lock screen, headphones unplugged, a call): resuming
  // would play stale audio, so turn off instead.
  audio.addEventListener("pause", () => {
    if (!audio.ended) stop();
  });
  audio.addEventListener("playing", onPlaying);

  if ("mediaSession" in navigator) {
    navigator.mediaSession.setActionHandler("play", () => {
      if (last !== null) start(last);
    });
    navigator.mediaSession.setActionHandler("pause", stop);
    navigator.mediaSession.setActionHandler("stop", stop);
  }

  return { start, stop };
}

/**
 * Whether the stream at `url` is playing here, and a toggle for it. `info`
 * is kept current in the Media Session.
 */
export function useStream(
  url: string | null,
  info: NowPlaying,
): [boolean, () => void] {
  const [on, setOn] = useState(false);
  const latest = useRef(info);
  latest.current = info;
  const stream = useRef<Stream | null>(null);
  // Also set again when playback (re)starts, in case the browser drops
  // metadata set while nothing was playing.
  stream.current ??= createStream(setOn, () => show(latest.current));
  useEffect(
    () => show(info),
    [info.title, info.artist, info.album, info.artwork],
  );
  useEffect(() => {
    if (url === null) stream.current?.stop();
  }, [url]);
  // Starts in the click handler itself: iOS only allows `play()` in a gesture.
  const toggle = () => {
    if (on) stream.current?.stop();
    else if (url !== null) stream.current?.start(url);
  };
  return [on, toggle];
}
