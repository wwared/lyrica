import type { RefObject } from "preact";
import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "preact/hooks";
import noCover from "./assets/no-cover.png";
import { covers, findCover } from "./cover";
import { Icon } from "./icon";
import { Marquee } from "./marquee";
import { Playlists } from "./playlists";
import { formatTime, position } from "./progress";
import { QueueList } from "./queue";
import { queueStore } from "./queue-store";
import { useStream } from "./stream";
import { Volume } from "./volume";
import { useWakeLock } from "./wake";
import {
  connect,
  NOT_CONNECTED,
  type Command,
  type Connection,
  type Snapshot,
} from "./ws";

export function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [streamUrl, setStreamUrl] = useState<string | null>(null);
  // One for the app's lifetime, so it never invalidates the list's memo.
  const [store] = useState(queueStore);
  const connection = useRef<Connection | null>(null);
  useEffect(() => {
    const c = connect(
      setSnapshot,
      (config) => setStreamUrl(config.stream),
      store,
    );
    connection.current = c;
    return () => c.close();
  }, []);
  useWakeLock();

  // Stable, so passing them never invalidates the list's or toolbar's memo.
  const [send] = useState(
    () => (command: Command) => connection.current?.send(command),
  );
  const [list] = useState(
    () => () => connection.current?.playlists() ?? Promise.resolve(null),
  );
  const [load] = useState(
    () => (name: string) =>
      connection.current?.load(name) ?? Promise.resolve(NOT_CONNECTED),
  );
  const offline = snapshot?.state === "offline";
  const disabled = snapshot === null || offline;
  const heading =
    snapshot === null
      ? "Connecting…"
      : offline
        ? "Offline"
        : (snapshot.title ?? "Nothing playing");
  const cover = useCover(snapshot === null ? undefined : snapshot.folder);
  const [listening, toggleListening] = useStream(streamUrl, {
    title: heading,
    artist: snapshot?.artist ?? "",
    album: snapshot?.album ?? "",
    // Left out while unknown, so the lock screen never flashes the placeholder.
    artwork:
      cover === undefined
        ? undefined
        : new URL(cover ?? noCover, location.href).href,
  });
  const [mini, setMini] = useState(false);
  // Layout effect: opening centres the list before the fade-in's first frame.
  useLayoutEffect(() => store.show(mini), [mini]);
  const [volumeOpen, setVolumeOpen] = useState(false);
  const nowPlaying = useRef<HTMLElement>(null);
  const capture = useFlip(nowPlaying, mini);
  // The same element while the length holds, so Preact skips the list on
  // every other state push.
  const queueLength = snapshot?.queue?.length ?? 0;
  const queueList = useMemo(
    () => <QueueList length={queueLength} store={store} send={send} />,
    [queueLength],
  );
  const playlists = useMemo(
    () => (
      <Playlists
        disabled={disabled}
        list={list}
        load={load}
        onLoaded={() => store.top()}
      />
    ),
    [disabled],
  );

  return (
    <main class={mini ? "app mini" : "app"}>
      <section class="queue" aria-hidden={!mini}>
        <div class="toolbar">
          {playlists}
          <button
            class="album-start"
            aria-label="Play album from the start"
            disabled={disabled || snapshot.queue?.id == null}
            onClick={() => {
              send({ type: "albumstart" });
              store.recentre();
            }}
          >
            <Icon path={ALBUM} />
          </button>
        </div>
        {queueList}
      </section>
      <section class="now-playing" ref={nowPlaying}>
        <Art
          cover={cover}
          onClick={mini ? () => store.recentre() : undefined}
        />
        <h1
          class={snapshot?.title ? "title" : "title placeholder"}
          data-flip="move"
        >
          <Marquee text={heading} />
        </h1>
        <button
          class="switch"
          aria-label={mini ? "Full player" : "Queue"}
          onClick={() => {
            capture();
            setMini(!mini);
            setVolumeOpen(false);
          }}
        >
          <Icon path={EXPAND} />
        </button>
        <p class="artist" data-flip="move">
          <Marquee text={snapshot?.artist ?? ""} />
        </p>
        <p class="album" data-flip="move">
          <Marquee text={snapshot?.album ?? ""} />
        </p>
        <Progress
          snapshot={snapshot}
          seekable={
            !mini &&
            (snapshot?.state === "play" || snapshot?.state === "pause") &&
            !!snapshot.duration
          }
          onSeek={(position) => send({ type: "seek", position })}
        />
        {/* Three columns so the transport stays centred whatever is beside it. */}
        <div class={streamUrl === null ? "controls" : "controls stream"}>
          <div class="extras">
            {streamUrl !== null && (
              <button
                aria-label="Play here"
                aria-pressed={listening}
                onClick={toggleListening}
              >
                <Icon path={HEADPHONES} />
              </button>
            )}
            <button
              aria-label="Volume"
              aria-expanded={volumeOpen}
              onClick={() => setVolumeOpen(!volumeOpen)}
            >
              <Icon path={VOLUME} />
            </button>
          </div>
          <Volume
            open={volumeOpen}
            volume={snapshot?.volume ?? null}
            disabled={disabled}
            onChange={(volume) => send({ type: "volume", volume })}
          />
          <div class="transport">
            <button
              data-flip="move"
              aria-label="Previous"
              disabled={disabled}
              onClick={() => send({ type: "previous" })}
            >
              <Icon path={PREVIOUS} />
            </button>
            <button
              class="toggle"
              data-flip="move"
              aria-label={snapshot?.state === "play" ? "Pause" : "Play"}
              disabled={disabled}
              onClick={() => send({ type: "toggle" })}
            >
              <Icon path={snapshot?.state === "play" ? PAUSE : PLAY} />
            </button>
            <button
              data-flip="move"
              aria-label="Next"
              disabled={disabled}
              onClick={() => send({ type: "next" })}
            >
              <Icon path={NEXT} />
            </button>
          </div>
          <div class="modes">
            {MODES.map(({ mode, label, path }) => (
              <button
                key={mode}
                class={mode}
                aria-label={label}
                aria-pressed={snapshot?.[mode] ?? false}
                disabled={disabled}
                onClick={() => send({ type: mode })}
              >
                <Icon path={path} />
              </button>
            ))}
          </div>
        </div>
      </section>
    </main>
  );
}

