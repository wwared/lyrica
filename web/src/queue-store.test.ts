import { afterEach, describe, expect, test, vi } from "vitest";
import {
  queueStore,
  RETRY_MS,
  type Entry,
  type Listener,
  type Window,
} from "./queue-store";

const LONG = 175_872;

const songs = (start: number, end: number): Entry[] =>
  Array.from({ length: end - start }, (_, i) => ({
    id: start + i,
    title: `t${start + i}`,
    artist: null,
    album: null,
    track: null,
    duration: null,
    folder: null,
  }));

/** A shown store at version 1; `sent` collects requested block starts. */
function setup(length = LONG) {
  let visible = true;
  const store = queueStore(() => visible);
  const sent: Window[] = [];
  store.opened((window) => sent.push(window));
  store.state({ version: 1, length, position: null, id: null });
  store.show(true);
  const starts = () => sent.splice(0).map((w) => w.start);
  const answer = (w: Window, version = w.version) =>
    store.reply(version, w.start, songs(w.start, w.end));
  return { store, sent, starts, answer, hide: () => (visible = false) };
}

afterEach(() => {
  vi.useRealTimers();
});

const listen = (overrides: Partial<Listener>): Listener => ({
  stored() {},
  current() {},
  opening() {},
  ...overrides,
});

describe("planning", () => {
  test("visible blocks first, then nearest, ties below, two at a time", () => {
    const { store, sent, starts, answer } = setup();
    // One viewport (100 rows) beyond reaches blocks 10 and 13, 50 rows away each.
    store.view(1150, 1250);
    const [a, b] = sent.splice(0);
    expect([a!.start, b!.start]).toEqual([1100, 1200]);
    answer(a!);
    expect(starts()).toEqual([1300]);
    answer(b!);
    expect(starts()).toEqual([1000]);
    store.reply(1, 1300, songs(1300, 1400));
    store.reply(1, 1000, songs(1000, 1100));
    expect(starts()).toEqual([]);
  });

  test("nearer above goes first", () => {
    const { store, sent } = setup();
    store.view(1150, 1240);
    store.reply(1, 1100, []);
    store.reply(1, 1200, []);
    expect(sent.map((w) => w.start)).toEqual([1100, 1200, 1000, 1300]);
  });

  test("clamped to the queue, the last block shorter", () => {
    const { store, sent } = setup(1234);
    store.view(1200, 1234);
    store.reply(1, 1200, []);
    store.reply(1, 1100, []);
    expect(sent).toEqual([
      { type: "queue", version: 1, start: 1200, end: 1234 },
      { type: "queue", version: 1, start: 1100, end: 1200 },
    ]);
    const top = setup(1234);
    top.store.view(0, 10);
    top.store.reply(1, 0, []);
    expect(top.sent.map((w) => w.start)).toEqual([0]);
  });

  test("a freed slot goes to what is nearest now, not to a backlog", () => {
    const { store, starts } = setup();
    store.view(0, 10);
    expect(starts()).toEqual([0]);
    store.view(50_050, 50_060);
    expect(starts()).toEqual([50_000]);
    store.view(90_050, 90_060);
    expect(starts()).toEqual([]);
    store.reply(1, 0, songs(0, 100));
    expect(starts()).toEqual([90_000]);
    store.reply(1, 50_000, songs(50_000, 50_100));
    expect(starts()).toEqual([]);
    expect(store.entry(0)).toBeUndefined();
  });

  test("nothing while not shown or hidden", () => {
    const { store, starts, hide } = setup();
    store.show(false);
    store.view(0, 10);
    expect(starts()).toEqual([]);
    store.show(true);
    expect(starts()).toEqual([0]);
    hide();
    store.view(500, 510);
    expect(starts()).toEqual([]);
  });
});

