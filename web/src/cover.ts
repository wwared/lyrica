// In priority order; the mirror is case-sensitive.
const NAMES = ["jpg", "JPG", "png", "PNG", "jpeg", "JPEG"].map(
  (ext) => `cover.${ext}`,
);

const known = new Map<string, string | null>();
/**
 * Probe results by folder URL: the cover URL, or `null` when there is none.
 * Misses are kept too, so revisiting a folder never shows the blank loading
 * state again; a cover added to the mirror appears after a reload.
 */
export const covers: ReadonlyMap<string, string | null> = known;

const probes = new Map<string, Promise<string | null>>();

/**
 * Probes all candidate names at once (one round trip instead of up to six)
 * and resolves to the first in priority order that loads and decodes, or
 * `null`. Decoding first means the visible `<img>` paints it immediately.
 * Each folder is probed once: later calls get the same promise, so the queue
 * and now playing share a probe still in flight.
 */
export function findCover(folder: string): Promise<string | null> {
  let found = probes.get(folder);
  if (found === undefined) {
    found = probe(folder);
    probes.set(folder, found);
  }
  return found;
}

async function probe(folder: string): Promise<string | null> {
  const candidates = NAMES.map((name) => {
    const url = folder + name;
    const image = new Image();
    image.src = url;
    return image.decode().then(
      () => url,
      () => null,
    );
  });
  for (const candidate of candidates) {
    const url = await candidate;
    if (url !== null) {
      known.set(folder, url);
      return url;
    }
  }
  known.set(folder, null);
  return null;
}