// Matches --switch-ms / --switch-ease in style.css (queue fade, chevron).
const SWITCH_MS = 350;
const SWITCH_EASE = "cubic-bezier(0.4, 0, 0.2, 1)";

/**
 * FLIP animation for a layout switch. Call the returned `capture` just
 * before changing `key`: it records where each `[data-flip]` element in
 * `root` is drawn. After the re-render each one animates from there to its
 * new place with transforms only, so it runs on the compositor.
 * `data-flip="scale"` also scales (art, bar); `"move"` only translates, so
 * text never stretches.
 */
function useFlip(root: RefObject<HTMLElement | null>, key: unknown) {
  const from = useRef<Map<HTMLElement, DOMRect> | null>(null);

  useLayoutEffect(() => {
    const first = from.current;
    from.current = null;
    if (first === null) return;
    for (const [element, before] of first) {
      const after = element.getBoundingClientRect();
      const scale = element.dataset.flip === "scale";
      const sx = scale ? before.width / after.width : 1;
      const sy = scale ? before.height / after.height : 1;
      const dx = before.left - after.left;
      const dy = before.top - after.top;
      element.animate(
        [
          {
            transformOrigin: "0 0",
            transform: `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})`,
          },
          { transformOrigin: "0 0", transform: "none" },
        ],
        { duration: SWITCH_MS, easing: SWITCH_EASE },
      );
    }
  }, [key]);

  return () => {
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const elements = Array.from(
      root.current!.querySelectorAll<HTMLElement>("[data-flip]"),
    );
    // Measured before cancelling: a switch mid-animation starts from where
    // things are drawn, so it reverses smoothly.
    from.current = new Map(
      elements.map((element) => [element, element.getBoundingClientRect()]),
    );
    for (const element of elements) {
      for (const animation of element.getAnimations()) animation.cancel();
    }
  };
}

/**
 * The cover URL for `folder`, `null` for none (placeholder), `undefined`
 * while unknown (probing, or before the first snapshot: `folder` is
 * `undefined`). Same folder, same result: a track change within an album
 * changes nothing.
 */
function useCover(folder: string | null | undefined) {
  // Only triggers the re-render; the result itself is read from `covers`.
  const [, setProbed] = useState<string | null>(null);
  useEffect(() => {
    if (!folder || covers.has(folder)) return;
    let current = true;
    findCover(folder).then(() => {
      if (current) setProbed(folder);
    });
    return () => {
      current = false;
    };
  }, [folder]);
  return folder === undefined
    ? undefined
    : folder === null
      ? null
      : covers.get(folder);
}

/**
 * While the cover is unknown the square stays empty (background only). Mini:
 * a tap centres the list on the current song.
 */
