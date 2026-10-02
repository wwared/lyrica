export type PlayState = "play" | "pause" | "stop" | "offline";

/** A `state` message, stamped with its local receive time (`performance.now()`). */
export type Snapshot = {
  state: PlayState;
  title: string | null;
  artist: string | null;
  /** URL of the song's folder on the music mirror, ending in `/`. */
  folder: string | null;
  elapsed: number | null;
  duration: number | null;
  at: number;
};

export type Command = "previous" | "toggle" | "next";

export type Connection = {
  /** Dropped unless the socket is open. */
  send(command: Command): void;
  close(): void;
};

const RETRY_MS = 1000;

/** Keeps a socket to the server open, reconnecting on drops. */
export function connect(onSnapshot: (snapshot: Snapshot) => void): Connection {
  // Relative so it works under a reverse-proxy path prefix.
  const url = new URL("ws", location.href);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";

  let socket: WebSocket;
  let retry: number | undefined;

  const open = () => {
    socket = new WebSocket(url);
    socket.onmessage = (event) => {
      const snapshot = parse(event.data, performance.now());
      if (snapshot) onSnapshot(snapshot);
    };
    socket.onclose = () => {
      onSnapshot({
        state: "offline",
        title: null,
        artist: null,
        folder: null,
        elapsed: null,
        duration: null,
        at: performance.now(),
      });
      retry = window.setTimeout(open, RETRY_MS);
    };
  };

  const shutdown = () => {
    window.clearTimeout(retry);
    socket.onmessage = socket.onclose = null;
    socket.close();
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

  return {
    send(command) {
      if (socket.readyState === WebSocket.OPEN)
        socket.send(JSON.stringify({ type: command }));
    },
    close() {
      document.removeEventListener("visibilitychange", onVisibility);
      shutdown();
    },
  };
}

function parse(data: unknown, at: number): Snapshot | undefined {
  if (typeof data !== "string") return;
  let message;
  try {
    message = JSON.parse(data);
  } catch {
    return;
  }
  if (message?.type !== "state") return;
  const { state, title, artist, folder, elapsed, duration } = message;
  return { state, title, artist, folder, elapsed, duration, at };
}
