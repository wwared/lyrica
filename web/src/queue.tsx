import { useLayoutEffect, useReducer, useRef } from "preact/hooks";
import { covers, findCover } from "./cover";
import { formatTime } from "./progress";
import type { QueueStore } from "./queue-store";
import {
  centre,
  firstRow,
  type Placement,
  poolSize,
  reanchor,
  resize,
  ROW_PX,
  slotPosition,
  spaceRows,
} from "./virtual";
import type { Command } from "./ws";

// Setting `scrollTop` cancels momentum scrolling (iOS), so the anchor only
// moves once scrolling has stopped this long with no touch held.
const SETTLE_MS = 150;
// Auto-centring resumes this long after the last scroll by the user.
const RESUME_MS = 15_000;

/**
 * The queue's virtual list (architecture.md → "Live queue"). Scroll state
 * lives in a ref, not in Preact state: the scroll handler re-renders only when
 * the first rendered row changes, or when entries arrived for rendered rows.
 * Tapping a row plays it.
 */
export function QueueList({
  length,
  store,
  send,
}: {
  length: number;
  store: QueueStore;
  /** Stable for the list's lifetime. */
  send: (command: Command) => void;
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
    // The current song changed since the last pass.
    moved: false,
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
    // No scroll for SETTLE_MS and no touch held: covers may be probed.
    let still = false;
    let disposed = false;
    // User input on the list stops auto-centring until `resume` fires.
    let paused = false;
    let resume = 0;
    const place = (next: Placement) => {
      v.anchor = next.anchor;
      v.scrollTop = next.scrollTop;
      v.pendingScrollTop = next.scrollTop;
    };
    const centred = () => {
      const { position } = store.current;
      if (position !== null) place(centre(position, v.viewportPx, v.length));
    };
    const pass = () => {
      frame = 0;
      v.scrollTop = el.scrollTop;
      if (v.moved) {
        v.moved = false;
        v.stale = true;
        if (store.shown && !paused) centred();
      }
      report();
      const pool = poolSize(v.viewportPx, v.length);
      if (v.stale || firstRow(v.scrollTop, pool, v.length) !== v.first)
        render(0);
      probe();
    };
    // A miss shows nothing new: the track number stays.
    const arrived = (cover: string | null) => {
      if (cover === null || disposed) return;
      v.stale = true;
      if (frame === 0) frame = requestAnimationFrame(pass);
    };
    // Visible group starts only, not the pool's margin rows.
    const probe = () => {
      if (!still || !store.shown) return;
      const offset = v.anchor * ROW_PX + v.scrollTop;
      const last = Math.min(
        v.length,
        Math.ceil((offset + v.viewportPx) / ROW_PX),
      );
      for (let p = Math.floor(offset / ROW_PX); p < last; p++) {
        const folder = store.entry(p)?.folder;
        if (folder && !covers.has(folder) && store.starts(p))
          findCover(folder).then(arrived);
      }
    };
    const unsettle = () => {
      still = false;
      clearTimeout(settle);
      settle = setTimeout(settled, SETTLE_MS);
    };
    // `stored` and `current` never render here: they come from the socket's
    // message handler, ahead of now playing's render.
    const unsubscribe = store.subscribe({
      stored(start, end) {
        const rendered = v.anchor + v.first;
        if (end <= rendered) return;
        if (start >= rendered + poolSize(v.viewportPx, v.length)) return;
        v.stale = true;
        if (frame === 0) frame = requestAnimationFrame(pass);
      },
      current() {
        v.moved = true;
        if (frame === 0) frame = requestAnimationFrame(pass);
      },
      opening() {
        paused = false;
        clearTimeout(resume);
        centred();
        report();
        render(0);
        unsettle();
      },
    });
    const resumed = () => {
      if (touches > 0) return;
      paused = false;
      if (!store.shown) return;
      centred();
      render(0);
    };
    const wait = () => {
      clearTimeout(resume);
      resume = setTimeout(resumed, RESUME_MS);
    };
    const pause = () => {
      paused = true;
      wait();
    };
    const settled = () => {
      if (touches > 0) return;
      still = true;
      const next = reanchor(v.anchor, el.scrollTop, v.length);
      if (next !== null) {
        place(next);
        render(0);
      }
      probe();
    };
    const onScroll = () => {
      if (frame === 0) frame = requestAnimationFrame(pass);
      unsettle();
      if (paused) wait();
    };
    const onTouchStart = (e: TouchEvent) => {
      touches = e.touches.length;
      still = false;
      pause();
    };
    const onTouchEnd = (e: TouchEvent) => {
      touches = e.touches.length;
      unsettle();
      if (paused) wait();
    };
    // No `click` follows a touch that scrolled or stopped a fling. The tap
    // ends the pause its `touchstart` began, so the song change centres.
    const onClick = (e: MouseEvent) => {
      const row = (e.target as Element).closest(".queue-row");
      const entry = row && store.entry(Number(row.ariaPosInSet) - 1);
      if (!entry || entry.id === null) return;
      paused = false;
      clearTimeout(resume);
      // `playid` would restart the current song.
      if (entry.id !== store.current.id)
        return send({ type: "playid", id: entry.id });
      centred();
      render(0);
    };
    // Middle-click autoscroll.
    const onPointerDown = (e: PointerEvent) => {
      if (e.pointerType === "mouse") pause();
    };
    const resized = new ResizeObserver((entries) => {
      v.viewportPx = entries[0]!.contentRect.height;
      render(0);
      probe();
    });
    resized.observe(el);
    const passive = { passive: true };
    el.addEventListener("scroll", onScroll, passive);
    el.addEventListener("touchstart", onTouchStart, passive);
    el.addEventListener("touchend", onTouchEnd, passive);
    el.addEventListener("touchcancel", onTouchEnd, passive);
    el.addEventListener("wheel", pause, passive);
    el.addEventListener("pointerdown", onPointerDown, passive);
    el.addEventListener("keydown", pause, passive);
    el.addEventListener("click", onClick);
    return () => {
      disposed = true;
      unsubscribe();
      cancelAnimationFrame(frame);
      clearTimeout(settle);
      clearTimeout(resume);
      resized.disconnect();
      el.removeEventListener("scroll", onScroll);
      el.removeEventListener("touchstart", onTouchStart);
      el.removeEventListener("touchend", onTouchEnd);
      el.removeEventListener("touchcancel", onTouchEnd);
      el.removeEventListener("wheel", pause);
      el.removeEventListener("pointerdown", onPointerDown);
      el.removeEventListener("keydown", pause);
      el.removeEventListener("click", onClick);
    };
  }, []);

  const pool = poolSize(v.viewportPx, length);
  v.first = firstRow(v.scrollTop, pool, length);
  v.stale = false;
  const first = v.anchor + v.first;
  const currentId = store.current.id;
  const rows = [];
  for (let slot = 0; slot < pool; slot++) {
    const position = slotPosition(slot, first, pool);
    const entry = store.entry(position);
    const artist = entry?.artist;
    const album = entry?.album;
    const duration = entry?.duration;
    const starts = store.starts(position);
    const cover =
      starts && entry!.folder ? covers.get(entry!.folder) : undefined;
    let className = "queue-row";
    if (currentId !== null && entry?.id === currentId) className += " current";
    if (position > 0 && starts) className += " group";
    if (cover) className += " cover";
    rows.push(
      <div
        key={slot}
        class={className}
        role="listitem"
        aria-setsize={length}
        aria-posinset={position + 1}
        style={{ transform: `translateY(${(position - v.anchor) * ROW_PX}px)` }}
      >
        <span class="queue-track">{entry?.track ?? ""}</span>
        <img
          class="queue-cover"
          alt=""
          decoding="async"
          src={cover ?? undefined}
        />
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
