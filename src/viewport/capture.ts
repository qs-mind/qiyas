/** A picture of the canvas for Mizan's `view_screen`: the pixels WebGL read back, the right way up,
 *  scaled to fit and made a PNG. */

/** `rgba` read from WebGL (rows from the bottom) with its rows from the top, every pixel opaque. */
export function flipRows(rgba: Uint8Array, width: number, height: number): Uint8ClampedArray<ArrayBuffer> {
  const row = width * 4;
  const out = new Uint8ClampedArray(new ArrayBuffer(rgba.length));
  for (let y = 0; y < height; y++) {
    out.set(rgba.subarray((height - 1 - y) * row, (height - y) * row), y * row);
  }
  for (let i = 3; i < out.length; i += 4) out[i] = 255;
  return out;
}

/** The size a `width` x `height` picture takes with its longer side at most `maxSide`. */
export function fitSize(width: number, height: number, maxSide: number): [number, number] {
  const scale = Math.min(1, maxSide / Math.max(width, height, 1));
  return [Math.max(1, Math.round(width * scale)), Math.max(1, Math.round(height * scale))];
}

/** The pixels read back as a PNG, base64, its longer side at most `maxSide`. */
export async function toPngBase64(
  shot: { width: number; height: number; rgba: Uint8Array },
  maxSide: number,
): Promise<string> {
  const { width, height } = shot;
  const full = document.createElement("canvas");
  [full.width, full.height] = [width, height];
  full.getContext("2d")!.putImageData(new ImageData(flipRows(shot.rgba, width, height), width, height), 0, 0);
  const [w, h] = fitSize(width, height, maxSide);
  const out = document.createElement("canvas");
  [out.width, out.height] = [w, h];
  const ctx = out.getContext("2d")!;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(full, 0, 0, w, h);
  const blob = await new Promise<Blob>((done, fail) =>
    out.toBlob((b) => (b ? done(b) : fail(new Error("The picture could not be made"))), "image/png"),
  );
  return base64(new Uint8Array(await blob.arrayBuffer()));
}

/** `bytes` as base64, a chunk at a time (a whole picture at once overflows the call stack). */
export function base64(bytes: Uint8Array): string {
  let text = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(text);
}
