import { useEffect, useState } from "preact/hooks";
import { connect, type Status } from "./ws";

export function App() {
  const [status, setStatus] = useState<Status>({ state: "connecting" });
  useEffect(() => connect(setStatus), []);

  return (
    <main class="app">
      <h1>lyrica</h1>
      <p class="status">
        {status.state === "open"
          ? `connected: ${status.greeting}`
          : status.state}
      </p>
    </main>
  );
}
