// In priority order; the mirror is case-sensitive.
const NAMES = ["jpg", "JPG", "png", "PNG", "jpeg", "JPEG"].map(
  (ext) => `cover.${ext}`,
);

const found = new Map<string, string>();
/** Found covers by folder URL; misses are not kept, so a later probe can still find one. */
export const covers: ReadonlyMap<string, string> = found;

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
      found.set(folder, url);
      return url;
    }
  }
  return null;
}
