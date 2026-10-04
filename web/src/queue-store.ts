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

/** Positions per block. Requests add the entry before the block. */
export const BLOCK = 100;
/** Rows kept beyond the visible ones on each side. */
export const KEEP_ROWS = 1000;
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
  // Block → the entries from `start` (one before the block, but in block 0),
  // and their queue version; `[]` when MPD rejected the range. Older versions
  // are only kept in view, shown until replaced.
  const blocks = new Map<
    number,
    { version: number; start: number; songs: Entry[] }
  >();
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
      const start = Math.max(0, block * BLOCK - 1);
      send({
        type: "queue",
        version,
        start,
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
      const block = Math.ceil(start / BLOCK);
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
          blocks.set(block, { version, start, songs });
          if (songs.length > 0 || replaced)
            listener?.stored(
              block * BLOCK,
              songs.length > 0 ? start + songs.length : block * BLOCK + BLOCK,
            );
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
      plan();
    },
    get current() {
      return current;
    },
    get shown() {
      return shown;
    },
    entry(position) {
      const b = served(position);
      return b?.songs[position - b.start];
    },
    starts(position) {
      const b = served(position);
      if (b === undefined) return undefined;
      const entry = b.songs[position - b.start];
      if (entry === undefined) return undefined;
      const before = b.songs[position - b.start - 1];
      return (
        before === undefined ||
        entry.album === null ||
        entry.album !== before.album ||
        entry.folder !== before.folder
      );
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
 * ones, then those within one viewport, nearest first, ties below.
 */
function* wanted(first: number, last: number, length: number) {
  const top = Math.floor(first / BLOCK);
  const bottom = Math.floor((last - 1) / BLOCK);
  for (let block = top; block <= bottom; block++) yield block;
  const rows = last - first;
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
