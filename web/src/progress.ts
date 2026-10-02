import type { Snapshot } from "./ws";

/** Playback position in seconds at local time `now`, extrapolated while playing. */
export function position(snapshot: Snapshot, now: number): number | null {
  const { state, elapsed, duration, at } = snapshot;
  if (elapsed === null) return null;
  if (state !== "play") return elapsed;
  const extrapolated = elapsed + Math.max(0, now - at) / 1000;
  return duration === null ? extrapolated : Math.min(extrapolated, duration);
}

/** `m:ss`, or `h:mm:ss` from one hour. */
export function formatTime(seconds: number): string {
  const total = Math.floor(seconds);
  const s = String(total % 60).padStart(2, "0");
  const m = Math.floor(total / 60) % 60;
  const h = Math.floor(total / 3600);
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}
