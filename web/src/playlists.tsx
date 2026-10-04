import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { Icon } from "./icon";
import { Marquee } from "./marquee";

const TOAST_MS = 3000;

type Step =
  | { kind: "idle" }
  | { kind: "listing" }
  | { kind: "choosing"; names: string[]; selected: string | null }
  | { kind: "loading"; name: string }
  | { kind: "failed"; name: string; error: string };

const IDLE: Step = { kind: "idle" };

/**
 * The toolbar's stored playlists button and its dialogs (architecture.md →
 * "Stored playlists"). Each tap lists the names afresh.
 */
export function Playlists({
  disabled,
  list,
  load,
  onLoaded,
}: {
  disabled: boolean;
  list: () => Promise<string[] | null>;
  /** `null` on success, or why it failed. */
  load: (name: string) => Promise<string | null>;
  onLoaded: () => void;
}) {
  const [step, setStep] = useState<Step>(IDLE);
  // Taps can come faster than renders.
  const busy = useRef(false);
  const [toast, setToast] = useState<{ text: string } | null>(null);
  const choose = useRef<HTMLDialogElement>(null);
  const loading = useRef<HTMLDialogElement>(null);
  const failed = useRef<HTMLDialogElement>(null);

  useLayoutEffect(() => {
    const dialogs = [
      ["choosing", choose],
      ["loading", loading],
      ["failed", failed],
    ] as const;
    for (const [kind, ref] of dialogs) {
      const dialog = ref.current!;
      if (step.kind === kind) {
        if (!dialog.open) dialog.showModal();
      } else if (dialog.open) dialog.close();
    }
    busy.current = step.kind !== "idle";
  }, [step.kind]);

  useEffect(() => {
    if (toast === null) return;
    const timer = setTimeout(() => setToast(null), TOAST_MS);
    return () => clearTimeout(timer);
  }, [toast]);

  const open = async () => {
    if (busy.current) return;
    busy.current = true;
    setToast(null);
    setStep({ kind: "listing" });
    const names = await list();
    if (names !== null && names.length > 0) {
      setStep({ kind: "choosing", names, selected: null });
      return;
    }
    setStep(IDLE);
    setToast({
      text: names === null ? "Couldn't list playlists" : "No playlists found",
    });
  };

  const start = async (name: string) => {
    setStep({ kind: "loading", name });
    const error = await load(name);
    if (error === null) {
      setStep(IDLE);
      onLoaded();
    } else setStep({ kind: "failed", name, error });
  };

  // Programmatic closes fire `close` too, after the step moved on.
  const closed = (kind: Step["kind"]) => () =>
    setStep((s) => (s.kind === kind ? IDLE : s));

  return (
    <>
      <button
        class="playlists"
        aria-label="Load playlist"
        disabled={disabled}
        onClick={open}
      >
        <Icon path={PLAYLISTS} />
      </button>
      <p class="toast" role="status">
        {toast?.text}
      </p>
      <dialog
        class="dialog"
        ref={choose}
        aria-labelledby="playlists-title"
        onClose={closed("choosing")}
        // The content fills the box: only the backdrop targets the dialog.
        onClick={(e) => {
          if (e.target === e.currentTarget) e.currentTarget.close();
        }}
      >
        {step.kind === "choosing" && (
          <div class="dialog-box">
            <header>
              <h2 id="playlists-title">Playlists</h2>
              <button
                class="close"
                aria-label="Close"
                onClick={() => choose.current!.close()}
              >
                <Icon path={CLOSE} />
              </button>
            </header>
            <div class="names" role="listbox" aria-labelledby="playlists-title">
              {step.names.map((name) => (
                <button
                  key={name}
                  role="option"
                  aria-selected={name === step.selected}
                  onClick={() => setStep({ ...step, selected: name })}
                >
                  <Marquee text={name} />
                </button>
              ))}
            </div>
            <footer>
              <button
                class="primary"
                disabled={step.selected === null}
                onClick={() => step.selected !== null && start(step.selected)}
              >
                Load
              </button>
            </footer>
          </div>
        )}
      </dialog>
      <dialog
        class="dialog small loading"
        ref={loading}
        // Chrome lets a repeated Escape close it despite `cancel` being
        // prevented, unless the key itself is.
        onKeyDown={(e) => e.key === "Escape" && e.preventDefault()}
        onCancel={(e) => e.preventDefault()}
      >
        <span class="spinner" aria-hidden="true" />
        <p>Loading…</p>
      </dialog>
      <dialog
        class="dialog small"
        ref={failed}
        aria-labelledby="load-failed-title"
        onClose={closed("failed")}
      >
        {step.kind === "failed" && (
          <div class="dialog-box">
            <h2 id="load-failed-title">
              <Marquee text={`Couldn't load ${step.name}`} />
            </h2>
            <p>{step.error}</p>
            <footer>
              <button class="primary" onClick={() => failed.current!.close()}>
                OK
              </button>
            </footer>
          </div>
        )}
      </dialog>
    </>
  );
}

// Material Icons (Apache 2.0): queue_music, close.
const PLAYLISTS =
  "M15 6H3v2h12V6zm0 4H3v2h12v-2zM3 16h8v-2H3v2zM17 6v8.18c-.31-.11-.65-.18-1-.18-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3V8h3V6h-5z";
const CLOSE =
  "M19 6.41 17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z";
