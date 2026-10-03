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

/**
 * Probes all candidate names at once (one round trip instead of up to six)
 * and resolves to the first in priority order that loads and decodes, or
 * `null`. Decoding first means the visible `<img>` paints it immediately.
 */
export async function findCover(folder: string): Promise<string | null> {
  const probes = NAMES.map((name) => {
    const url = folder + name;
    const image = new Image();
    image.src = url;
    return image.decode().then(
      () => url,
      () => null,
    );
  });
  for (const probe of probes) {
    const url = await probe;
    if (url !== null) {
      known.set(folder, url);
      return url;
    }
  }
  known.set(folder, null);
  return null;
}
