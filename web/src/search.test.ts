import { describe, expect, test } from "vitest";
import {
  canGoBack,
  CHUNK,
  count,
  IDLE,
  LOST,
  leave,
  NO_MATCHES,
  queued,
  reply,
  type Reply,
  type Search,
  type Step,
  step,
  submit,
  UNSERVED,
  view,
} from "./search";

const QUEUE = { position: 1000, version: 7 };

/** Matches at every 10th position from `10 × start`. */
const chunk = (start: number, n = CHUNK) =>
  Array.from({ length: n }, (_, i) => (start + i) * 10);

const matches = (
  s: Search,
  start: number,
  positions: number[] | null,
  first: number | null = null,
  version: number | null = 7,
): Reply => ({ type: "matches", id: s.id, version, start, positions, first });

/** Feeds replies in order, carrying the state along. */
function feed(s: Search, ...replies: ((s: Search) => Reply)[]): Step {
  let last: Step = { search: s };
  for (const r of replies) last = reply(last.search, r(last.search), QUEUE);
  return last;
}

/** A finished search holding `chunks` (start → count), focused on `first`. */
function found(chunks: [number, number][], first: number): Search {
  const s = submit(IDLE, "love", QUEUE).search;
  const replies = chunks.map(
    ([start, n], i) =>
      (s: Search) =>
        matches(
          s,
          start,
          chunk(start, n),
          i === chunks.length - 1 ? first : null,
        ),
  );
  return feed(s, ...replies).search;
}

describe("submitting", () => {
  test("searches from the current song; blank does nothing", () => {
    const { search, send } = submit(IDLE, "love", QUEUE);
    expect(send).toEqual({ type: "search", id: 1, query: "love", from: 1000 });
    expect(search.pending).not.toBeNull();
    expect(submit(search, "  ", QUEUE)).toEqual({ search });
  });

  test("replaces the matches, even for the same query", () => {
    const s = found([[0, 3]], 1);
    const { search } = submit(s, "love", QUEUE);
    expect(search.chunks.size).toBe(0);
    expect(search.focused).toBeNull();
    expect(search.id).toBe(s.id + 1);
  });

  test("the reply with `first` focuses and reveals it", () => {
    const s = submit(IDLE, "love", QUEUE).search;
    const step = feed(
      s,
      (s) => matches(s, 0, chunk(0)),
      (s) => matches(s, 1000, chunk(1000)),
      (s) => matches(s, 500, chunk(500), 620),
    );
    expect(step.search.focused).toBe(620);
    expect(step.reveal).toBe(6200);
    expect(step.search.pending).toBeNull();
    expect(step.search.chunks.size).toBe(3);
    expect(count(step.search)).toBe("621 / …");
  });

  test("an empty chunk 0 means no matches", () => {
    const s = submit(IDLE, "zzqx", QUEUE).search;
    const step = feed(s, (s) => matches(s, 0, []));
    expect(step.toast).toBe(NO_MATCHES);
    expect(step.search.focused).toBeNull();
    expect(step.search.pending).toBeNull();
  });

  test("a later empty chunk is a probe, not an end", () => {
    const s = submit(IDLE, "love", QUEUE).search;
    const step = feed(
      s,
      (s) => matches(s, 0, chunk(0)),
      (s) => matches(s, 2000, []),
    );
    expect(step.toast).toBeUndefined();
    expect(step.search.pending).not.toBeNull();
  });

  test("a failure is a toast; search mode stays", () => {
    const s = submit(IDLE, "love", QUEUE).search;
    const step = reply(
      s,
      { type: "searchfailed", id: s.id, error: "Busy" },
      QUEUE,
    );
    expect(step.toast).toBe("Busy");
    expect(step.search.pending).toBeNull();
    expect(step.search.focused).toBeNull();
  });

  test("replies to another search are dropped", () => {
    const old = submit(IDLE, "a", QUEUE).search;
    const s = submit(old, "b", QUEUE).search;
    expect(reply(s, matches(old, 0, [], null), QUEUE)).toEqual({ search: s });
    const failed = { type: "searchfailed", id: old.id, error: "x" } as const;
    expect(reply(s, failed, QUEUE)).toEqual({ search: s });
  });

  test("going stale while pending reruns from the latest current song", () => {
    const s = submit(IDLE, "love", QUEUE).search;
    const step = reply(
      s,
      { type: "searchstale", id: s.id },
      { position: 40, version: 8 },
    );
    expect(step.send).toEqual({
      type: "search",
      id: s.id + 1,
      query: "love",
      from: 40,
    });
  });
});

