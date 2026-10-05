import type { Queue } from "./ws";

/** A queue entry as the server sends it; its position is implied by its block. */
export type Entry = {
  id: number | null;
  title: string;
  artist: string | null;
  album: string | null;
  track: number | null;
  duration: number | null;
  folder: string | null;
  /** Whether it starts an album group (the server's rule). */
  start: boolean;
};

/** A request for the entries at positions `start` to `end - 1`. */
export type Window = {
  type: "queue";
  version: number;
  start: number;
  end: number;
};

/** What the list hears from the store (architecture.md → "Wiring"). */
export type Listener = {
  /** Entries stored for positions `start` to `end - 1`. */
  stored(start: number, end: number): void;
  /** `state` changed the current song's position or id. */
  current(): void;
  /** `show(true)`, before planning: the list places itself and reports its view. */
  opening(): void;
  /** `show(false)`: the list stops its marquees. */
  hiding(): void;
  /** `top()`: the list jumps to its first row. */
  top(): void;
  /** `recentre()`: the list centres the current song and ends any pause. */
  recentre(): void;
};

/** Queue entries loaded on demand around the view (architecture.md → "Blocks"). */
export type QueueStore = {
  /** On every socket open: the sender for window requests; frees the slots. */
  opened(send: (window: Window) => void): void;
  /** Each snapshot's queue, `null` once the socket closed. */
  state(queue: Queue | null): void;
  /** A `queue` message. */
  reply(version: number, start: number, songs: Entry[] | null): void;
  /** The visible positions, `first` to `last - 1`. */
  view(first: number, last: number): void;
  /** Whether the list is shown (mini mode). */
  show(shown: boolean): void;
  /** After a stored playlist load: the list jumps to its first row. */
  top(): void;
  /** A tap on the mini player's cover or the album button: the list centres the current song. */
  recentre(): void;
  /** The current song from the last `state`; both `null` offline or with none. */
  readonly current: { position: number | null; id: number | null };
  /** The last `show`. */
  readonly shown: boolean;
  entry(position: number): Entry | undefined;
  /** Whether the entry starts an album group; `undefined` while not served. */
  starts(position: number): boolean | undefined;
  /** Returns the unsubscriber. */
  subscribe(listener: Listener): () => void;
};

/** Positions per block. */
export const BLOCK = 100;
/** Rows kept beyond the visible ones on each side. */
export const KEEP_ROWS = 1000;
/** Viewports loaded ahead on each side: entries here, covers in the list. */
export const AHEAD_VIEWPORTS = 2;
export const IN_FLIGHT = 2;
export const RETRY_MS = 1000;

export function queueStore(
  visible = () => document.visibilityState === "visible",
): QueueStore {
  let send: ((window: Window) => void) | null = null;
  let queue: Queue | null = null;
  let shown = false;
  let first = 0;
  let last = 0;
  // Block → its entries and their queue version; `[]` when MPD rejected the
  // range. Older versions are only kept in view, shown until replaced.
  const blocks = new Map<number, { version: number; songs: Entry[] }>();
  // Replies to these still count against the limit, even for an older version.
  let inFlight: { version: number; block: number }[] = [];
  // Blocks waiting to be retried after a `null` reply.
  const retries = new Map<number, number>();
  let current: QueueStore["current"] = { position: null, id: null };
  let listener: Listener | null = null;

  const served = (position: number) =>
    queue !== null && position < queue.length
      ? blocks.get(Math.floor(position / BLOCK))
      : undefined;

  const kept = (block: number) =>
    block * BLOCK < last + KEEP_ROWS && (block + 1) * BLOCK > first - KEEP_ROWS;

  // Keeps the blocks overlapping the visible rows when `keepVisible`.
  const drop = (keepVisible: boolean) => {
    for (const timer of retries.values()) clearTimeout(timer);
    retries.clear();
    let dropped = false;
    for (const [block, { songs }] of blocks)
      if (
        !keepVisible ||
        songs.length === 0 ||
        block * BLOCK >= last ||
        (block + 1) * BLOCK <= first
      ) {
        blocks.delete(block);
        dropped = true;
      }
    if (dropped) listener?.stored(0, Infinity);
  };

  const plan = () => {
    if (!shown || send === null || queue === null || !visible()) return;
    const { version, length } = queue;
    const top = Math.min(first, length);
    const bottom = Math.min(last, length);
    if (top >= bottom) return;
    for (const block of wanted(top, bottom, length)) {
      if (inFlight.length >= IN_FLIGHT) return;
      if (blocks.get(block)?.version === version || retries.has(block))
        continue;
      if (inFlight.some((r) => r.version === version && r.block === block))
        continue;
      inFlight.push({ version, block });
      send({
        type: "queue",
        version,
        start: block * BLOCK,
        end: Math.min(block * BLOCK + BLOCK, length),
      });
    }
  };

  return {
    opened(sender) {
      send = sender;
      inFlight = [];
    },
    state(next) {
      if (next === null || next.version !== queue?.version)
        drop(next !== null && queue !== null);
      queue = next;
      const position = next?.position ?? null;
      const id = next?.id ?? null;
      if (position !== current.position || id !== current.id) {
        current = { position, id };
        listener?.current();
      }
      plan();
    },
    reply(version, start, songs) {
      const block = start / BLOCK;
      const i = inFlight.findIndex(
        (r) => r.version === version && r.block === block,
      );
      if (i >= 0) inFlight.splice(i, 1);
      if (version === queue?.version && kept(block)) {
        if (songs === null) {
          retries.set(
            block,
            setTimeout(() => {
              retries.delete(block);
              plan();
            }, RETRY_MS),
          );
        } else {
          const replaced = blocks.has(block);
          blocks.set(block, { version, songs });
          if (songs.length > 0 || replaced)
            listener?.stored(start, start + (songs.length || BLOCK));
        }
      }
      plan();
    },
    view(top, bottom) {
      if (top === first && bottom === last) return;
      first = top;
      last = bottom;
      for (const block of blocks.keys()) if (!kept(block)) blocks.delete(block);
      for (const [block, timer] of retries)
        if (!kept(block)) {
          clearTimeout(timer);
          retries.delete(block);
        }
      plan();
    },
    show(next) {
      shown = next;
      if (next) listener?.opening();
      else listener?.hiding();
      plan();
    },
    top() {
      listener?.top();
    },
    recentre() {
      listener?.recentre();
    },
    get current() {
      return current;
    },
    get shown() {
      return shown;
    },
    entry(position) {
      return served(position)?.songs[position % BLOCK];
    },
    starts(position) {
      return served(position)?.songs[position % BLOCK]?.start;
    },
    subscribe(next) {
      listener = next;
      return () => {
        listener = null;
      };
    },
  };
}

/**
 * Blocks to load for the visible positions `first` to `last - 1`: the visible
 * ones, then those within `AHEAD_VIEWPORTS`, nearest first, ties below.
 */
function* wanted(first: number, last: number, length: number) {
  const top = Math.floor(first / BLOCK);
  const bottom = Math.floor((last - 1) / BLOCK);
  for (let block = top; block <= bottom; block++) yield block;
  const rows = (last - first) * AHEAD_VIEWPORTS;
  const lowest = Math.floor(Math.max(0, first - rows) / BLOCK);
  const highest = Math.floor((Math.min(length, last + rows) - 1) / BLOCK);
  let above = top - 1;
  let below = bottom + 1;
  while (above >= lowest || below <= highest) {
    const belowFirst =
      below <= highest &&
      (above < lowest || below * BLOCK - last <= first - (above + 1) * BLOCK);
    yield belowFirst ? below++ : above--;
  }
}
