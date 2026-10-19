/** Line segments, text glyph quads and underlay image quads of one sheet, decoded from
 *  `render_batch` (`Batch::to_bytes` in qiyas-render); the glyph atlas pages from `atlas_pages`
 *  (`Atlas::to_bytes`); underlay pictures from `underlay_image`. */

/** The segments (or glyph quads) of one drawn entity. Inside blocks, `top` is the outermost block
 *  reference on the sheet, `entity` the entity itself and `chain` an index into `Batch.chains`; at
 *  the top level `top === entity` and `chain === NO_CHAIN`. `flags` bit 0 (`TEXT_BOX`): a text's
 *  box, picked and highlighted but not drawn. */
export type Range = {
  top: bigint;
  entity: bigint;
  chain: number;
  first: number;
  count: number;
  flags: number;
};

/** `Range.flags`: the segments are a text's box (its glyph quads draw it). */
export const TEXT_BOX = 1;
/** `Range.flags`: the segments are a hatch's pattern lines. */
export const PATTERN = 2;

/** The ranges section read in place: a big drawing has hundreds of thousands of ranges, too many
 *  to turn into objects. Range `i` is the 32-byte record `i`. */
export class Ranges {
  /** `wide` views the section as u64s (4 per record), `narrow` as u32s (8 per record). */
  constructor(
    private readonly wide: BigUint64Array,
    private readonly narrow: Uint32Array,
  ) {}

  get length() {
    return this.wide.length / 4;
  }
  top(i: number) {
    return this.wide[4 * i];
  }
  entity(i: number) {
    return this.wide[4 * i + 1];
  }
  chain(i: number) {
    return this.narrow[8 * i + 4];
  }
  first(i: number) {
    return this.narrow[8 * i + 5];
  }
  count(i: number) {
    return this.narrow[8 * i + 6];
  }
  flags(i: number) {
    return this.narrow[8 * i + 7];
  }
  /** Range `i` as an object, for tests and debugging. */
  at(i: number): Range {
    const [top, entity, chain, first, count, flags] = [
      this.top(i),
      this.entity(i),
      this.chain(i),
      this.first(i),
      this.count(i),
      this.flags(i),
    ];
    return { top, entity, chain, first, count, flags };
  }
}

/** One glyph quad: corners relative to the batch origin (top-left, top-right, bottom-right,
 *  bottom-left of the glyph), texel rectangle `u0 v0 u1 v1` on atlas page `page`. */
export type Quad = { corners: number[]; uv: number[]; color: number; page: number; flags: number };

/** `Quad.flags`: the text sits inside a block. */
export const QUAD_IN_BLOCK = 1;

/** The glyph quads section read in place; quad `i` is the 48-byte record `i`. */
export class Quads {
  /** The same bytes as f32 (12 per record), u16 (24) and u32 (12). */
  constructor(
    readonly f32: Float32Array,
    readonly u16: Uint16Array,
    readonly u32: Uint32Array,
  ) {}

  get length() {
    return this.f32.length / 12;
  }
  /** Corner `k` (0 to 3) of quad `i`: x, y relative to the batch origin. */
  x(i: number, k: number) {
    return this.f32[12 * i + 2 * k];
  }
  y(i: number, k: number) {
    return this.f32[12 * i + 2 * k + 1];
  }
  /** `u0 v0 u1 v1` for `k` 0 to 3. */
  uv(i: number, k: number) {
    return this.u16[24 * i + 16 + k];
  }
  color(i: number) {
    return this.u32[12 * i + 10];
  }
  page(i: number) {
    return this.u16[24 * i + 22];
  }
  flags(i: number) {
    return this.u16[24 * i + 23];
  }
  at(i: number): Quad {
    return {
      corners: Array.from(this.f32.subarray(12 * i, 12 * i + 8)),
      uv: [0, 1, 2, 3].map((k) => this.uv(i, k)),
      color: this.color(i),
      page: this.page(i),
      flags: this.flags(i),
    };
  }
}

/** `ImageQuads.flags`: draw the picture in grey. */
export const IMAGE_MONOCHROME = 1;
/** `ImageQuads.flags`: draw the picture half faded into the background. */
export const IMAGE_FADED = 2;
/** `ImageQuads.flags`: the upper bytes hold an image's brightness (bits 8 to 15), contrast (16 to
 *  23) and fade (24 to 31), 0 to 100 each, as AutoCAD keeps them; the fade replaces
 *  `IMAGE_FADED`'s half. */