function Art({
  cover,
  onClick,
}: {
  cover: string | null | undefined;
  onClick?: () => void;
}) {
  return (
    <div class="art" data-flip="scale" onClick={onClick}>
      {cover !== undefined && <img src={cover ?? noCover} alt="" />}
    </div>
  );
}

// After a seek, the position sent stays shown until the next state push, so
// the bar doesn't jump back meanwhile; at most this long (MPD rejected it).
const SEEK_HOLD_MS = 1000;
const SEEK_STEP = 5;

/**
 * Drawn outside Preact: per-frame re-renders would be wasted work. While
 * `seekable`, tap or drag on the bar: it follows the pointer and seeks on
 * release.
 */
function Progress({
  snapshot,
  seekable,
  onSeek,
}: {
  snapshot: Snapshot | null;
  seekable: boolean;
  onSeek: (position: number) => void;
}) {
  const slider = useRef<HTMLDivElement>(null);
  const track = useRef<HTMLDivElement>(null);
  const fill = useRef<HTMLDivElement>(null);
  const time = useRef<HTMLSpanElement>(null);
  const tip = useRef<HTMLSpanElement>(null);
  const seek = useRef({
    /** Fraction of the bar under the pointer while dragging. */
    drag: null as number | null,
    /** The position sent, anchored like a snapshot's `elapsed`. */
    sent: null as { elapsed: number; at: number } | null,
    hold: 0,
    /** Seconds last drawn. */
    shown: null as number | null,
  });
  const paint = useRef(() => {});
  const [dragging, setDragging] = useState(false);
  const duration = snapshot?.duration ?? null;

  useLayoutEffect(() => {
    const s = seek.current;
    s.sent = null;
    let frame = 0;
    let text: string | null = null;
    const draw = () => {
      const seconds =
        snapshot === null
          ? null
          : s.drag !== null && duration
            ? s.drag * duration
            : position(
                s.sent ? { ...snapshot, ...s.sent } : snapshot,
                performance.now(),
              );
      s.shown = seconds;
      const ratio = seconds !== null && duration ? seconds / duration : 0;
      fill.current!.style.transform = `scaleX(${ratio})`;
      const next =
        seconds === null
          ? ""
          : duration === null
            ? formatTime(seconds)
            : `${formatTime(seconds)} / ${formatTime(duration)}`;
      if (next === text) return;
      time.current!.textContent = text = next;
      slider.current!.setAttribute(
        "aria-valuenow",
        String(Math.floor(seconds ?? 0)),
      );
      slider.current!.setAttribute("aria-valuetext", next);
    };
    const loop = () => {
      draw();
      frame = requestAnimationFrame(loop);
    };
    paint.current = draw;
    if (snapshot?.state === "play") loop();
    else draw();
    return () => cancelAnimationFrame(frame);
  }, [snapshot]);

  useEffect(() => () => window.clearTimeout(seek.current.hold), []);

  const send = (seconds: number) => {
    const s = seek.current;
    s.sent = { elapsed: seconds, at: performance.now() };
    window.clearTimeout(s.hold);
    s.hold = window.setTimeout(() => {
      s.sent = null;
      paint.current();
    }, SEEK_HOLD_MS);
    onSeek(seconds);
  };
  /** Starts or continues a drag at the pointer. */
  const dragTo = (e: PointerEvent) => {
    const r = track.current!.getBoundingClientRect();
    const f = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    seek.current.drag = f;
    tip.current!.textContent = `${Math.round(f * 100)}%`;
    tip.current!.style.setProperty("--at", `${f * 100}%`);
    paint.current();
  };
  const endDrag = (commit: boolean) => {
    const s = seek.current;
    if (s.drag === null) return;
    if (commit && seekable && duration) send(s.drag * duration);
    s.drag = null;
    setDragging(false);
    paint.current();
  };

  // E.g. switching to mini mode mid-drag.
  useEffect(() => {
    if (!seekable) endDrag(false);
  }, [seekable]);

  return (
    <div class="progress">
      <div
        class={dragging ? "seek dragging" : "seek"}
        ref={slider}
        role="slider"
        tabIndex={seekable ? 0 : -1}
        aria-label="Position"
        aria-valuemin={0}
        aria-valuemax={duration ?? undefined}
        aria-disabled={!seekable}
        onPointerDown={(e) => {
          if (!seekable || e.button !== 0) return;
          e.currentTarget.setPointerCapture(e.pointerId);
          dragTo(e);
          setDragging(true);
        }}
        onPointerMove={(e) => {
          if (seek.current.drag !== null) dragTo(e);
        }}
        onPointerUp={() => endDrag(true)}
        onPointerCancel={() => endDrag(false)}
        onKeyDown={(e) => {
          const step = SEEK_KEYS[e.key];
          const shown = seek.current.shown;
          if (!seekable || !duration || step === undefined || shown === null)
            return;
          e.preventDefault();
          send(Math.min(duration, Math.max(0, shown + step)));
          paint.current();
        }}
      >
        <div class="track" data-flip="scale" ref={track}>
          <div class="fill" ref={fill} />
        </div>
        <span class="tip" ref={tip} aria-hidden="true" />
      </div>
      <p class="time" data-flip="move">
        {/* Invisible, sizes the box: fits every time of this song, so
            ticking never resizes the bar. */}
        <span aria-hidden="true">{timeSizer(duration)}</span>
        <span ref={time} />
      </p>
    </div>
  );
}

