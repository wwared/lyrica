import { useEffect } from "preact/hooks";

/**
 * Keeps the screen on while the page is visible. The system releases the
 * lock whenever the page is hidden, so it's requested again on return; a
 * refused request is retried on the next touch.
 */
export function useWakeLock() {
  useEffect(() => {
    if (!("wakeLock" in navigator)) return;
    let lock: WakeLockSentinel | null = null;
    let pending = false;
    let closed = false;
    const request = () => {
      if (pending || document.visibilityState !== "visible") return;
      if (lock !== null && !lock.released) return;
      pending = true;
      navigator.wakeLock.request("screen").then(
        (l) => {
          pending = false;
          if (closed) void l.release();
          else lock = l;
        },
        () => {
          pending = false;
        },
      );
    };
    request();
    document.addEventListener("visibilitychange", request);
    document.addEventListener("pointerdown", request);
    return () => {
      closed = true;
      document.removeEventListener("visibilitychange", request);
      document.removeEventListener("pointerdown", request);
      void lock?.release();
    };
  }, []);
}
