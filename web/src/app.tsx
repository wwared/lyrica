import type { RefObject } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import noCover from "./assets/no-cover.png";
import { covers, findCover } from "./cover";
import { Marquee } from "./marquee";
import { formatTime, position } from "./progress";
import { connect, type Command, type Connection, type Snapshot } from "./ws";

export function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const connection = useRef<Connection | null>(null);
  useEffect(() => {
    const c = connect(setSnapshot);
    connection.current = c;
    return () => c.close();
  }, []);

  const send = (command: Command) => connection.current?.send(command);
  const offline = snapshot?.state === "offline";
  const disabled = snapshot === null || offline;
  const heading =
    snapshot === null
      ? "Connecting…"
      : offline
        ? "Offline"
        : (snapshot.title ?? "Nothing playing");
  const [mini, setMini] = useState(false);
  const nowPlaying = useRef<HTMLElement>(null);
  const capture = useFlip(nowPlaying, mini);

  return (
    <main class={mini ? "app mini" : "app"}>
      <section class="playlist" aria-hidden={!mini}>
        <p class="placeholder">Playlist</p>
      </section>
      <section class="now-playing" ref={nowPlaying}>
        <Art folder={snapshot === null ? undefined : snapshot.folder} />
        <h1
          class={snapshot?.title ? "title" : "title placeholder"}
          data-flip="move"
        >
          <Marquee text={heading} />
        </h1>
        <button
          class="switch"
          aria-label={mini ? "Full player" : "Playlist"}
          onClick={() => {
            capture();
            setMini(!mini);
          }}
        >
          <Icon path={EXPAND} />
        </button>
        <p class="artist" data-flip="move">
          <Marquee text={snapshot?.artist ?? ""} />
        </p>
        <Progress snapshot={snapshot} />
        {/* Three columns so the transport stays centred whatever is beside it. */}
        <div class="controls">
          <div class="transport">
            <button
              aria-label="Previous"
              disabled={disabled}
              onClick={() => send("previous")}
            >
              <Icon path={PREVIOUS} />
            </button>
            <button
              class="toggle"
              aria-label={snapshot?.state === "play" ? "Pause" : "Play"}
              disabled={disabled}
              onClick={() => send("toggle")}
            >
              <Icon path={snapshot?.state === "play" ? PAUSE : PLAY} />
            </button>
            <button
              aria-label="Next"
              disabled={disabled}
              onClick={() => send("next")}
            >
              <Icon path={NEXT} />
            </button>
          </div>
          <div class="modes">
            {MODES.map(({ mode, label, path }) => (
              <button
                key={mode}
                aria-label={label}
                aria-pressed={snapshot?.[mode] ?? false}
                disabled={disabled}
                onClick={() => send(mode)}
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

// Matches --switch-ms / --switch-ease in style.css (playlist fade, chevron).
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
 * `folder` is `undefined` before the first snapshot. While the cover is
 * unknown the square stays empty (background only); then the cover, or the
 * placeholder when there is none. Same folder, same image: a track change
 * within an album changes nothing.
 */
function Art({ folder }: { folder: string | null | undefined }) {
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

  const cover =
    folder === undefined
      ? undefined
      : folder === null
        ? null
        : covers.get(folder);
  return (
    <div class="art" data-flip="scale">
      {cover !== undefined && <img src={cover ?? noCover} alt="" />}
    </div>
  );
}

/** Drawn outside Preact: per-frame re-renders would be wasted work. */
function Progress({ snapshot }: { snapshot: Snapshot | null }) {
  const fill = useRef<HTMLDivElement>(null);
  const time = useRef<HTMLParagraphElement>(null);

  useLayoutEffect(() => {
    let frame = 0;
    let shown: string | null = null;
    const draw = () => {
      const seconds = snapshot && position(snapshot, performance.now());
      const duration = snapshot?.duration ?? null;
      const ratio = seconds !== null && duration ? seconds / duration : 0;
      fill.current!.style.transform = `scaleX(${ratio})`;
      const text =
        seconds === null
          ? ""
          : duration === null
            ? formatTime(seconds)
            : `${formatTime(seconds)} / ${formatTime(duration)}`;
      if (text !== shown) time.current!.textContent = shown = text;
      if (snapshot?.state === "play") frame = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(frame);
  }, [snapshot]);

  return (
    <div class="progress">
      <div class="track" data-flip="scale">
        <div class="fill" ref={fill} />
      </div>
      <p class="time" ref={time} data-flip="move" />
    </div>
  );
}

const PREVIOUS = "M6 6h2v12H6zM9.5 12 18 6v12z";
const PLAY = "M8 5v14l11-7z";
const PAUSE = "M6 5h4v14H6zM14 5h4v14h-4z";
const NEXT = "M16 6h2v12h-2zM6 18l8.5-6L6 6z";
// Material Icons (Apache 2.0): expand_more; points up (rotated) in mini mode.
const EXPAND = "M16.59 8.59 12 13.17 7.41 8.59 6 10l6 6 6-6z";
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

function Icon({ path }: { path: string }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d={path} />
    </svg>
  );
}
