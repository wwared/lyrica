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

  return (
    <main class="app">
      <section class="now-playing">
        <Art folder={snapshot?.folder ?? null} />
        <h1 class={snapshot?.title ? "title" : "title placeholder"}>
          <Marquee text={heading} />
        </h1>
        <p class="artist">
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

/**
 * Same folder, same image: a track change within an album changes nothing.
 * A new folder shows the placeholder until its cover is found and decoded.
 */
function Art({ folder }: { folder: string | null }) {
  // Only triggers the re-render; the URL itself is read from `covers`.
  const [, setFound] = useState<string | null>(null);
  useEffect(() => {
    if (folder === null || covers.has(folder)) return;
    let current = true;
    findCover(folder).then((url) => {
      if (current && url !== null) setFound(url);
    });
    return () => {
      current = false;
    };
  }, [folder]);

  return (
    <div class="art">
      <img src={(folder !== null && covers.get(folder)) || noCover} alt="" />
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
      <div class="track">
        <div class="fill" ref={fill} />
      </div>
      <p class="time" ref={time} />
    </div>
  );
}

const PREVIOUS = "M6 6h2v12H6zM9.5 12 18 6v12z";
const PLAY = "M8 5v14l11-7z";
const PAUSE = "M6 5h4v14H6zM14 5h4v14h-4z";
const NEXT = "M16 6h2v12h-2zM6 18l8.5-6L6 6z";
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
