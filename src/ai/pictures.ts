/** Pictures the user pastes, drops or picks for Mizan: read, at most 1600 pixels a side, ready to
 *  send (`qiyas_ai::UserImage`), with a data URL for the thumbnail. */
import type { AiImage } from "../api";
import { base64, fitSize } from "../viewport/capture";

/** Most pictures in one message (`qiyas_ai::context::MAX_USER_IMAGES`). */
export const MAX_PICTURES = 4;
const MAX_SIDE = 1600;
/** The kinds sent as they are, when small enough. */
const KEPT = ["image/png", "image/jpeg", "image/webp", "image/gif"];
/** The most base64 the core takes for one (`qiyas_ai::context::MAX_USER_IMAGE`). */
const MAX_BASE64 = 6 * 1024 * 1024;

export type Picture = AiImage & { url: string };

/** Whether `file` is a picture the composer takes. */
export const isPicture = (file: { type: string }) => file.type.startsWith("image/");

/** `file` ready to send: as it is when it is small enough, else drawn again smaller (a PNG stays a
 *  PNG unless that is too big; anything else becomes a JPEG). */
export async function readPicture(file: Blob): Promise<Picture> {
  const bitmap = await createImageBitmap(file);
  try {
    const [w, h] = fitSize(bitmap.width, bitmap.height, MAX_SIDE);
    const fits = w === bitmap.width && h === bitmap.height && (file.size * 4) / 3 < MAX_BASE64;
    if (KEPT.includes(file.type) && fits) {
      return picture(file.type, new Uint8Array(await file.arrayBuffer()));
    }
    const canvas = document.createElement("canvas");
    [canvas.width, canvas.height] = [w, h];
    canvas.getContext("2d")!.drawImage(bitmap, 0, 0, w, h);
    let type = file.type === "image/png" ? "image/png" : "image/jpeg";
    let blob = await toBlob(canvas, type, 0.9);
    if ((blob.size * 4) / 3 >= MAX_BASE64) {
      type = "image/jpeg";
      blob = await toBlob(canvas, type, 0.8);
    }
    return picture(type, new Uint8Array(await blob.arrayBuffer()));
  } finally {
    bitmap.close();
  }
}

function picture(media_type: string, bytes: Uint8Array): Picture {
  const data = base64(bytes);
  return { media_type, data, url: `data:${media_type};base64,${data}` };
}

function toBlob(canvas: HTMLCanvasElement, type: string, quality: number): Promise<Blob> {
  return new Promise((done, fail) =>
    canvas.toBlob((b) => (b ? done(b) : fail(new Error("The picture could not be made"))), type, quality),
  );
}