export const IMAGE_ADJUSTED = 4;

/** The image quads section read in place; quad `i` is the 40-byte record `i`: corners (top-left,
 *  top-right, bottom-right, bottom-left of the picture) relative to the batch origin, image id,
 *  flags. */
export class ImageQuads {
  /** The same bytes as f32 and u32 (10 per record). */
  constructor(
    readonly f32: Float32Array,
    readonly u32: Uint32Array,
  ) {}

  get length() {
    return this.f32.length / 10;
  }
  x(i: number, k: number) {
    return this.f32[10 * i + 2 * k];
  }
  y(i: number, k: number) {
    return this.f32[10 * i + 2 * k + 1];
  }
  id(i: number) {
    return this.u32[10 * i + 8];
  }
  flags(i: number) {
    return this.u32[10 * i + 9];
  }
  at(i: number) {
    return {
      corners: Array.from(this.f32.subarray(10 * i, 10 * i + 8)),
      id: this.id(i),
      flags: this.flags(i),
    };
  }
}

/** `Fills.color` bit: fill with the background colour (a wipeout), not the RGB below it. */
export const FILL_BACKGROUND = 0x01000000;

/** The fill triangles section read in place; triangle `i` is the 28-byte record `i`: corners
 *  `x0 y0 x1 y1 x2 y2` relative to the batch origin, then `0xRRGGBB` or `FILL_BACKGROUND`. */
export class Fills {
  /** The same bytes as f32 and u32 (7 per record). */
  constructor(
    readonly f32: Float32Array,
    readonly u32: Uint32Array,
  ) {}

  get length() {
    return this.f32.length / 7;
  }
  /** Corner `k` (0 to 2) of triangle `i`. */
  x(i: number, k: number) {
    return this.f32[7 * i + 2 * k];
  }
  y(i: number, k: number) {
    return this.f32[7 * i + 2 * k + 1];
  }
  color(i: number) {
    return this.u32[7 * i + 6];
  }
  at(i: number) {
    return { corners: Array.from(this.f32.subarray(7 * i, 7 * i + 6)), color: this.color(i) };
  }
}

/** The segments to draw before the wipeouts (`lines`) and the wipeouts' own frames drawn after
 *  them (`frames`), as `[first, count]` spans of the segments in order. A wipeout's background
 *  fill goes between the two, so it hides the line work under it. */
export function wipeoutSpans(b: Batch): { lines: [number, number][]; frames: [number, number][] } {
  const wipeouts = new Set<bigint>();
  for (let r = 0; r < b.fillRanges.length; r++) {
    if (b.fills.color(b.fillRanges.first(r)) & FILL_BACKGROUND) wipeouts.add(b.fillRanges.entity(r));
  }
  const total = b.segments.length / 4;
  if (wipeouts.size === 0) return { lines: total > 0 ? [[0, total]] : [], frames: [] };
  const frames: [number, number][] = [];
  for (let r = 0; r < b.ranges.length; r++) {
    if (wipeouts.has(b.ranges.entity(r))) frames.push([b.ranges.first(r), b.ranges.count(r)]);
  }
  frames.sort((x, y) => x[0] - y[0]);
  const lines: [number, number][] = [];
  let at = 0;
  for (const [first, count] of frames) {
    if (first > at) lines.push([at, first - at]);
    at = Math.max(at, first + count);
  }
  if (at < total) lines.push([at, total - at]);
  return { lines, frames };
}

/** The pixels image quads need: picture `id` as of `version`. */
export type ImageEntry = { id: number; version: number };

