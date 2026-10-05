// Search mode's state (architecture.md → "Search" → "Frontend") as pure
// transitions: a user action or a reply in, the next state and what to do out.

import type { SearchView } from "./queue-store";

/** Matches per chunk; chunk starts are multiples of it. */
export const CHUNK = 500;

export const NO_MATCHES = "No matches";
export const UNSERVED = "Couldn't load matches";
export const LOST = "Connection lost";

export type Request =
  | { type: "search"; id: number; query: string; from: number | null }
  | { type: "matches"; id: number; query: string; start: number }
  | { type: "searchend" };

export type Reply =
  | {
      type: "matches";
      id: number;
      version: number | null;
      start: number;
      positions: number[] | null;
      first: number | null;
    }
  | { type: "searchfailed"; id: number; error: string }
  | { type: "searchstale"; id: number }
  /** After the reply with `first`: the count. */
  | { type: "total"; id: number; version: number; total: number }
  /** The socket closed. */
  | { type: "lost" };

/** ▲ previous, ▼ next. */
export type Direction = -1 | 1;

type Pending =
  /** `direction` 0: a submission, focusing `first`; else a rerun continuing in it. */
  | { kind: "search"; from: number | null; direction: Direction | 0 }
  /** A chunk fetch for match `target`. */
  | { kind: "fetch"; target: number; direction: Direction };

export type Search = {
  /** Of the latest request; replies to others are dropped. */
  id: number;
  query: string;
  /** The queue version the held chunks were found at. */
  version: number | null;
  /** Chunk start → its matches' positions, ascending. */
  chunks: ReadonlyMap<number, readonly number[]>;
  /** The number of matches, once a chunk reveals it. */
  total: number | null;
  /** The focused match's index and position. */
  focused: number | null;
  at: number | null;
  pending: Pending | null;
  /** The queue changed since the matches were found: ▲▼ rerun. */
  stale: boolean;
  /** The held chunks predate the pending rerun; shown until its first reply. */
  old: boolean;
};

/** From the latest snapshot. */
export type Queue = { position: number | null; version: number | null };

export type Step = {
  search: Search;
  send?: Request;
  toast?: string;
  /** Centre this queue position. */
  reveal?: number;
};

export const IDLE: Search = {
  id: 0,
  query: "",
  version: null,
  chunks: new Map(),
  total: null,
  focused: null,
  at: null,
  pending: null,
  stale: false,
  old: false,
};

const chunkOf = (index: number) => index - (index % CHUNK);

/** Match `index`'s position; `undefined` unless its chunk is held. */
export const position = (s: Search, index: number) =>
  s.chunks.get(chunkOf(index))?.[index % CHUNK];

/** A short chunk ends the matches, as does an empty one after a full one. */
function totalOf(chunks: ReadonlyMap<number, readonly number[]>) {
  for (const [start, positions] of chunks) {
    const n = positions.length;
    if (n > 0 && n < CHUNK) return start + n;
    if (n === 0 && (start === 0 || chunks.get(start - CHUNK)?.length === CHUNK))
      return start;
  }
  return null;
}

/** `3 / 120`; `…` for a total not known yet. */
export const count = (s: Search) =>
  s.focused === null ? "" : `${s.focused + 1} / ${s.total ?? "…"}`;

/** ▲ on the first match needs the last one to wrap to. */
export const canGoBack = (s: Search) =>
  s.focused !== 0 || s.total !== null || s.stale;

/** Answers the list's row questions; positions ascend across chunks. */
export function view(s: Search): SearchView {
  const sorted = [...s.chunks.values()]
    .filter((p) => p.length > 0)
    .sort((a, b) => a[0]! - b[0]!);
  return {
    focused: s.at,
    matches(position) {
      for (const p of sorted) {
        if (position < p[0]!) return false;
        if (position > p[p.length - 1]!) continue;
        let lo = 0;
        let hi = p.length - 1;
        while (lo <= hi) {
          const mid = (lo + hi) >> 1;
          if (p[mid]! < position) lo = mid + 1;
          else if (p[mid]! > position) hi = mid - 1;
          else return true;
        }
        return false;
      }
      return false;
    },
  };
}

/** Return or 🔍: a fresh search from the current song; blank does nothing. */
export function submit(s: Search, query: string, queue: Queue): Step {
  if (query.trim() === "") return { search: s };
  const id = s.id + 1;
  const from = queue.position;
  return {
    search: {
      ...IDLE,
      id,
      query,
      pending: { kind: "search", from, direction: 0 },
    },
    send: { type: "search", id, query, from },
  };
}

