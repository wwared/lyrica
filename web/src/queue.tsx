import { useLayoutEffect, useReducer, useRef } from "preact/hooks";
import { formatTime } from "./progress";
import type { QueueStore } from "./queue-store";
import {
  firstRow,
  poolSize,
  reanchor,
  resize,
  ROW_PX,
  slotPosition,
  spaceRows,
} from "./virtual";

// Setting `scrollTop` cancels momentum scrolling (iOS), so the anchor only
// moves once scrolling has stopped this long with no touch held.
const SETTLE_MS = 150;

/**
 * The queue's virtual list (architecture.md → "Live queue"). Scroll state
 * lives in a ref, not in Preact state: the scroll handler re-renders only when
 * the first rendered row changes, or when entries arrived for rendered rows.
 */
export function QueueList({
  length,
  store,
}: {
  length: number;
  store: QueueStore;
}) {
  const list = useRef<HTMLDivElement>(null);
  const [, render] = useReducer((n: number) => n + 1, 0);
  const v = useRef({
    length,
    anchor: 0,
    scrollTop: 0,
    viewportPx: 0,
    // Rendered first space row, compared on scroll.
    first: 0,
    // Entries arrived for rendered rows since the last render.
    stale: false,
    // Written in the layout effect, together with the anchor it belongs to.
    pendingScrollTop: null as number | null,
  }).current;

  const report = () => {
    const offset = v.anchor * ROW_PX + v.scrollTop;
    store.view(
      Math.floor(offset / ROW_PX),
      Math.min(v.length, Math.ceil((offset + v.viewportPx) / ROW_PX)),
    );
  };

  if (length !== v.length) {
    const scrollTop = list.current?.scrollTop ?? 0;
    const next = resize(v.anchor, scrollTop, v.viewportPx, length);
    v.length = length;
    v.anchor = next.anchor;
    v.scrollTop = next.scrollTop;
    // Even an unchanged value would stop a momentum scroll.
    if (next.scrollTop !== scrollTop) v.pendingScrollTop = next.scrollTop;
  }

  useLayoutEffect(() => {
    if (v.pendingScrollTop !== null) {
      list.current!.scrollTop = v.pendingScrollTop;
      v.pendingScrollTop = null;
    }
    report();
  });

  useLayoutEffect(() => {
    const el = list.current!;
    let frame = 0;
    let settle = 0;
    let touches = 0;
    const pass = () => {
      frame = 0;
      v.scrollTop = el.scrollTop;
      report();
      const pool = poolSize(v.viewportPx, v.length);
      if (v.stale || firstRow(v.scrollTop, pool, v.length) !== v.first)
        render(0);
    };
    // Never renders here: replies arrive in the socket's message handler,
    // possibly just ahead of a state push.
    const unsubscribe = store.subscribe((start, end) => {
      const rendered = v.anchor + v.first;
      if (end <= rendered) return;
      if (start >= rendered + poolSize(v.viewportPx, v.length)) return;
      v.stale = true;
      if (frame === 0) frame = requestAnimationFrame(pass);
    });
    const settled = () => {
      if (touches > 0) return;
      const next = reanchor(v.anchor, el.scrollTop, v.length);
      if (next === null) return;
      v.anchor = next.anchor;
      v.scrollTop = next.scrollTop;
      v.pendingScrollTop = next.scrollTop;
      render(0);
    };
    const onScroll = () => {
      if (frame === 0) frame = requestAnimationFrame(pass);
      clearTimeout(settle);
      settle = setTimeout(settled, SETTLE_MS);
    };
    const onTouchStart = (e: TouchEvent) => (touches = e.touches.length);
    const onTouchEnd = (e: TouchEvent) => {
      touches = e.touches.length;
      clearTimeout(settle);
      settle = setTimeout(settled, SETTLE_MS);
    };
    const resized = new ResizeObserver((entries) => {
      v.viewportPx = entries[0]!.contentRect.height;
      render(0);
    });
    resized.observe(el);
    const passive = { passive: true };
    el.addEventListener("scroll", onScroll, passive);
    el.addEventListener("touchstart", onTouchStart, passive);
    el.addEventListener("touchend", onTouchEnd, passive);
    el.addEventListener("touchcancel", onTouchEnd, passive);
    return () => {
      unsubscribe();
      cancelAnimationFrame(frame);
      clearTimeout(settle);
      resized.disconnect();
      el.removeEventListener("scroll", onScroll);
      el.removeEventListener("touchstart", onTouchStart);
      el.removeEventListener("touchend", onTouchEnd);
      el.removeEventListener("touchcancel", onTouchEnd);
    };
  }, []);

  const pool = poolSize(v.viewportPx, length);
  v.first = firstRow(v.scrollTop, pool, length);
  v.stale = false;
  const first = v.anchor + v.first;
  const rows = [];
  for (let slot = 0; slot < pool; slot++) {
    const position = slotPosition(slot, first, pool);
    const entry = store.entry(position);
    const artist = entry?.artist;
    const album = entry?.album;
    const duration = entry?.duration;
    rows.push(
      <div
        key={slot}
        class="queue-row"
        role="listitem"
        aria-setsize={length}
        aria-posinset={position + 1}
        style={{ transform: `translateY(${(position - v.anchor) * ROW_PX}px)` }}
      >
        <span class="queue-title">{entry?.title ?? ""}</span>
        <span class="queue-info">
          {artist && album ? `${artist} · ${album}` : (artist ?? album ?? "")}
        </span>
        <span class="queue-time">
          {duration == null ? "" : formatTime(duration)}
        </span>
      </div>,
    );
  }
  return (
    <div class="queue-list" ref={list}>
      <div
        class="queue-space"
        role="list"
        style={{ height: `${spaceRows(length) * ROW_PX}px` }}
      >
        {rows}
      </div>
    </div>
  );
}
