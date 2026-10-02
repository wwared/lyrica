export type Status =
  | { state: "connecting" }
  | { state: "open"; greeting: string }
  | { state: "closed" };

const RETRY_MS = 1000;

/** Keeps a socket to the server open, reconnecting on drops. Returns a disposer. */
export function connect(onStatus: (status: Status) => void): () => void {
  // Relative so it works under a reverse-proxy path prefix.
  const url = new URL("ws", location.href);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";

  let socket: WebSocket;
  let retry: number | undefined;
  let disposed = false;

  const open = () => {
    onStatus({ state: "connecting" });
    socket = new WebSocket(url);
    socket.onmessage = (event) =>
      onStatus({ state: "open", greeting: String(event.data) });
    socket.onclose = () => {
      if (disposed) return;
      onStatus({ state: "closed" });
      retry = window.setTimeout(open, RETRY_MS);
    };
  };
  open();

  return () => {
    disposed = true;
    window.clearTimeout(retry);
    socket.close();
  };
}