describe("total", () => {
  test("a short chunk ends the matches", () => {
    expect(found([[0, 3]], 0).total).toBe(3);
    expect(
      found(
        [
          [0, CHUNK],
          [500, 20],
        ],
        510,
      ).total,
    ).toBe(520);
  });

  test("an empty chunk ends them only right after a full one", () => {
    expect(
      found(
        [
          [0, CHUNK],
          [1000, 0],
          [500, CHUNK],
        ],
        600,
      ).total,
    ).toBe(1000);
    expect(
      found(
        [
          [0, CHUNK],
          [1500, 0],
          [500, CHUNK],
        ],
        600,
      ).total,
    ).toBeNull();
  });

  const total = (s: Search, n: number, version = 7): Reply => ({
    type: "total",
    id: s.id,
    version,
    total: n,
  });

  test("the server's count sets it: ▲ on the first match then wraps", () => {
    const s = found([[0, CHUNK]], 0);
    expect(canGoBack(s)).toBe(false);
    const counted = reply(s, total(s, 3156), QUEUE).search;
    expect(count(counted)).toBe("1 / 3156");
    expect(step(counted, -1).send).toEqual({
      type: "matches",
      id: s.id,
      query: "love",
      start: 3000,
    });
  });

  test("a count for another version or search is dropped", () => {
    const s = found([[0, CHUNK]], 0);
    expect(reply(s, total(s, 3156, 8), QUEUE)).toEqual({ search: s });
    const other = { ...total(s, 3156), id: s.id - 1 };
    expect(reply(s, other, QUEUE)).toEqual({ search: s });
  });

  test("a count arriving during a fetch leaves the fetch going, and stays", () => {
    const s = found([[0, CHUNK]], 499);
    const fetching = step(s, 1).search;
    const counted = reply(fetching, total(fetching, 3156), QUEUE).search;
    expect(counted.pending).not.toBeNull();
    const next = reply(counted, matches(counted, 500, chunk(500)), QUEUE);
    expect(next.search.focused).toBe(500);
    expect(next.search.total).toBe(3156);
  });
});

describe("stepping", () => {
  test("within held chunks, across a chunk edge", () => {
    const s = found(
      [
        [0, CHUNK],
        [500, CHUNK],
      ],
      499,
    );
    const next = step(s, 1);
    expect(next.search.focused).toBe(500);
    expect(next.reveal).toBe(5000);
    expect(step(next.search, -1).search.focused).toBe(499);
  });

  test("wraps at the ends once the total is known", () => {
    const s = found([[0, 3]], 2);
    expect(step(s, 1).search.focused).toBe(0);
    const first = step(s, 1).search;
    expect(step(first, -1).search.focused).toBe(2);
    expect(count(first)).toBe("1 / 3");
  });

  test("▲ on the first match does nothing until the total is known", () => {
    const s = found([[0, CHUNK]], 0);
    expect(canGoBack(s)).toBe(false);
    expect(step(s, -1)).toEqual({ search: s });
  });

  test("does nothing while pending", () => {
    const s = found([[0, CHUNK]], 499);
    const fetching = step(s, 1).search;
    expect(step(fetching, 1)).toEqual({ search: fetching });
  });
});

describe("fetching", () => {
  test("▼ into an unheld chunk fetches it, then focuses there", () => {
    const s = found([[0, CHUNK]], 499);
    const { search, send } = step(s, 1);
    expect(send).toEqual({
      type: "matches",
      id: s.id,
      query: "love",
      start: 500,
    });
    const step2 = reply(search, matches(search, 500, chunk(500)), QUEUE);
    expect(step2.search.focused).toBe(500);
    expect(step2.reveal).toBe(5000);
  });

  test("▲ into an unheld chunk focuses its last match", () => {
    const s = found(
      [
        [0, CHUNK],
        [1000, CHUNK],
      ],
      1000,
    );
    const { search, send } = step(s, -1);
    expect(send).toMatchObject({ type: "matches", start: 500 });
    const next = reply(search, matches(search, 500, chunk(500)), QUEUE);
    expect(next.search.focused).toBe(999);
  });

  test("▼ getting [] is past the last match: the total, wrap to 0", () => {
    const s = found([[0, CHUNK]], 499);
    const { search } = step(s, 1);
    const next = reply(search, matches(search, 500, []), QUEUE);
    expect(next.search.total).toBe(500);
    expect(next.search.focused).toBe(0);
    expect(next.reveal).toBe(0);
  });

  test("[] rejected by MPD or another version reruns in the same direction", () => {
    const s = found(
      [
        [0, CHUNK],
        [1000, CHUNK],
      ],
      1000,
    );
    for (const r of [
      (s: Search) => matches(s, 500, [], null, null),
      (s: Search) => matches(s, 500, chunk(500), null, 8),
    ]) {
      const { search } = step(s, -1);
      const next = reply(search, r(search), QUEUE);
      expect(next.send).toEqual({
        type: "search",
        id: s.id + 1,
        query: "love",
        from: 9999,
      });
    }
  });

  test("an unserved fetch is a toast, focus unchanged", () => {
    const s = found([[0, CHUNK]], 499);
    const { search } = step(s, 1);
    const next = reply(search, matches(search, 500, null, null, null), QUEUE);
    expect(next.toast).toBe(UNSERVED);
    expect(next.search.focused).toBe(499);
    expect(next.search.pending).toBeNull();
  });

  test("going stale after the first match marks it; the next ▼ reruns", () => {
    const s = found([[0, CHUNK]], 120);
    const marked = reply(s, { type: "searchstale", id: s.id }, QUEUE).search;
    expect(marked.stale).toBe(true);
    expect(step(marked, 1).send).toMatchObject({ type: "search", from: 1200 });
  });
});