describe("replies", () => {
  test("stored and announced for the current version", () => {
    const { store, sent, answer } = setup();
    const announced: [number, number][] = [];
    store.subscribe(
      listen({ stored: (start, end) => announced.push([start, end]) }),
    );
    store.view(1150, 1160);
    answer(sent[0]!);
    expect(store.entry(1100)?.title).toBe("t1100");
    expect(store.entry(1199)?.title).toBe("t1199");
    expect(store.entry(1200)).toBeUndefined();
    expect(announced).toEqual([[1100, 1200]]);
  });

  test("an empty one is not retried", () => {
    const { store, starts } = setup();
    store.view(1150, 1160);
    expect(starts()).toEqual([1100]);
    store.reply(1, 1100, []);
    store.view(1151, 1161);
    store.show(false);
    store.show(true);
    expect(starts()).toEqual([]);
    expect(store.entry(1150)).toBeUndefined();
  });

  test("a null one is retried after a second", () => {
    vi.useFakeTimers();
    const { store, starts } = setup();
    store.view(1150, 1160);
    expect(starts()).toEqual([1100]);
    store.reply(1, 1100, null);
    expect(starts()).toEqual([]);
    vi.advanceTimersByTime(RETRY_MS - 1);
    expect(starts()).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(starts()).toEqual([1100]);
  });

  test("after a new version, old ones are dropped and refetches wait for their slots", () => {
    const { store, sent, answer } = setup();
    store.view(1150, 1250);
    const [a, b] = sent.splice(0);
    answer(a!);
    const [c] = sent.splice(0);
    store.state({ version: 2, length: LONG, position: null, id: null });
    // Both slots still wait for version 1 replies, which are dropped.
    expect(sent).toEqual([]);
    answer(b!);
    expect(store.entry(1200)).toBeUndefined();
    expect(sent).toEqual([
      { type: "queue", version: 2, start: 1100, end: 1200 },
    ]);
    answer(c!);
    expect(sent.map((w) => [w.version, w.start])).toEqual([
      [2, 1100],
      [2, 1200],
    ]);
  });

  test("dropped once the view is far, refetched when back", () => {
    const { store, sent, starts, answer } = setup();
    store.view(1150, 1160);
    store.reply(1, 1100, songs(1100, 1200));
    starts();
    // Kept while within 1,000 rows of the visible ones.
    store.view(2199, 2209);
    expect(store.entry(1100)).toBeDefined();
    store.view(2200, 2210);
    expect(store.entry(1100)).toBeUndefined();
    for (const w of sent.splice(0)) answer(w);
    store.view(1150, 1160);
    expect(starts()).toEqual([1100]);
  });

  test("one arriving after the view moved far away is dropped", () => {
    const { store, starts } = setup();
    store.view(1150, 1160);
    expect(starts()).toEqual([1100]);
    store.view(90_000, 90_010);
    store.reply(1, 1100, songs(1100, 1200));
    expect(store.entry(1100)).toBeUndefined();
  });
});