export type Batch = {
  /** `x0 y0 x1 y1` per segment, relative to `origin`. */
  segments: Float32Array;
  /** `0xRRGGBB` per segment. */
  colors: Uint32Array;
  /** One per drawn entity, in segment order; every segment is in exactly one. */
  ranges: Ranges;
  /** Block-reference handles leading to nested entities, outermost first. */
  chains: bigint[][];
  /** `[[min x, min y], [max x, max y]]` in absolute drawing coordinates; zeros when empty. */
  bounds: [[number, number], [number, number]];
  /** Absolute drawing point that `segments` and `quads` are relative to. */
  origin: [number, number];
  /** Glyph quads of the texts. */
  quads: Quads;
  /** One per drawn text, over `quads`, in quad order. */
  textRanges: Ranges;
  /** The atlas version and page count the quads need (see `parseAtlas`). */
  atlasVersion: number;
  atlasPages: number;
  /** The zoom the glyphs were drawn for, in screen pixels per drawing unit; 0 without texts. */
  pxPerUnit: number;
  /** The tolerance its curves were flattened within, and the drawing's own (0.05 mm in its
   *  units), in drawing units; 0 or left out when not known (see `curvesNeedRedraw`). */
  curveTol?: number;
  baseTol?: number;
  /** Underlay pictures, drawn under the lines. */
  images: ImageQuads;
  /** One per picture id the image quads use. */
  imageTable: ImageEntry[];
  /** Filled triangles (solid hatches, 2D solids, wipeouts), drawn under the lines. */
  fills: Fills;
  /** One per filled entity, over `fills`, in triangle order. */
  fillRanges: Ranges;
};

/** Something drawn, to highlight or frame: an entity on the sheet by its handle (`chain` empty), or
 *  an entity inside blocks by its own handle and the block references leading to it. */
export type Target = { handle: number; chain: number[] };

/** "BYIQ": `0x51495942` read little-endian. */
const MAGIC = 0x51495942;
const VERSION = 2;
export const NO_CHAIN = 0xffffffff;
const SECTION = {
  ranges: 3,
  chains: 4,
  segments: 5,
  colors: 6,
  quads: 7,
  atlas: 8,
  textRanges: 9,
  images: 10,
  imageTable: 11,
  fills: 12,
  fillRanges: 13,
};

/**
 * Decodes layout version 2 documented on `Batch::to_bytes` (crates/qiyas-render/src/batch.rs): a
 * header, then a table of sections found by kind; unknown kinds are skipped, and the text sections
 * (7, 8, 9), the image sections (10, 11) and the fill sections (12, 13) may be left out (no texts,
 * no underlays, no fills). The typed views share `buf` (every section is 8-byte
 * aligned) and read in platform byte order, which is little-endian on every desktop platform Qiyas
 * targets.
 */