const SEEK_KEYS: Record<string, number> = {
  ArrowLeft: -SEEK_STEP,
  ArrowDown: -SEEK_STEP,
  ArrowRight: SEEK_STEP,
  ArrowUp: SEEK_STEP,
};

/** The widest `formatTime` pair for `duration`: digits are tabular. */
function timeSizer(duration: number | null) {
  if (duration !== null && duration >= 36000) return "00:00:00 / 00:00:00";
  if (duration !== null && duration >= 3600) return "0:00:00 / 0:00:00";
  // Also any stream (elapsed only) under 100 hours.
  return "00:00 / 00:00";
}

const PREVIOUS = "M6 6h2v12H6zM9.5 12 18 6v12z";
const PLAY = "M8 5v14l11-7z";
const PAUSE = "M6 5h4v14H6zM14 5h4v14h-4z";
const NEXT = "M16 6h2v12h-2zM6 18l8.5-6L6 6z";
// Material Icons (Apache 2.0): expand_more; points up (rotated) in mini mode.
const EXPAND = "M16.59 8.59 12 13.17 7.41 8.59 6 10l6 6 6-6z";
// Material Icons (Apache 2.0): headphones.
const HEADPHONES =
  "M12 3c-4.97 0-9 4.03-9 9v7c0 1.1.9 2 2 2h4v-8H5v-1c0-3.87 3.13-7 7-7s7 3.13 7 7v1h-4v8h4c1.1 0 2-.9 2-2v-7c0-4.97-4.03-9-9-9z";
// Material Icons (Apache 2.0): volume_up.
const VOLUME =
  "M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z";
// Material Symbols (Apache 2.0): music_history (outlined), scaled to 24 × 24.
const ALBUM =
  "M10.5 17q-1.05 0-1.775-.725t-.725-1.775q0-1.05 .725-1.775t1.775-.725q.425 0 .8 .125t.7 .375v-5.5h4v2H13v5.5q0 1.05-.725 1.775t-1.775 .725Zm1.5 5q-2.075 0-3.9-.7875T4.925 19.075q-1.35-1.35-2.1375-3.175T2 12h2q0 1.65 .625 3.1125t1.7125 2.55q1.0875 1.0875 2.55 1.725T12 20.025q3.35 0 5.675-2.325t2.325-5.675q0-3.35-2.325-5.675t-5.675-2.325q-2.225 0-4.0375 1.0875T5.1 8h2.9v2H2v-6h2v2q1.375-1.825 3.45-2.9125T12 2q2.075 0 3.9 .7875T19.075 4.925q1.35 1.35 2.1375 3.175T22 12q0 2.075-.7875 3.9T19.075 19.075q-1.35 1.35-3.175 2.1375T12 22Z";
// Material Icons (Apache 2.0): shuffle, looks_one (outlined), repeat.
const MODES = [
  {
    mode: "random",
    label: "Random",
    path: "M10.59 9.17 5.41 4 4 5.41l5.17 5.17 1.42-1.41zM14.5 4l2.04 2.04L4 18.59 5.41 20 17.96 7.46 20 9.5V4h-5.5zm.33 9.41-1.41 1.41 3.13 3.13L14.5 20H20v-5.5l-2.04 2.04-3.13-3.13z",
  },
  {
    mode: "single",
    label: "Single",
    path: "M19 3H5c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h14c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm0 16H5V5h14v14zm-7-2h2V7h-4v2h2z",
  },
  {
    mode: "repeat",
    label: "Repeat",
    path: "M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4z",
  },
] as const;