/** ▲ or ▼, wrapping at the ends. */
export function step(s: Search, direction: Direction): Step {
  if (s.pending !== null || s.focused === null) return { search: s };
  if (s.stale) return rerun(s, direction);
  let target = s.focused + direction;
  if (target === s.total) target = 0;
  if (target < 0) {
    if (s.total === null) return { search: s };
    target = s.total - 1;
  }
  return go(s, target, direction);
}

/** Every reply; `lost` too. */
export function reply(s: Search, r: Reply, queue: Queue): Step {
  if (r.type === "lost") {
    if (s.pending === null && s.focused === null) return { search: s };
    const search =
      s.focused === null
        ? { ...IDLE, id: s.id }
        : { ...s, pending: null, stale: true, old: false };
    return s.pending === null ? { search } : { search, toast: LOST };
  }
  if (r.id !== s.id) return { search: s };
  const pending = s.pending;
  // After the reply with `first`, while the server counts.
  if (r.type === "total") {
    if (r.version !== s.version || s.old) return { search: s };
    return { search: { ...s, total: r.total } };
  }
  if (r.type === "searchstale") {
    if (pending?.kind === "search") {
      const from = pending.direction === 0 ? queue.position : pending.from;
      return search(s, from, pending.direction);
    }
    // Counting or fetching: the matches found are out of date.
    if (s.focused === null) return { search: s };
    return { search: { ...s, stale: true } };
  }
  if (pending === null) return { search: s };
  if (r.type === "searchfailed") {
    const search =
      s.focused === null
        ? { ...IDLE, id: s.id }
        : { ...s, pending: null, stale: true, old: false };
    return { search, toast: r.error };
  }
  if (r.positions === null) {
    return { search: { ...s, pending: null }, toast: UNSERVED };
  }
  if (pending.kind === "fetch") return fetched(s, pending, r, r.positions);
  if (r.start === 0 && r.positions.length === 0)
    return { search: { ...IDLE, id: s.id }, toast: NO_MATCHES };
  const chunks = new Map(s.old ? [] : s.chunks).set(r.start, r.positions);
  const next = {
    ...s,
    chunks,
    version: r.version,
    // A rerun's chunks replace the old ones, and their total.
    total: (s.old ? null : s.total) ?? totalOf(chunks),
    old: false,
  };
  if (r.first === null) return { search: next };
  const done = { ...next, pending: null, stale: r.version !== queue.version };
  if (pending.direction >= 0) return go(done, r.first, 1);
  if (r.first > 0) return go(done, r.first - 1, -1);
  // ▲ from the first match.
  return done.total === null ? go(done, 0, 1) : go(done, done.total - 1, -1);
}

/** Each snapshot's queue version: another one makes the matches stale. */
export function queued(s: Search, version: number | null): Search {
  if (s.stale || s.version === null || version === s.version) return s;
  return { ...s, stale: true };
}

/** Leaving search mode; the id goes on, so late replies are dropped. */
export const leave = (s: Search): Step => ({
  search: { ...IDLE, id: s.id },
  send: { type: "searchend" },
});

function go(s: Search, target: number, direction: Direction): Step {
  const at = position(s, target);
  if (at !== undefined) {
    return { search: { ...s, focused: target, at, pending: null }, reveal: at };
  }
  return {
    search: { ...s, pending: { kind: "fetch", target, direction } },
    send: { type: "matches", id: s.id, query: s.query, start: chunkOf(target) },
  };
}

/** ▼ continues after the focused match, ▲ before it. */
function rerun(s: Search, direction: Direction): Step {
  const at = s.at!;
  return search(s, direction > 0 ? at : at > 0 ? at - 1 : null, direction);
}

function search(
  s: Search,
  from: number | null,
  direction: Direction | 0,
): Step {
  const id = s.id + 1;
  return {
    search: {
      ...s,
      id,
      pending: { kind: "search", from, direction },
      old: s.chunks.size > 0,
    },
    send: { type: "search", id, query: s.query, from },
  };
}

function fetched(
  s: Search,
  pending: Extract<Pending, { kind: "fetch" }>,
  r: Extract<Reply, { type: "matches" }>,
  positions: number[],
): Step {
  // `version` `null` with `[]`: MPD rejected the range, the queue changed.
  if (r.version !== s.version) return rerun(s, pending.direction);
  const chunks = new Map(s.chunks).set(r.start, positions);
  const total = s.total ?? totalOf(chunks);
  const next = { ...s, chunks, total, pending: null };
  if (position(next, pending.target) !== undefined)
    return go(next, pending.target, pending.direction);
  // ▼ past the last match: the empty chunk set the total.
  if (pending.direction > 0) return go(next, 0, 1);
  return rerun(next, pending.direction);
}
