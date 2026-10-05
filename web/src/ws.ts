import type { Entry, QueueStore } from "./queue-store";
import type { Reply as SearchReply, Request as SearchRequest } from "./search";

export type PlayState = "play" | "pause" | "stop" | "offline";

/** A `state` message, stamped with its local receive time (`performance.now()`). */
export type Snapshot = {
  state: PlayState;
  title: string | null;
  artist: string | null;
  album: string | null;
  /** URL of the song's folder on the music mirror, ending in `/`. */
  folder: string | null;
  elapsed: number | null;
  duration: number | null;
  /** 0–100; `null` when MPD can't tell. */
  volume: number | null;
  random: boolean;
  repeat: boolean;
  single: boolean;
  /** `null` while offline. */
  queue: Queue | null;
  at: number;
};

/** Queue state; entries travel in their own messages. */
export type Queue = {
  version: number;
  length: number;
  /** Current song's position and id; `null` when there is none. */
  position: number | null;
  id: number | null;
};

/** Sent first on every connection. */
export type Config = {
  /** URL of MPD's `httpd` output, `null` when not configured. */
  stream: string | null;
};

/**
 * `random`, `repeat` and `single` toggle the mode; `volume` sets MPD's;
 * `seek` jumps to `position` seconds in the current song; `playid` plays the
 * queue entry with that id; `albumstart` plays the current song's album group
 * in order from its first entry, random and single off.
 */
export type Command =
  | {
      type:
        | "previous"
        | "toggle"
        | "next"
        | "random"
        | "repeat"
        | "single"
        | "albumstart";
    }
  | { type: "volume"; volume: number }
  | { type: "seek"; position: number }
  | { type: "playid"; id: number };

export type Connection = {
  /** Dropped unless the socket is open. */
  send(command: Command): void;
  /** Stored playlist names, sorted; `null` when listing failed. */
  playlists(): Promise<string[] | null>;
  /** Replaces the queue with a stored playlist: `null`, or why it failed. */
  load(name: string): Promise<string | null>;
  /** Search requests; `false` unless the socket is open. */
  search(request: SearchRequest): boolean;
  close(): void;
};

export const NOT_CONNECTED = "Not connected";
// The server keeps going: only the reply is lost.
export const CONNECTION_LOST = "Connection lost; the load may still finish";

/** Replies awaited from the server; `lost` settles them all. */
export function requests() {
  let listings: ((names: string[] | null) => void)[] = [];
  let loads: { name: string; resolve: (error: string | null) => void }[] = [];
  const listed = (names: string[] | null) => {
    const waiting = listings;
    listings = [];
    for (const resolve of waiting) resolve(names);
  };
  return {
    listing: () =>
      new Promise<string[] | null>((resolve) => listings.push(resolve)),
    load: (name: string) =>
      new Promise<string | null>((resolve) => loads.push({ name, resolve })),
    /** Any listing answers them all: they ask the same question. */
    listed,
    loaded(name: string, error: string | null) {
      const i = loads.findIndex((l) => l.name === name);
      if (i >= 0) loads.splice(i, 1)[0]!.resolve(error);
    },
    lost() {
      listed(null);
      const waiting = loads;
      loads = [];
      for (const { resolve } of waiting) resolve(CONNECTION_LOST);
    },
  };
}

const RETRY_MS = 1000;

/**
 * Keeps a socket to the server open, reconnecting on drops. Queue state and
 * entries go to `store`, outside `App`'s state; search replies to `onSearch`,
 * with `lost` on every close.
 */
