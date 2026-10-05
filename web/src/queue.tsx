import { useLayoutEffect, useReducer, useRef } from "preact/hooks";
import { covers, findCover } from "./cover";
import { formatTime } from "./progress";
import { AHEAD_VIEWPORTS, type QueueStore } from "./queue-store";
import {
  albumSpans,
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
// Faster, rows cross the list before their covers could arrive.
const AHEAD_ROWS_PER_S = 100;
// Scroll speed is measured over this long.
const SPEED_MS = 100;
// Scrolling passes run every frame: none for this long means stopped.
const STOPPED_MS = 50;
// A 580 px list's groups, so the visible ones never wait.
const AHEAD_PROBES = 12;

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
  // iOS's rubber band reports positions beyond the space's ends; there are no
  // rows there.
  const scrolled = (el: HTMLElement) =>
    Math.max(
      0,
      Math.min(el.scrollTop, spaceRows(v.length) * ROW_PX - v.viewportPx),
    );

  const report = () => {
    const offset = v.anchor * ROW_PX + v.scrollTop;
    store.view(
      Math.floor(offset / ROW_PX),
      Math.min(v.length, Math.ceil((offset + v.viewportPx) / ROW_PX)),
    );
  };

  if (length !== v.length) {
    const scrollTop = list.current ? scrolled(list.current) : 0;
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
    // Recent [time, queue offset] pairs, flat, oldest first.
    const moves: number[] = [];
    // Of the last movement: 1 down, -1 up.
    let direction = 1;
    let slowed = 0;
    const probing = new Set<string>();
    let disposed = false;
    // User input on the list stops auto-centring until `resume` fires.
    let paused = false;
    let resume = 0;
    let unhalt = 0;
    const place = (next: Placement) => {
      v.anchor = next.anchor;
      v.scrollTop = next.scrollTop;
      v.pendingScrollTop = next.scrollTop;
    };
    const centred = () => {
      const { position } = store.current;
      if (position === null) return;
      place(centre(position, v.viewportPx, v.length));
      // A jump, not scrolling.
      moves.length = 0;
    };
    const pass = () => {
      frame = 0;
      v.scrollTop = scrolled(el);
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
    // Found or not, the freed slot goes to the nearest unknown folder then.
    const probed = (folder: string, cover: string | null) => {
      probing.delete(folder);
      if (disposed) return;
      if (cover !== null) v.stale = true;
      if (frame === 0) frame = requestAnimationFrame(pass);
    };
    const tooFast = (offset: number) => {
      const now = performance.now();
      const n = moves.length;
      if (n > 0 && offset !== moves[n - 1]) {
        direction = offset > moves[n - 1]! ? 1 : -1;
      }
      // After a pause, the previous position counts as SPEED_MS ago.
      let since = n > 0 ? now - SPEED_MS : now;
      let from = n > 0 ? moves[n - 1]! : offset;
      let drop = 0;
      while (drop < n && moves[drop]! <= now - SPEED_MS) drop += 2;
      moves.splice(0, drop);
      if (moves.length > 0) {
        since = moves[0]!;
        from = moves[1]!;
      }
      moves.push(now, offset);
      const pxPerMs = Math.abs(offset - from) / Math.max(now - since, 16);
      return pxPerMs > (AHEAD_ROWS_PER_S * ROW_PX) / 1000;
    };
    // The folders of the visible rows, then those within AHEAD_VIEWPORTS on
    // the scroll's side, then the other side, nearest first. Rows of a group
    // share its folder, so this covers groups started outside the range too.
    const probe = () => {
      if (!store.shown || disposed) return;
      const offset = v.anchor * ROW_PX + v.scrollTop;
      clearTimeout(slowed);
      if (tooFast(offset)) {
        slowed = setTimeout(stopped, STOPPED_MS);
        return;
      }
      const first = Math.floor(offset / ROW_PX);
      const last = Math.min(
        v.length,
        Math.ceil((offset + v.viewportPx) / ROW_PX),
      );
      const rows = (last - first) * AHEAD_VIEWPORTS;
      const start = (p: number) => {
        const folder = store.entry(p)?.folder;
        if (!folder || covers.has(folder) || probing.has(folder)) return;
        probing.add(folder);
        findCover(folder).then((cover) => probed(folder, cover));
      };
      const below = () => {
        const end = Math.min(v.length, last + rows);
        for (let p = last; p < end && probing.size < AHEAD_PROBES; p++)
          start(p);
      };
      const above = () => {
        const end = Math.max(0, first - rows);
        for (let p = first - 1; p >= end && probing.size < AHEAD_PROBES; p--)
          start(p);
      };
      for (let p = first; p < last && probing.size < AHEAD_PROBES; p++)
        start(p);
      if (direction > 0) {
        below();
        above();
      } else {
        above();
        below();
      }
    };
    const stopped = () => {
      moves.length = 0;
      probe();
    };
    const unsettle = () => {
      clearTimeout(settle);
      settle = setTimeout(settled, SETTLE_MS);
    };
    // iOS keeps a fling going through `scrollTop` writes; a painted frame with
    // `overflow: hidden` ends it. Two frames: the first runs before this one's
    // paint.
    const halt = () => {
      el.style.overflowY = "hidden";
      cancelAnimationFrame(unhalt);
      unhalt = requestAnimationFrame(() => {
        unhalt = requestAnimationFrame(() => {
          el.style.overflowY = "";
        });
      });
    };
    const recentre = () => {
      halt();
      paused = false;
      clearTimeout(resume);
      centred();
      render(0);
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
        probe();
      },
      top() {
        paused = false;
        clearTimeout(resume);
        place({ anchor: 0, scrollTop: 0 });
        moves.length = 0;
        render(0);
      },
      recentre,
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
      const next = reanchor(v.anchor, scrolled(el), v.length);
      if (next !== null) {
        place(next);
        render(0);
      }
    };
    const onScroll = () => {
      if (frame === 0) frame = requestAnimationFrame(pass);
      unsettle();
      if (paused) wait();
    };
    const onTouchStart = (e: TouchEvent) => {
      touches = e.touches.length;
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
      // `playid` would restart the current song.
      if (entry.id === store.current.id) return recentre();
      paused = false;
      clearTimeout(resume);
      send({ type: "playid", id: entry.id });
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
      clearTimeout(slowed);
      clearTimeout(resume);
      cancelAnimationFrame(unhalt);
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
    // By position, not slot: a song keeps its shade across re-anchors.
    let className = position % 2 ? "queue-row odd" : "queue-row";
    if (currentId !== null && entry?.id === currentId) className += " current";
    if (position > 0 && store.starts(position)) className += " group";
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
  // Keyed by album, so a cover keeps its element while its span changes; the
  // count tells apart an album found twice in the pool.
  const albums = [];
  const seen = new Map<string, number>();
  for (const span of albumSpans(first, first + pool, (p) => store.starts(p))) {
    const { folder, album } = store.entry(span.start)!;
    const cover = folder ? covers.get(folder) : undefined;
    if (!cover) continue;
    const id = `${folder}\n${album}`;
    const n = seen.get(id) ?? 0;
    seen.set(id, n + 1);
    albums.push(
      <div
        key={`${id}\n${n}`}
        class="queue-album"
        style={{
          top: `${(span.start - v.anchor) * ROW_PX}px`,
          height: `${(span.end - span.start) * ROW_PX}px`,
        }}
      >
        <img class="queue-cover" alt="" decoding="async" src={cover} />
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
        <div aria-hidden="true">{albums}</div>
      </div>
    </div>
  );
}