export function parseBatch(buf: ArrayBuffer): Batch {
  const v = new DataView(buf);
  if (buf.byteLength < 60 || v.getUint32(0, true) !== MAGIC) {
    throw new Error("Render batch is not in the Qiyas batch format");
  }
  const version = v.getUint32(4, true);
  if (version !== VERSION) {
    throw new Error(`Render batch is layout version ${version}; this window reads version ${VERSION}`);
  }
  const f = (i: number) => v.getFloat64(8 + 8 * i, true);
  const k = v.getUint32(56, true);
  if (60 + 12 * k > buf.byteLength) throw new Error("Render batch section table too short");
  const sections = new Map<number, { at: number; length: number }>();
  for (let i = 0; i < k; i++) {
    const entry = 60 + 12 * i;
    const kind = v.getUint32(entry, true);
    const at = v.getUint32(entry + 4, true);
    const length = v.getUint32(entry + 8, true);
    if (at + length > buf.byteLength) throw new Error(`Render batch section ${kind} too short`);
    sections.set(kind, { at, length });
  }
  /** A required section; its length must be a whole number of `unit`-byte records. */
  const section = (kind: number, unit = 1) => {
    const found = sections.get(kind);
    if (!found) throw new Error(`Render batch missing section ${kind}`);
    if (found.length % unit !== 0) throw new Error(`Render batch section ${kind} too short`);
    return found;
  };

  const segments = section(SECTION.segments, 16);
  const n = segments.length / 16;
  const colors = section(SECTION.colors, 4);
  if (colors.length !== 4 * n) throw new Error(`Render batch section ${SECTION.colors} too short`);

  const c = section(SECTION.chains);
  const chains: bigint[][] = [];
  for (let at = c.at; at < c.at + c.length; ) {
    const m = at + 4 <= c.at + c.length ? v.getUint32(at, true) : -1;
    if (m < 0 || at + 4 + 8 * m > c.at + c.length) {
      throw new Error(`Render batch section ${SECTION.chains} too short`);
    }
    const chain = [];
    for (let j = 0; j < m; j++) chain.push(v.getBigUint64(at + 4 + 8 * j, true));
    chains.push(chain);
    at += 4 + 8 * m;
  }

  /** The ranges section `kind`; its ranges must cover `n` records in order and name chains that
   *  exist. */
  const readRanges = (kind: number, found: { at: number; length: number }, n: number) => {
    const ranges = new Ranges(
      new BigUint64Array(buf, found.at, found.length / 8),
      new Uint32Array(buf, found.at, found.length / 4),
    );
    let covered = 0;
    for (let i = 0; i < ranges.length; i++) {
      const chain = ranges.chain(i);
      if (ranges.first(i) !== covered || (chain !== NO_CHAIN && chain >= chains.length)) {
        const what =
          kind === SECTION.ranges ? "segments" : kind === SECTION.fillRanges ? "triangles" : "quads";
        throw new Error(`Render batch section ${kind} does not match the ${what}`);
      }
      covered += ranges.count(i);
    }
    if (covered !== n) throw new Error(`Render batch section ${kind} too short`);
    return ranges;
  };
  const ranges = readRanges(SECTION.ranges, section(SECTION.ranges, 32), n);

  // The text sections: absent means no texts.
  const empty = { at: 0, length: 0 };
  const q = sections.has(SECTION.quads) ? section(SECTION.quads, 48) : empty;
  const quads = new Quads(
    new Float32Array(buf, q.at, q.length / 4),
    new Uint16Array(buf, q.at, q.length / 2),
    new Uint32Array(buf, q.at, q.length / 4),
  );
  const t = sections.has(SECTION.textRanges) ? section(SECTION.textRanges, 32) : empty;
  const textRanges = readRanges(SECTION.textRanges, t, quads.length);
  let [atlasVersion, atlasPages, pxPerUnit, curveTol, baseTol] = [0, 0, 0, 0, 0];
  const a = sections.get(SECTION.atlas);
  if (a) {
    if (a.length < 24) throw new Error(`Render batch section ${SECTION.atlas} too short`);
    atlasVersion = Number(v.getBigUint64(a.at, true));
    atlasPages = v.getUint32(a.at + 8, true);
    pxPerUnit = v.getFloat64(a.at + 16, true);
    if (a.length >= 40) {
      curveTol = v.getFloat64(a.at + 24, true);
      baseTol = v.getFloat64(a.at + 32, true);
    }
  }

  const im = sections.has(SECTION.images) ? section(SECTION.images, 40) : empty;
  const images = new ImageQuads(
    new Float32Array(buf, im.at, im.length / 4),
    new Uint32Array(buf, im.at, im.length / 4),
  );
  const tb = sections.has(SECTION.imageTable) ? section(SECTION.imageTable, 16) : empty;
  const imageTable: ImageEntry[] = [];
  for (let at = tb.at; at < tb.at + tb.length; at += 16) {
    imageTable.push({ id: v.getUint32(at, true), version: Number(v.getBigUint64(at + 8, true)) });
  }
  for (let i = 0; i < images.length; i++) {
    if (!imageTable.some((e) => e.id === images.id(i))) {
      throw new Error(`Render batch section ${SECTION.images} names a picture not in section ${SECTION.imageTable}`);
    }
  }

  // The fill sections: absent means no fills.
  const fl = sections.has(SECTION.fills) ? section(SECTION.fills, 28) : empty;
  const fills = new Fills(
    new Float32Array(buf, fl.at, fl.length / 4),
    new Uint32Array(buf, fl.at, fl.length / 4),
  );
  const fr = sections.has(SECTION.fillRanges) ? section(SECTION.fillRanges, 32) : empty;
  const fillRanges = readRanges(SECTION.fillRanges, fr, fills.length);

  return {
    bounds: [
      [f(0), f(1)],
      [f(2), f(3)],
    ],
    origin: [f(4), f(5)],
    ranges,
    chains,
    segments: new Float32Array(buf, segments.at, 4 * n),
    colors: new Uint32Array(buf, colors.at, n),
    quads,
    textRanges,
    atlasVersion,
    atlasPages,
    pxPerUnit,
    curveTol,
    baseTol,
    images,
    imageTable,
    fills,
    fillRanges,
  };
}

