import { useLayoutEffect, useRef } from "preact/hooks";

const PAUSE_MS = 1000;
const PX_PER_S = 50;

/**
 * One line of text. If it is too wide, it waits, scrolls until its end is
 * visible, waits, scrolls back, and repeats. Restarts from the beginning when
 * the text changes, when it comes on screen and when `paused` turns off.
 * Stopped while off screen, frozen in place while `paused`.
 */
export function Marquee({
  text,
  paused = false,
}: {
  text: string;
  paused?: boolean;
}) {
  const box = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLSpanElement>(null);
  const control = useRef<{ restart(): void; pause(paused: boolean): void }>({
    restart() {},
    pause() {},
  });

  useLayoutEffect(() => {
    const outer = box.current!;
    const inner = content.current!;
    let animation: Animation | undefined;
    let distance = 0;
    let onScreen = false;
    let held = false;
    const play = () => {
      animation?.cancel();
      animation = undefined;
      if (distance === 0 || !onScreen || held) return;
      const scroll = (distance / PX_PER_S) * 1000;
      const total = 2 * (PAUSE_MS + scroll);
      const start = "translateX(0)";
      const end = `translateX(${-distance}px)`;
      animation = inner.animate(
        [
          { transform: start, offset: 0 },
          { transform: start, offset: PAUSE_MS / total },
          { transform: end, offset: (PAUSE_MS + scroll) / total },
          { transform: end, offset: (2 * PAUSE_MS + scroll) / total },
          { transform: start, offset: 1 },
        ],
        { duration: total, iterations: Infinity },
      );
    };
    // Measured only here, after layout, so no render forces one. Also runs on
    // resize: rotation, and the web font replacing the fallback.
    const fit = () => {
      const overflow = Math.max(0, inner.offsetWidth - outer.clientWidth);
      if (overflow === distance) return;
      distance = overflow;
      play();
    };
    const resized = new ResizeObserver(fit);
    resized.observe(outer);
    resized.observe(inner);
    // Off-screen animations would still cost a style update every frame; and
    // a paused one keeps its layer, so they go entirely.
    const seen = new IntersectionObserver((entries) => {
      onScreen = entries.at(-1)!.isIntersecting;
      if (onScreen) play();
      else animation?.cancel();
    });
    seen.observe(outer);
    control.current = {
      restart: play,
      // Frozen, not cancelled: a jump back would show while the list fades.
      pause(next) {
        held = next;
        if (held) animation?.pause();
        else play();
      },
    };
    return () => {
      resized.disconnect();
      seen.disconnect();
      animation?.cancel();
    };
  }, []);
  // The new text's width, if it differs, reaches `fit` before the paint.
  useLayoutEffect(() => control.current.restart(), [text]);
  useLayoutEffect(() => control.current.pause(paused), [paused]);

  return (
    <div class="marquee" ref={box}>
      <span ref={content}>{text}</span>
    </div>
  );
}