describe("new version", () => {
  /** Blocks 10 to 13 loaded at version 1, rows 1150 to 1249 visible. */
  function loaded(length = LONG) {
    const t = setup(length);
    t.store.view(1150, 1250);
    for (let i = 0; i < 4; i++) t.answer(t.sent.shift()!);
    const announced: [number, number][] = [];
    t.store.subscribe(
      listen({ stored: (start, end) => announced.push([start, end]) }),
    );
    const next = (version: number, length = LONG) =>
      t.store.state({ version, length, position: null, id: null });
    return { ...t, announced, next };
  }

  test("visible old entries stay until replaced in place; the rest go", () => {
    const { store, sent, starts, announced, next } = loaded();
    next(2);
    expect(store.entry(1100)?.title).toBe("t1100");
    expect(store.entry(1299)?.title).toBe("t1299");
    expect(store.entry(1099)).toBeUndefined();
    expect(store.entry(1300)).toBeUndefined();
    expect(announced).toEqual([[0, Infinity]]);
    const [a, b] = sent.splice(0);
    expect([a!.version, a!.start, b!.start]).toEqual([2, 1100, 1200]);
    store.reply(2, 1100, songs(0, 100));
    expect(store.entry(1100)?.title).toBe("t0");
    expect(store.entry(1200)?.title).toBe("t1200");
    expect(announced).toEqual([
      [0, Infinity],
      [1100, 1200],
    ]);
    expect(starts()).toEqual([1300]);
  });

  test("an empty refetch blanks the old entries, a null one keeps them", () => {
    vi.useFakeTimers();
    const { store, starts, announced, next } = loaded();
    store.view(1150, 1160);
    next(2);
    expect(starts()).toEqual([1100]);
    store.reply(2, 1100, null);
    expect(store.entry(1150)?.title).toBe("t1150");
    vi.advanceTimersByTime(RETRY_MS);
    expect(starts()).toEqual([1100]);
    store.reply(2, 1100, []);
    expect(store.entry(1150)).toBeUndefined();
    expect(announced.at(-1)).toEqual([1100, 1200]);
  });

  test("several in a row keep the old entries; replies for older ones are dropped", () => {
    const { store, sent, next } = loaded();
    next(2);
    next(3);
    expect(store.entry(1150)?.title).toBe("t1150");
    store.reply(2, 1100, songs(0, 100));
    expect(store.entry(1150)?.title).toBe("t1150");
    expect(sent.map((w) => [w.version, w.start])).toEqual([
      [2, 1100],
      [2, 1200],
      [3, 1100],
    ]);
    store.reply(3, 1100, songs(0, 100));
    expect(store.entry(1150)?.title).toBe("t50");
  });

  test("entries past a shorter queue are gone at once", () => {
    const { store, sent, next } = loaded();
    next(2, 1210);
    expect(store.entry(1209)?.title).toBe("t1209");
    expect(store.entry(1210)).toBeUndefined();
    expect(sent[1]).toEqual({
      type: "queue",
      version: 2,
      start: 1200,
      end: 1210,
    });
  });

  test("old entries are dropped once the view is far", () => {
    const { store, next } = loaded();
    store.show(false);
    next(2);
    expect(store.entry(1150)?.title).toBe("t1150");
    store.view(2300, 2310);
    expect(store.entry(1150)).toBeUndefined();
  });
});

describe("current song", () => {
  test("announced when its position or id changes, not on other state", () => {
    const { store } = setup();
    let changes = 0;
    store.subscribe(listen({ current: () => changes++ }));
    store.state({ version: 1, length: LONG, position: 7, id: 70 });
    expect(changes).toBe(1);
    store.state({ version: 2, length: LONG + 1, position: 7, id: 70 });
    expect(changes).toBe(1);
    store.state({ version: 3, length: LONG, position: 8, id: 70 });
    expect(store.current).toEqual({ position: 8, id: 70 });
    store.state({ version: 3, length: LONG, position: 8, id: 71 });
    expect(changes).toBe(3);
    store.state(null);
    expect(changes).toBe(4);
    expect(store.current).toEqual({ position: null, id: null });
  });

  test("opening reports the view that decides the first request", () => {
    const { store, starts } = setup();
    store.view(0, 10);
    expect(starts()).toEqual([0]);
    store.reply(1, 0, songs(0, 100));
    store.show(false);
    store.subscribe(listen({ opening: () => store.view(90_050, 90_060) }));
    store.show(true);
    expect(starts()).toEqual([90_000]);
  });
});

describe("connection", () => {
  test("offline drops every block; reconnecting refetches", () => {
    const { store, starts } = setup();
    store.view(1150, 1160);
    store.reply(1, 1100, songs(1100, 1200));
    starts();
    store.state(null);
    expect(store.entry(1100)).toBeUndefined();
    const sent: Window[] = [];
    store.opened((window) => sent.push(window));
    store.state({ version: 1, length: LONG, position: null, id: null });
    expect(sent.map((w) => w.start)).toEqual([1100]);
  });

  test("a reopened socket frees the slots and keeps the blocks", () => {
    const { store, starts } = setup();
    store.view(1150, 1250);
    store.reply(1, 1100, songs(1100, 1200));
    expect(starts()).toEqual([1100, 1200, 1300]);
    const sent: Window[] = [];
    store.opened((window) => sent.push(window));
    store.state({ version: 1, length: LONG, position: null, id: null });
    expect(store.entry(1100)?.title).toBe("t1100");
    expect(sent.map((w) => w.start)).toEqual([1200, 1300]);
  });
});