/** One glyph atlas page: `width` x `height` coverage bytes (R8), rows from the top. */
export type AtlasPage = { index: number; width: number; height: number; data: Uint8Array };

/** The atlas pages changed since the version asked for, and the atlas version they bring. */
export type AtlasUpdate = { version: number; pages: AtlasPage[] };

/** Decodes `atlas_pages` (`Atlas::to_bytes` in qiyas-render): `u64` version, `u32` page count, then
 *  per page `u32` index, width, height and byte length, and the bytes. Little-endian. */
export function parseAtlas(buf: ArrayBuffer): AtlasUpdate {
  const v = new DataView(buf);
  if (buf.byteLength < 12) throw new Error("Atlas pages too short");
  const version = Number(v.getBigUint64(0, true));
  const count = v.getUint32(8, true);
  const pages: AtlasPage[] = [];
  let at = 12;
  for (let i = 0; i < count; i++) {
    if (at + 16 > buf.byteLength) throw new Error("Atlas pages too short");
    const [index, width, height, length] = [0, 4, 8, 12].map((o) => v.getUint32(at + o, true));
    if (length !== width * height || at + 16 + length > buf.byteLength) {
      throw new Error("Atlas pages too short");
    }
    pages.push({ index, width, height, data: new Uint8Array(buf, at + 16, length) });
    at += 16 + length;
  }
  return { version, pages };
}

/** The parts of an image quad's flags. Without `IMAGE_ADJUSTED` (a PDF underlay) brightness and
 *  contrast are neutral (50) and the fade is 50 when `IMAGE_FADED`, else 0. */
export function unpackImageFlags(flags: number): {
  monochrome: boolean;
  brightness: number;
  contrast: number;
  fade: number;
} {
  const monochrome = (flags & IMAGE_MONOCHROME) !== 0;
  if (!(flags & IMAGE_ADJUSTED)) {
    return { monochrome, brightness: 50, contrast: 50, fade: flags & IMAGE_FADED ? 50 : 0 };
  }
  const byte = (shift: number) => Math.min(100, (flags >>> shift) & 255);
  return { monochrome, brightness: byte(8), contrast: byte(16), fade: byte(24) };
}

/** Image quad flags for an image's brightness, contrast and fade (each cut to 0..100), as the core
 *  packs them. */
export function packImageFlags(brightness: number, contrast: number, fade: number): number {
  const byte = (v: number) => Math.max(0, Math.min(100, Math.round(v)));
  return (IMAGE_ADJUSTED | (byte(brightness) << 8) | (byte(contrast) << 16) | (byte(fade) << 24)) >>> 0;
}

/** What the image shader is given for a quad's flags: `brightness` is added to each colour channel
 *  (-1 at 0, 0 at 50, +1 at 100), `contrast` scales it about mid grey (0 at 0, 1 at 50, 2 at 100),
 *  and `fade` mixes it into the background (0 to 1). An approximation of AutoCAD's image adjust. */
export function imageUniforms(flags: number): {
  monochrome: boolean;
  brightness: number;
  contrast: number;
  fade: number;
} {
  const f = unpackImageFlags(flags);
  return {
    monochrome: f.monochrome,
    brightness: (f.brightness - 50) / 50,
    contrast: f.contrast / 50,
    fade: f.fade / 100,
  };
}

/** An underlay picture: `width` x `height` RGBA bytes, rows from the top. */
export type Picture = { width: number; height: number; data: Uint8Array };

/** Decodes `underlay_image`: `u32` width, `u32` height, then the RGBA bytes. */
export function parsePicture(buf: ArrayBuffer): Picture {
  const v = new DataView(buf);
  if (buf.byteLength < 8) throw new Error("Underlay picture too short");
  const [width, height] = [v.getUint32(0, true), v.getUint32(4, true)];
  if (buf.byteLength !== 8 + 4 * width * height) throw new Error("Underlay picture too short");
  return { width, height, data: new Uint8Array(buf, 8) };
}

/** Decodes `pdf_thumbnails`: `u32` count, then per page `u32` page, width, height and the RGBA
 *  bytes. */
