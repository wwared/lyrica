import type { RefObject } from "preact";
import {
  useEffect,
  useLayoutEffect,
  useReducer,
  useRef,
  useState,
} from "preact/hooks";
import { CLOSE, Icon } from "./icon";
import type { QueueStore } from "./queue-store";
import {
  canGoBack,
  count,
  type Direction,
  IDLE,
  leave,
  type Queue,
  queued,
  type Reply,
  reply,
  type Search,
  type Step,
  step,
  submit,
  view,
} from "./search";
import { type Connection, NOT_CONNECTED } from "./ws";

export type SearchMode = {
  readonly searching: boolean;
  readonly search: Search;
  enter(): void;
  leave(): void;
  /** Whether it ran: a blank query does nothing. */
  submit(query: string): boolean;
  step(direction: Direction): void;
  /** Each snapshot's queue. */
  queue(queue: { position: number | null; version: number } | null): void;
  reply(reply: Reply): void;
};

/**
 * Search mode's state (`search.ts`) applied: requests sent, matches and
 * reveals passed to the list through `store`, toasts shown. One object for
 * the app's lifetime, so the socket's callbacks can hold it.
 */
export function useSearchMode(
  store: QueueStore,
  connection: RefObject<Connection | null>,
  toast: (text: string) => void,
): SearchMode {
  const [, render] = useReducer((n: number) => n + 1, 0);
  const [mode] = useState((): SearchMode => {
    let searching = false;
    let search = IDLE;
    let latest: Queue = { position: null, version: null };
    const update = (next: Search) => {
      if (next === search) return;
      const rows = next.chunks !== search.chunks || next.at !== search.at;
      search = next;
      if (rows) store.search(view(next));
      render(0);
    };
    const apply = (next: Step) => {
      if (!searching) return;
      update(next.search);
      const { send } = next;
      if (
        send !== undefined &&
        send.type !== "searchend" &&
        !connection.current?.search(send)
      ) {
        const failed = { type: "searchfailed", id: send.id } as const;
        apply(reply(search, { ...failed, error: NOT_CONNECTED }, latest));
        return;
      }
      if (next.toast !== undefined) toast(next.toast);
      if (next.reveal !== undefined) store.reveal(next.reveal);
    };
    return {
      get searching() {
        return searching;
      },
      get search() {
        return search;
      },
      enter() {
        if (searching) return;
        searching = true;
        store.search(view(search));
        render(0);
      },
      leave() {
        if (!searching) return;
        const left = leave(search);
        connection.current?.search(left.send!);
        searching = false;
        search = left.search;
        store.search(null);
        render(0);
      },
      submit(query) {
        const next = submit(search, query, latest);
        apply(next);
        return next.send !== undefined;
      },
      step(direction) {
        apply(step(search, direction));
      },
      queue(queue) {
        latest = {
          position: queue?.position ?? null,
          version: queue?.version ?? null,
        };
        if (searching) update(queued(search, latest.version));
      },
      reply(r) {
        if (searching) apply(reply(search, r, latest));
      },
    };
  });
  return mode;
}

/** The toolbar in search mode: `[✕][input ⌫][🔍]`. Escape leaves too. */
export function SearchBar({
  onClose,
  onSubmit,
}: {
  onClose: () => void;
  onSubmit: (query: string) => boolean;
}) {
  const input = useRef<HTMLInputElement>(null);
  const [filled, setFilled] = useState(false);
  useLayoutEffect(() => {
    // As a property (newer browsers), Preact would set the string "off" as
    // `true`.
    input.current!.setAttribute("autocorrect", "off");
    input.current!.focus();
  }, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // A search input would clear itself instead.
      e.preventDefault();
      onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);
  return (
    <form
      class="search-bar"
      role="search"
      onSubmit={(e) => {
        e.preventDefault();
        // Closes the iOS keyboard.
        if (onSubmit(input.current!.value)) input.current!.blur();
      }}
    >
      <button type="button" aria-label="Close search" onClick={onClose}>
        <Icon path={CLOSE} />
      </button>
      <span class="search-field">
        <input
          ref={input}
          type="search"
          aria-label="Search the queue"
          enterkeyhint="search"
          autocomplete="off"
          autocapitalize="off"
          spellcheck={false}
          onInput={(e) => setFilled(e.currentTarget.value !== "")}
        />
        {filled && (
          <button
            type="button"
            class="search-clear"
            aria-label="Clear"
            // Keeps the focus, so an open keyboard doesn't close and reopen.
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => {
              input.current!.value = "";
              setFilled(false);
              input.current!.focus();
            }}
          >
            <Icon path={CLEAR} />
          </button>
        )}
      </span>
      <button type="submit" aria-label="Search">
        <Icon path={SEARCH} />
      </button>
    </form>
  );
}

/** `( ▲ 3 / 120 ▼ )` over the list's bottom edge; the spinner while pending. */
export function SearchPill({
  search,
  onStep,
}: {
  search: Search;
  onStep: (direction: Direction) => void;
}) {
  if (search.pending === null && search.focused === null) return null;
  return (
    <div class="search-pill">
      <button
        aria-label="Previous match"
        disabled={!canGoBack(search)}
        onClick={() => onStep(-1)}
      >
        <Icon path={UP} />
      </button>
      <span class="search-count" role="status">
        {search.pending === null ? (
          count(search)
        ) : (
          <span class="spinner" aria-hidden="true" />
        )}
      </span>
      <button aria-label="Next match" onClick={() => onStep(1)}>
        <Icon path={DOWN} />
      </button>
    </div>
  );
}

// Material Icons (Apache 2.0): search, backspace, keyboard_arrow_up,
// keyboard_arrow_down.
export const SEARCH =
  "M15.5 14h-.79l-.28-.27C15.41 12.59 16 11.11 16 9.5 16 5.91 13.09 3 9.5 3S3 5.91 3 9.5 5.91 16 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z";
const CLEAR =
  "M22 3H7c-.69 0-1.23.35-1.59.88L0 12l5.41 8.11c.36.53.9.89 1.59.89h15c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm-3 12.59L17.59 17 14 13.41 10.41 17 9 15.59 12.59 12 9 8.41 10.41 7 14 10.59 17.59 7 19 8.41 15.41 12 19 15.59z";
const UP = "M7.41 15.41 12 10.83l4.59 4.58L18 14l-6-6-6 6z";
const DOWN = "M7.41 8.59 12 13.17l4.59-4.58L18 10l-6 6-6-6z";