describe("stale", () => {
  test("another queue version makes the next ▼ rerun after the focused match", () => {
    const s = queued(found([[0, CHUNK]], 120), 8);
    const { search, send } = step(s, 1);
    expect(send).toEqual({
      type: "search",
      id: s.id + 1,
      query: "love",
      from: 1200,
    });
    // The old matches stay shown until the rerun's arrive.
    expect(search.chunks.size).toBe(1);
    expect(view(search).matches(1200)).toBe(true);
    const next = reply(search, matches(search, 0, [1210, 1300], 0, 8), {
      position: 1000,
      version: 8,
    });
    expect([...next.search.chunks.keys()]).toEqual([0]);
    expect(next.search.focused).toBe(0);
    expect(next.reveal).toBe(1210);
    expect(next.search.stale).toBe(false);
  });

  test("▲ reruns from just before the focused match and focuses the one before `first`", () => {
    const s = queued(found([[0, CHUNK]], 120), 8);
    const { search, send } = step(s, -1);
    expect(send).toMatchObject({ from: 1199 });
    const next = reply(search, matches(search, 0, chunk(0), 120, 8), {
      position: 1000,
      version: 8,
    });
    expect(next.search.focused).toBe(119);
  });

  test("▲ rerun with `first` 0 wraps only once the total is known", () => {
    const at0 = queued(found([[0, 3]], 0), 8);
    expect(step(at0, -1).send).toMatchObject({ from: null });
    const known = step(at0, -1).search;
    const wrapped = reply(known, matches(known, 0, [5, 9], 0, 8), {
      position: null,
      version: 8,
    });
    expect(wrapped.search.focused).toBe(1);
    const unknown = step(at0, -1).search;
    const stays = reply(unknown, matches(unknown, 0, chunk(0), 0, 8), {
      position: null,
      version: 8,
    });
    expect(stays.search.focused).toBe(0);
  });

  test("a push arriving before the last reply leaves the result stale", () => {
    const s = submit(IDLE, "love", QUEUE).search;
    const done = reply(s, matches(s, 0, chunk(0, 3), 0, 7), {
      position: 1000,
      version: 8,
    });
    expect(done.search.stale).toBe(true);
  });

  test("a lost connection makes the matches stale, failing what's pending", () => {
    const s = found([[0, CHUNK]], 499);
    expect(reply(s, { type: "lost" }, QUEUE).search.stale).toBe(true);
    const fetching = step(s, 1).search;
    const lost = reply(fetching, { type: "lost" }, QUEUE);
    expect(lost.toast).toBe(LOST);
    expect(lost.search.pending).toBeNull();
    expect(reply(IDLE, { type: "lost" }, QUEUE)).toEqual({ search: IDLE });
  });
});

test("rows: matches among the held chunks only", () => {
  const v = view(
    found(
      [
        [1000, CHUNK],
        [0, CHUNK],
      ],
      3,
    ),
  );
  expect(v.focused).toBe(30);
  // 9990 is a match of chunk 500, not held.
  expect([0, 10, 4990, 9990, 10_000, 15].map((p) => v.matches(p))).toEqual([
    true,
    true,
    true,
    false,
    true,
    false,
  ]);
});

test("leaving ends the search; its late replies are dropped", () => {
  const s = submit(IDLE, "love", QUEUE).search;
  const { search, send } = leave(s);
  expect(send).toEqual({ type: "searchend" });
  expect(reply(search, matches(s, 0, chunk(0), 0), QUEUE)).toEqual({ search });
});