export function parseThumbnails(buf: ArrayBuffer): (Picture & { page: number })[] {
  const v = new DataView(buf);
  if (buf.byteLength < 4) throw new Error("Thumbnails too short");
  const out = [];
  let at = 4;
  for (let i = 0, n = v.getUint32(0, true); i < n; i++) {
    if (at + 12 > buf.byteLength) throw new Error("Thumbnails too short");
    const [page, width, height] = [0, 4, 8].map((o) => v.getUint32(at + o, true));
    const length = 4 * width * height;
    if (at + 12 + length > buf.byteLength) throw new Error("Thumbnails too short");
    out.push({ page, width, height, data: new Uint8Array(buf, at + 12, length) });
    at += 12 + length;
  }
  return out;
}

/** An underlay picture the window holds: drawn as of `version`, for `pxPerUnit` screen pixels per
 *  drawing unit. */
export type HeldPicture = Picture & { version: number; pxPerUnit: number };

/** `[[min x, min y], [max x, max y]]` in absolute drawing coordinates. */
export type WorldBounds = [[number, number], [number, number]];

/** How far past the view, as a share of its size each way, a picture still counts as in view. */
export const PICTURE_MARGIN = 0.25;

/** The picture ids of `batch` with a quad that meets `visible` grown by `PICTURE_MARGIN`. */
export function picturesInView(batch: Batch | null, visible: WorldBounds): Set<number> {
  const out = new Set<number>();
  if (!batch) return out;
  const [[x0, y0], [x1, y1]] = visible;
  const [mx, my] = [(x1 - x0) * PICTURE_MARGIN, (y1 - y0) * PICTURE_MARGIN];
  const [ox, oy] = batch.origin;
  const q = batch.images;
  for (let i = 0; i < q.length; i++) {
    let [lx, ly, hx, hy] = [Infinity, Infinity, -Infinity, -Infinity];
    for (let k = 0; k < 4; k++) {
      const [x, y] = [q.x(i, k) + ox, q.y(i, k) + oy];
      [lx, ly, hx, hy] = [Math.min(lx, x), Math.min(ly, y), Math.max(hx, x), Math.max(hy, y)];
    }
    if (hx >= x0 - mx && lx <= x1 + mx && hy >= y0 - my && ly <= y1 + my) out.add(q.id(i));
  }
  return out;
}

/** The box around the pictures of `after` whose ids `before` did not have (an underlay or image
 *  just attached), or null when there are none. */
export function newPictureBounds(before: Batch | null, after: Batch | null): WorldBounds | null {
  if (!after) return null;
  const old = new Set((before?.imageTable ?? []).map((e) => e.id));
  const [ox, oy] = after.origin;
  const q = after.images;
  let [lx, ly, hx, hy] = [Infinity, Infinity, -Infinity, -Infinity];
  for (let i = 0; i < q.length; i++) {
    if (old.has(q.id(i))) continue;
    for (let k = 0; k < 4; k++) {
      const [x, y] = [q.x(i, k) + ox, q.y(i, k) + oy];
      [lx, ly, hx, hy] = [Math.min(lx, x), Math.min(ly, y), Math.max(hx, x), Math.max(hy, y)];
    }
  }
  return lx <= hx ? [[lx, ly], [hx, hy]] : null;
}

/** The picture ids of `batch` in view (`picturesInView`) the window should fetch for a zoom of
 *  `now`: not held, held at another version, or held for a zoom too far off
 *  (`pictureNeedsRedraw`). Pictures out of view are never fetched. */
export function picturesToFetch(
  batch: Batch | null,
  held: Map<number, HeldPicture>,
  now: number,
  visible: WorldBounds,
): number[] {
  const inView = picturesInView(batch, visible);
  return (batch?.imageTable ?? [])
    .filter((e) => {
      if (!inView.has(e.id)) return false;
      const p = held.get(e.id);
      return !p || p.version !== e.version || pictureNeedsRedraw(p.pxPerUnit, now, p);
    })
    .map((e) => e.id);
}

/** The held picture ids to let go of: out of view (or gone from `batch`). */
export function picturesToRelease(
  batch: Batch | null,
  held: Map<number, HeldPicture>,
  visible: WorldBounds,
): number[] {
  const inView = picturesInView(batch, visible);
  return [...held.keys()].filter((id) => !inView.has(id));
}

