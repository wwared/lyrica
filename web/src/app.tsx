import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
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
        {/* Keyed so a new song starts scrolled back to the beginning. */}
        <h1
          key={heading}
          class={snapshot?.title ? "line title" : "line title placeholder"}
        >
          {heading}
        </h1>
        <p key={snapshot?.artist} class="line artist">
          {snapshot?.artist}
        </p>
        <Progress snapshot={snapshot} />
        <div class="controls">
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
      </section>
    </main>
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

function Icon({ path }: { path: string }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d={path} />
    </svg>
  );
}
