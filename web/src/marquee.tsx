import { useLayoutEffect, useRef } from "preact/hooks";

const PAUSE_MS = 1000;
const PX_PER_S = 50;

/**
 * One line of text. If it is too wide, it waits, scrolls until its end is
 * visible, waits, scrolls back, and repeats. Restarts from the beginning
 * when the text changes.
 */
export function Marquee({ text }: { text: string }) {
  const box = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLSpanElement>(null);

  useLayoutEffect(() => {
    const outer = box.current!;
    const inner = content.current!;
    let animation: Animation | undefined;
    let distance = 0;
    // Also runs on resize: rotation, and the web font replacing the fallback.
    const fit = () => {
      const overflow = Math.max(0, inner.offsetWidth - outer.clientWidth);
      if (overflow === distance) return;
      distance = overflow;
      animation?.cancel();
      if (distance === 0) return;
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
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(outer);
    observer.observe(inner);
    return () => {
      observer.disconnect();
      animation?.cancel();
    };
  }, [text]);

  return (
    <div class="marquee" ref={box}>
      <span ref={content}>{text}</span>
    </div>
  );
}
