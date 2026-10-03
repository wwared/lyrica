import { useEffect, useRef, useState } from "preact/hooks";

// While dragging, at most one `setvol` per SEND_MS.
const SEND_MS = 100;
// The dragged value stays shown this long after the last change, so the
// state pushes for the values sent meanwhile don't make the thumb jump back.
const HOLD_MS = 1000;
const STEP = 5;

/**
 * MPD's volume: a band over the album and progress rows, shown while `open`.
 * Tap or drag anywhere on it.
 */
export function Volume({
  open,
  volume,
  disabled,
  onChange,
}: {
  open: boolean;
  volume: number | null;
  disabled: boolean;
  onChange: (volume: number) => void;
}) {
  const [held, set] = useThrottled(onChange);
  const shown = held ?? volume;
  const rail = useRef<HTMLDivElement>(null);

  const change = (value: number) => {
    const v = Math.min(100, Math.max(0, Math.round(value)));
    if (v !== shown) set(v);
  };
  const fromPointer = (e: PointerEvent) => {
    const r = rail.current!.getBoundingClientRect();
    change(((e.clientX - r.left) / r.width) * 100);
  };
  const keys: Record<string, number> = {
    ArrowLeft: -STEP,
    ArrowDown: -STEP,
    ArrowRight: STEP,
    ArrowUp: STEP,
    Home: -100,
    End: 100,
  };

  return (
    <div class={open ? "volume open" : "volume"}>
      <div
        class="slider"
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label="Volume"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={shown ?? undefined}
        aria-disabled={disabled}
        onPointerDown={(e) => {
          if (disabled) return;
          e.currentTarget.setPointerCapture(e.pointerId);
          fromPointer(e);
        }}
        onPointerMove={(e) => {
          if (e.currentTarget.hasPointerCapture(e.pointerId)) fromPointer(e);
        }}
        onKeyDown={(e) => {
          const step = keys[e.key];
          if (disabled || step === undefined) return;
          e.preventDefault();
          change((shown ?? 0) + step);
        }}
      >
        <div class="rail" ref={rail}>
          <div class="track">
            <div
              class="fill"
              style={{ transform: `scaleX(${(shown ?? 0) / 100})` }}
            />
          </div>
          {shown !== null && (
            <div class="thumb" style={{ left: `${shown}%` }} />
          )}
        </div>
      </div>
      <p class="level">{shown ?? "–"}</p>
    </div>
  );
}

/**
 * `set` shows a value at once and sends it throttled, the last one always;
 * `held` is that value until HOLD_MS after the last `set`, else `null`.
 */
function useThrottled(send: (value: number) => void) {
  const [held, setHeld] = useState<number | null>(null);
  const s = useRef({ latest: 0, sentAt: -Infinity, trailing: 0, hold: 0 });
  useEffect(
    () => () => {
      window.clearTimeout(s.current.trailing);
      window.clearTimeout(s.current.hold);
    },
    [],
  );

  const flush = () => {
    s.current.trailing = 0;
    s.current.sentAt = performance.now();
    send(s.current.latest);
  };
  const set = (value: number) => {
    const t = s.current;
    t.latest = value;
    setHeld(value);
    window.clearTimeout(t.hold);
    t.hold = window.setTimeout(() => setHeld(null), HOLD_MS);
    if (t.trailing) return;
    const wait = t.sentAt + SEND_MS - performance.now();
    if (wait <= 0) flush();
    else t.trailing = window.setTimeout(flush, wait);
  };
  return [held, set] as const;
}