export function connect(
  onSnapshot: (snapshot: Snapshot) => void,
  onConfig: (config: Config) => void,
  store: QueueStore,
  onSearch: (reply: SearchReply) => void,
): Connection {
  // Relative so it works under a reverse-proxy path prefix.
  const url = new URL("ws", location.href);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";

  let socket: WebSocket;
  let retry: number | undefined;
  const pending = requests();

  const open = () => {
    socket = new WebSocket(url);
    socket.onopen = () =>
      store.opened((window) => {
        if (socket.readyState === WebSocket.OPEN)
          socket.send(JSON.stringify(window));
      });
    socket.onmessage = (event) => {
      const message = parse(event.data, performance.now());
      if (message === undefined) return;
      if ("stream" in message) onConfig(message);
      else if ("songs" in message)
        store.reply(message.version, message.start, message.songs);
      else if ("names" in message) pending.listed(message.names);
      else if ("type" in message) onSearch(message);
      else if ("loaded" in message)
        pending.loaded(message.loaded, message.error);
      else {
        store.state(message.queue);
        onSnapshot(message);
      }
    };
    socket.onclose = () => {
      pending.lost();
      onSearch({ type: "lost" });
      store.state(null);
      onSnapshot({
        state: "offline",
        title: null,
        artist: null,
        album: null,
        folder: null,
        elapsed: null,
        duration: null,
        volume: null,
        random: false,
        repeat: false,
        single: false,
        queue: null,
        at: performance.now(),
      });
      retry = window.setTimeout(open, RETRY_MS);
    };
  };

  // Also from `onVisibility`, which detaches `onclose` first.
  const shutdown = () => {
    window.clearTimeout(retry);
    socket.onopen = socket.onmessage = socket.onclose = null;
    socket.close();
    pending.lost();
    onSearch({ type: "lost" });
  };

  // iOS suspends hidden pages, leaving the socket dead or silently stale and the
  // local clock unreliable; a new socket gets a fresh snapshot with current `elapsed`.
  const onVisibility = () => {
    if (document.visibilityState !== "visible") return;
    shutdown();
    open();
  };

  open();
  document.addEventListener("visibilitychange", onVisibility);

  const isOpen = () => socket.readyState === WebSocket.OPEN;
  return {
    send(command) {
      if (isOpen()) socket.send(JSON.stringify(command));
    },
    playlists() {
      if (!isOpen()) return Promise.resolve(null);
      const names = pending.listing();
      socket.send(JSON.stringify({ type: "playlists" }));
      return names;
    },
    load(name) {
      if (!isOpen()) return Promise.resolve(NOT_CONNECTED);
      const error = pending.load(name);
      socket.send(JSON.stringify({ type: "load", name }));
      return error;
    },
    search(request) {
      if (!isOpen()) return false;
      socket.send(JSON.stringify(request));
      return true;
    },
    close() {
      document.removeEventListener("visibilitychange", onVisibility);
      shutdown();
    },
  };
}

type Reply = { version: number; start: number; songs: Entry[] | null };
type Listing = { names: string[] | null };
type Loaded = { loaded: string; error: string | null };

function parse(
  data: unknown,
  at: number,
): Snapshot | Config | Reply | Listing | Loaded | SearchReply | undefined {
  if (typeof data !== "string") return;
  let message;
  try {
    message = JSON.parse(data);
  } catch {
    return;
  }
  if (message?.type === "config") return { stream: message.stream };
  if (message?.type === "queue") {
    const { version, start, songs } = message;
    return { version, start, songs };
  }
  if (message?.type === "playlists") return { names: message.names };
  if (message?.type === "loaded")
    return { loaded: message.name, error: message.error };
  if (message?.type === "matches") {
    const { id, version, start, positions, first } = message;
    return { type: "matches", id, version, start, positions, first };
  }
  if (message?.type === "searchfailed")
    return { type: "searchfailed", id: message.id, error: message.error };
  if (message?.type === "searchstale")
    return { type: "searchstale", id: message.id };
  if (message?.type !== "state") return;
  const { state, title, artist, album, folder, elapsed, duration, volume } =
    message;
  const { random, repeat, single, queue } = message;
  return {
    state,
    title,
    artist,
    album,
    folder,
    elapsed,
    duration,
    volume,
    random,
    repeat,
    single,
    queue,
    at,
  };
}