/** The pictures to hold once those `next` needs in view are fetched (`fetch` answers the bytes of
 *  `underlay_image`) and those out of view let go: a new map, so the one held is never written
 *  to. Null when `stale()` turns true after a fetch (another drawing came on screen): nothing
 *  fetched for the old one may be kept. */
export async function fetchPictures(
  next: Batch | null,
  held: Map<number, HeldPicture>,
  pxPerUnit: number,
  visible: WorldBounds,
  fetch: (id: number) => Promise<ArrayBuffer>,
  stale: () => boolean,
): Promise<Map<number, HeldPicture> | null> {
  const out = new Map(held);
  for (const id of picturesToRelease(next, out, visible)) out.delete(id);
  for (const id of picturesToFetch(next, out, pxPerUnit, visible)) {
    const entry = next!.imageTable.find((e) => e.id === id)!;
    const bytes = await fetch(id);
    if (stale()) return null;
    out.set(id, { ...parsePicture(bytes), version: entry.version, pxPerUnit });
  }
  return out;
}

/** The longest side the core draws an underlay picture at. */
export const PICTURE_MAX_PX = 2048;

/** Whether a picture drawn for `drawnFor` pixels per unit should be fetched again at `now`: the
 *  zoom is more than twice off (as for glyphs), unless it is zoomed in on a picture already at its
 *  largest. */
export function pictureNeedsRedraw(drawnFor: number, now: number, picture: Picture): boolean {
  if (!glyphsNeedRedraw(drawnFor, now)) return false;
  return !(now > drawnFor && Math.max(picture.width, picture.height) >= PICTURE_MAX_PX);
}

/** Whether glyphs drawn for `drawnFor` screen pixels per drawing unit look poor at `now`: more than
 *  twice as big or small. */
/** Screen pixels a chord may stray from its curve when zoomed in past the drawing's own
 *  tolerance (`qiyas_core::ZOOM_TOL_PX`). */
export const ZOOM_TOL_PX = 0.25;

/** Whether curves drawn within `current` drawing units should be drawn again at `pxPerUnit` screen
 *  pixels per drawing unit, as the core's `zoom_tol` decides: more than twice too coarse (a
 *  corner could show), or eight times finer than needed while finer than the drawing's own
 *  `base`. False when either is not known. */
export function curvesNeedRedraw(base: number, current: number, pxPerUnit: number): boolean {
  if (!(base > 0) || !(current > 0) || !(pxPerUnit > 0) || !Number.isFinite(pxPerUnit)) return false;
  const want = Math.min(ZOOM_TOL_PX / pxPerUnit, base);
  return current > 2 * want || (current < want / 8 && current < base);
}

export function glyphsNeedRedraw(drawnFor: number, now: number): boolean {
  if (!(drawnFor > 0) || !(now > 0)) return false;
  const ratio = now / drawnFor;
  return ratio > 2 || ratio < 0.5;
}

/** A test for "is range `i` of `batch` one of `targets`": a target with no chain matches its whole
 *  top-level entity; one with a chain matches only that entity reached through that chain. Over the
 *  segment ranges, or `ranges` (the text ranges). */
export function rangeMatcher(
  batch: Batch,
  targets: Target[],
  ranges: Ranges = batch.ranges,
): (i: number) => boolean {
  const tops = new Set<bigint>();
  const inner = new Set<string>();
  for (const t of targets) {
    if (t.chain.length === 0) tops.add(BigInt(t.handle));
    else inner.add(`${t.handle}@${t.chain.join(",")}`);
  }
  const { chains } = batch;
  return (i) => {
    if (tops.has(ranges.top(i))) return true;
    const chain = ranges.chain(i);
    if (inner.size === 0 || chain === NO_CHAIN) return false;
    return inner.has(`${ranges.entity(i)}@${(chains[chain] ?? []).join(",")}`);
  };
}

/** The segments `[first, first + count)` of every range of `batch` that matches one of `targets`;
 *  with `ranges` the text ranges, the glyph quads. */
export function* matchingRanges(
  batch: Batch,
  targets: Target[],
  ranges: Ranges = batch.ranges,
): Generator<[number, number]> {
  if (targets.length === 0) return;
  const matches = rangeMatcher(batch, targets, ranges);
  for (let i = 0; i < ranges.length; i++) {
    if (matches(i)) yield [ranges.first(i), ranges.count(i)];
  }
}
