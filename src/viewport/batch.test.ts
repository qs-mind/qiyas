import { expect, test } from "vitest";
import {
  curvesNeedRedraw,
  glyphsNeedRedraw,
  IMAGE_ADJUSTED,
  IMAGE_FADED,
  imageUniforms,
  IMAGE_MONOCHROME,
  matchingRanges,
  newPictureBounds,
  NO_CHAIN,
  packImageFlags,
  parseAtlas,
  parseBatch,
  parsePicture,
  parseThumbnails,
  pictureNeedsRedraw,
  fetchPictures,
  picturesToFetch,
  picturesToRelease,
  rangeMatcher,
  type HeldPicture,
  FILL_BACKGROUND,
  PATTERN,
  TEXT_BOX,
  unpackImageFlags,
  wipeoutSpans,
  Fills,
  Ranges,
  type Batch,
} from "./batch";

/** A version 2 batch, laid out as `Batch::to_bytes` does: two segments, the first of a line at the
 *  top level (handle 7), the second of a line (9) inside block reference 8. `extra` adds a section
 *  of an unknown kind before the known ones; `without` leaves a section out. */
function v2(extra = false, without = 0): ArrayBuffer {
  const sections: [number, number][] = [
    ...(extra ? ([[99, 8]] as [number, number][]) : []),
    [3, 64], // ranges
    [4, 12], // chains: one chain of one handle
    [5, 32], // segments
    [6, 8], // colours
  ].filter(([kind]) => kind !== without) as [number, number][];
  const align = (n: number) => Math.ceil(n / 8) * 8;
  let at = align(60 + 12 * sections.length);
  const placed = sections.map(([kind, length]) => {
    const entry = { kind, at, length };
    at = align(at + length);
    return entry;
  });
  const buf = new ArrayBuffer(at);
  const v = new DataView(buf);
  v.setUint32(0, 0x51495942, true);
  v.setUint32(4, 2, true);
  [100, 200, 104, 203].forEach((x, i) => v.setFloat64(8 + 8 * i, x, true));
  v.setFloat64(40, 102, true);
  v.setFloat64(48, 201.5, true);
  v.setUint32(56, placed.length, true);
  placed.forEach(({ kind, at, length }, i) => {
    v.setUint32(60 + 12 * i, kind, true);
    v.setUint32(64 + 12 * i, at, true);
    v.setUint32(68 + 12 * i, length, true);
  });
  // A left-out section's data goes nowhere (offset past the end is never written).
  const of = (kind: number) => placed.find((p) => p.kind === kind)?.at ?? -1;
  if (without !== 0) return buf;
  const ranges = of(3);
  [
    [7n, 7n, NO_CHAIN, 0],
    [8n, 9n, 0, 1],
  ].forEach(([top, entity, chain, first], i) => {
    const r = ranges + 32 * i;
    v.setBigUint64(r, top as bigint, true);
    v.setBigUint64(r + 8, entity as bigint, true);
    v.setUint32(r + 16, chain as number, true);
    v.setUint32(r + 20, first as number, true);
    v.setUint32(r + 24, 1, true);
  });
  v.setUint32(of(4), 1, true);
  v.setBigUint64(of(4) + 4, 8n, true);
  [-2, -1.5, 2, 1.5, 0, 0, 1, 1].forEach((x, i) => v.setFloat32(of(5) + 4 * i, x, true));
  v.setUint32(of(6), 0xff0000, true);
  v.setUint32(of(6) + 4, 0x00ff00, true);
  return buf;
}

test("parseBatch reads the header and the section table", () => {
  for (const extra of [false, true]) {
    const b = parseBatch(v2(extra));
    expect(Array.from(b.segments)).toEqual([-2, -1.5, 2, 1.5, 0, 0, 1, 1]);
    expect(b.ranges.length).toBe(2);
    expect([b.ranges.at(0), b.ranges.at(1)]).toEqual([
      { top: 7n, entity: 7n, chain: NO_CHAIN, first: 0, count: 1, flags: 0 },
      { top: 8n, entity: 9n, chain: 0, first: 1, count: 1, flags: 0 },
    ]);
    // A batch without the text sections has no texts.
    expect(b.quads.length).toBe(0);
    expect(b.textRanges.length).toBe(0);
    expect([b.atlasVersion, b.atlasPages, b.pxPerUnit]).toEqual([0, 0, 0]);
    expect(b.chains).toEqual([[8n]]);
    expect(Array.from(b.colors)).toEqual([0xff0000, 0x00ff00]);
    expect(b.bounds).toEqual([
      [100, 200],
      [104, 203],
    ]);
    expect(b.origin).toEqual([102, 201.5]);
  }
});

test("parseBatch rejects other versions and other data", () => {
  const old = v2();
  new DataView(old).setUint32(4, 1, true);
  expect(() => parseBatch(old)).toThrow(/version 1/);
  // A version 1 batch started with the segment count, not the magic.
  const v1 = new ArrayBuffer(100);
  new DataView(v1).setUint32(0, 1, true);
  expect(() => parseBatch(v1)).toThrow(/not in the Qiyas batch format/);
});

test("parseBatch rejects a missing section or a cut-short buffer", () => {
  for (const kind of [3, 4, 5, 6]) {
    expect(() => parseBatch(v2(false, kind))).toThrow(`missing section ${kind}`);
  }
  const whole = v2();
  // Cut inside the colours: their section no longer fits.
  expect(() => parseBatch(whole.slice(0, whole.byteLength - 4))).toThrow("section 6 too short");
  // Cut inside the section table.
  expect(() => parseBatch(whole.slice(0, 70))).toThrow("section table too short");
  // Colours for one segment only.
  const short = v2();
  const sv = new DataView(short);
  const colorsEntry = 60 + 12 * 3;
  expect(sv.getUint32(colorsEntry, true)).toBe(6);
  sv.setUint32(colorsEntry + 8, 4, true);
  expect(() => parseBatch(short)).toThrow("section 6 too short");
});

test("rangeMatcher tells a whole block from an entity inside it", () => {
  const b = parseBatch(v2());
  const [line, leaf] = [0, 1];
  const whole = rangeMatcher(b, [{ handle: 8, chain: [] }]);
  expect([whole(line), whole(leaf)]).toEqual([false, true]);
  const inner = rangeMatcher(b, [{ handle: 9, chain: [8] }]);
  expect([inner(line), inner(leaf)]).toEqual([false, true]);
  // The same entity through another chain is another drawn thing.
  expect(rangeMatcher(b, [{ handle: 9, chain: [5] }])(leaf)).toBe(false);
  expect(rangeMatcher(b, [{ handle: 7, chain: [] }])(line)).toBe(true);
});

/** The bytes `Batch::to_bytes` writes for `bytes_match_the_frontend_fixture`
 *  (crates/qiyas-render/tests/batch.rs): the segments of `v2()`, the first being a text's box
 *  (handle 7, flagged) drawn as one glyph quad, and the atlas section. */
const FROM_RUST = [
  "4259495102000000000000000000594000000000000069400000000000005a400000000000606940",
  "000000000080594000000000003069400700000003000000900000004000000004000000d0000000",
  "0c00000005000000e000000020000000060000000001000008000000070000000801000030000000",
  "08000000380100002800000009000000600100002000000007000000000000000700000000000000",
  "ffffffff000000000100000001000000080000000000000009000000000000000000000001000000",
  "010000000000000001000000080000000000000000000000000000c00000c0bf000000400000c03f",
  "00000000000000000000803f0000803f0000ff0000ff0000000000c00000c03f000080bf0000c03f",
  "000080bf0000003f000000c00000003f0a0014001e002c000000ff00010000000300000000000000",
  "02000000000000000000000000000440000000000000d03f000000000000e03f0700000000000000",
  "0700000000000000ffffffff000000000100000000000000",
].join("");

function fromHex(hex: string): ArrayBuffer {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i++) bytes[i] = parseInt(hex.slice(2 * i, 2 * i + 2), 16);
  return bytes.buffer;
}

test("parseBatch reads what the Rust side writes, glyph quads and text ranges too", () => {
  const b = parseBatch(fromHex(FROM_RUST));
  const lines = parseBatch(v2());
  expect(Array.from(b.segments)).toEqual(Array.from(lines.segments));
  expect(Array.from(b.colors)).toEqual(Array.from(lines.colors));
  expect(b.chains).toEqual(lines.chains);
  expect([b.bounds, b.origin]).toEqual([lines.bounds, lines.origin]);
  expect([b.ranges.at(0), b.ranges.at(1)]).toEqual([
    { top: 7n, entity: 7n, chain: NO_CHAIN, first: 0, count: 1, flags: TEXT_BOX },
    { top: 8n, entity: 9n, chain: 0, first: 1, count: 1, flags: 0 },
  ]);
  expect(b.quads.length).toBe(1);
  expect(b.quads.at(0)).toEqual({
    corners: [-2, 1.5, -1, 1.5, -1, 0.5, -2, 0.5],
    uv: [10, 20, 30, 44],
    color: 0xff0000,
    page: 1,
    flags: 0,
  });
  expect(b.textRanges.length).toBe(1);
  expect(b.textRanges.at(0)).toEqual({
    top: 7n,
    entity: 7n,
    chain: NO_CHAIN,
    first: 0,
    count: 1,
    flags: 0,
  });
  expect([b.atlasVersion, b.atlasPages, b.pxPerUnit]).toEqual([3, 2, 2.5]);
  // The curve tolerance it was drawn within and the drawing's own (curvesNeedRedraw).
  expect([b.curveTol, b.baseTol]).toEqual([0.25, 0.5]);
  // Highlighting the text finds its box among the segments and its glyph among the quads.
  const text = [{ handle: 7, chain: [] }];
  expect([...matchingRanges(b, text)]).toEqual([[0, 1]]);
  expect([...matchingRanges(b, text, b.textRanges)]).toEqual([[0, 1]]);
});

test("parseBatch rejects text ranges that do not cover the quads", () => {
  const buf = fromHex(FROM_RUST);
  const v = new DataView(buf);
  // Section table entry 6 is the text ranges (kind 9); its one range's count sits at +24.
  const entry = 60 + 12 * 6;
  expect(v.getUint32(entry, true)).toBe(9);
  const at = v.getUint32(entry + 4, true);
  v.setUint32(at + 24, 2, true);
  expect(() => parseBatch(buf)).toThrow("section 9 too short");
});

test("parseAtlas reads the pages after the version", () => {
  const page = (index: number, w: number, h: number, fill: number) => {
    const out = new Uint8Array(16 + w * h);
    const v = new DataView(out.buffer);
    [index, w, h, w * h].forEach((x, i) => v.setUint32(4 * i, x, true));
    out.fill(fill, 16);
    return out;
  };
  const parts = [page(0, 2, 3, 7), page(4, 1, 1, 9)];
  const buf = new Uint8Array(12 + parts.reduce((n, p) => n + p.length, 0));
  const v = new DataView(buf.buffer);
  v.setBigUint64(0, 5n, true);
  v.setUint32(8, 2, true);
  let at = 12;
  for (const p of parts) {
    buf.set(p, at);
    at += p.length;
  }
  const atlas = parseAtlas(buf.buffer);
  expect(atlas.version).toBe(5);
  expect(atlas.pages.map((p) => [p.index, p.width, p.height, Array.from(p.data)])).toEqual([
    [0, 2, 3, [7, 7, 7, 7, 7, 7]],
    [4, 1, 1, [9]],
  ]);
  expect(() => parseAtlas(buf.buffer.slice(0, buf.length - 1))).toThrow("too short");
  // Nothing changed since the version asked for: the version and no pages.
  const none = buf.buffer.slice(0, 12);
  new DataView(none).setUint32(8, 0, true);
  expect(parseAtlas(none)).toEqual({ version: 5, pages: [] });
});

test("curves are drawn again only when the zoom shows their corners, or no longer needs them so fine", () => {
  // At the drawing's own tolerance and a normal zoom: nothing (the core's zoom_tol agrees).
  expect(curvesNeedRedraw(0.05, 0.05, 1)).toBe(false);
  // Far in: 0.05 units is 5 px at 100 px a unit.
  expect(curvesNeedRedraw(0.05, 0.05, 100)).toBe(true);
  // Already within a quarter pixel there, and a little further in: nothing.
  expect(curvesNeedRedraw(0.05, 0.0025, 150)).toBe(false);
  // Back out: let the fine curves go.
  expect(curvesNeedRedraw(0.05, 0.0025, 1)).toBe(true);
  // Not known (an older batch): never.
  expect(curvesNeedRedraw(0, 0, 100)).toBe(false);
});

test("glyphs are drawn again when the zoom has more than doubled or halved", () => {
  expect(glyphsNeedRedraw(10, 19)).toBe(false);
  expect(glyphsNeedRedraw(10, 21)).toBe(true);
  expect(glyphsNeedRedraw(10, 4.9)).toBe(true);
  expect(glyphsNeedRedraw(0, 100)).toBe(false);
});

/** The bytes `Batch::to_bytes` writes for `image_sections_match_the_frontend_fixture`
 *  (crates/qiyas-render/tests/batch.rs): no lines, one picture (id 3, faded and grey) whose corners
 *  are (0, 10), (20, 10), (20, 0), (0, 0), version 2^53 - 1. */
const IMAGES_FROM_RUST = [
  "42594951020000000000000000000000000000000000000000000000000034400000000000002440",
  "000000000000244000000000000014400900000003000000a80000000000000004000000a8000000",
  "0000000005000000a80000000000000006000000a80000000000000007000000a800000000000000",
  "08000000a80000002800000009000000d0000000000000000a000000d0000000280000000b000000",
  "f8000000100000000000000000000000000000000000000000000000000000000000000000000000",
  "0000000000000000000020c10000a040000020410000a040000020410000a0c0000020c10000a0c0",
  "03000000030000000300000000000000ffffffffffff1f00",
].join("");

test("parseBatch reads the image quads and the image table the Rust side writes", () => {
  const b = parseBatch(fromHex(IMAGES_FROM_RUST));
  expect(b.segments.length).toBe(0);
  expect([b.bounds, b.origin]).toEqual([
    [
      [0, 0],
      [20, 10],
    ],
    [10, 5],
  ]);
  expect(b.images.length).toBe(1);
  expect(b.images.at(0)).toEqual({
    corners: [-10, 5, 10, 5, 10, -5, -10, -5],
    id: 3,
    flags: IMAGE_FADED | IMAGE_MONOCHROME,
  });
  expect(b.imageTable).toEqual([{ id: 3, version: 2 ** 53 - 1 }]);
  // A batch without them has none.
  const plain = parseBatch(fromHex(FROM_RUST));
  expect([plain.images.length, plain.imageTable]).toEqual([0, []]);
});

test("parseBatch rejects a picture missing from the image table", () => {
  const buf = fromHex(IMAGES_FROM_RUST);
  const v = new DataView(buf);
  // Section table entry 7 is the image quads (kind 10); the id sits at +32 of the quad.
  const entry = 60 + 12 * 7;
  expect(v.getUint32(entry, true)).toBe(10);
  v.setUint32(v.getUint32(entry + 4, true) + 32, 4, true);
  expect(() => parseBatch(buf)).toThrow("not in section 11");
});

test("pictures and thumbnails decode, and a picture at its largest is not fetched again", () => {
  const picture = new Uint8Array(8 + 2 * 1 * 4);
  new DataView(picture.buffer).setUint32(0, 2, true);
  new DataView(picture.buffer).setUint32(4, 1, true);
  picture.fill(9, 8);
  const p = parsePicture(picture.buffer);
  expect([p.width, p.height, Array.from(p.data)]).toEqual([2, 1, [9, 9, 9, 9, 9, 9, 9, 9]]);
  expect(() => parsePicture(picture.buffer.slice(0, 12))).toThrow("too short");

  const thumbs = new Uint8Array(4 + 12 + 4);
  const tv = new DataView(thumbs.buffer);
  [1, 7, 1, 1].forEach((x, i) => tv.setUint32(4 * i, x, true));
  thumbs.fill(5, 16);
  expect(parseThumbnails(thumbs.buffer).map((t) => [t.page, t.width, t.height])).toEqual([[7, 1, 1]]);
  expect(() => parseThumbnails(thumbs.buffer.slice(0, 18))).toThrow("too short");

  const small = { width: 1024, height: 512, data: new Uint8Array() };
  const largest = { width: 2048, height: 1024, data: new Uint8Array() };
  expect(pictureNeedsRedraw(100, 150, small)).toBe(false);
  expect(pictureNeedsRedraw(100, 250, small)).toBe(true);
  expect(pictureNeedsRedraw(100, 250, largest)).toBe(false);
  expect(pictureNeedsRedraw(100, 40, largest)).toBe(true);
});

test("only pictures in view (with a quarter margin) are fetched, and those out of view let go", () => {
  // Picture 3 covers x 0 to 20, y 0 to 10.
  const b = parseBatch(fromHex(IMAGES_FROM_RUST));
  const none = new Map<number, HeldPicture>();
  expect(picturesToFetch(b, none, 10, [[0, 0], [20, 10]])).toEqual([3]);
  expect(picturesToFetch(b, none, 10, [[100, 100], [110, 110]])).toEqual([]);
  // A view 10 wide starting at x 22: its margin (2.5) reaches back to 19.5, over the picture.
  expect(picturesToFetch(b, none, 10, [[22, 0], [32, 10]])).toEqual([3]);
  expect(picturesToFetch(b, none, 10, [[26, 0], [36, 10]])).toEqual([]);

  const held = new Map<number, HeldPicture>([
    [3, { width: 512, height: 256, data: new Uint8Array(), version: 2 ** 53 - 1, pxPerUnit: 10 }],
  ]);
  // Held at this version and zoom: nothing to fetch; zoomed 4x: fetched again; new version too.
  expect(picturesToFetch(b, held, 12, [[0, 0], [20, 10]])).toEqual([]);
  expect(picturesToFetch(b, held, 40, [[0, 0], [20, 10]])).toEqual([3]);
  held.get(3)!.version = 1;
  expect(picturesToFetch(b, held, 12, [[0, 0], [20, 10]])).toEqual([3]);
  // Out of view: let go; a picture of another batch: let go too.
  expect(picturesToRelease(b, held, [[0, 0], [20, 10]])).toEqual([]);
  expect(picturesToRelease(b, held, [[100, 100], [110, 110]])).toEqual([3]);
  expect(picturesToRelease(null, held, [[0, 0], [20, 10]])).toEqual([3]);
});

test("a picture that arrives after another drawing came on screen is dropped", async () => {
  const b = parseBatch(fromHex(IMAGES_FROM_RUST));
  const picture = new Uint8Array(8 + 4);
  new DataView(picture.buffer).setUint32(0, 1, true);
  new DataView(picture.buffer).setUint32(4, 1, true);
  const held = new Map<number, HeldPicture>();
  let gen = 1;
  const fetch = async () => {
    gen = 2; // a tab switch while the picture is drawn
    return picture.buffer;
  };
  const view: [[number, number], [number, number]] = [[0, 0], [20, 10]];
  expect(await fetchPictures(b, held, 10, view, fetch, () => gen !== 1)).toBeNull();
  expect(held.size).toBe(0);

  const fresh = await fetchPictures(b, held, 10, view, async () => picture.buffer, () => false);
  expect([...fresh!.keys()]).toEqual([3]);
  // The map held is never written to.
  expect(held.size).toBe(0);
});

test("image flags pack and unpack brightness, contrast and fade", () => {
  const flags = packImageFlags(70, 40, 30);
  // The same bits the core packs (`image_flags` in qiyas-render).
  expect(flags).toBe((IMAGE_ADJUSTED | (70 << 8) | (40 << 16) | (30 << 24)) >>> 0);
  expect(unpackImageFlags(flags)).toEqual({ monochrome: false, brightness: 70, contrast: 40, fade: 30 });
  // Cut to 0..100, rounded.
  expect(unpackImageFlags(packImageFlags(250, -5, 100.4))).toEqual({
    monochrome: false,
    brightness: 100,
    contrast: 0,
    fade: 100,
  });
  // A PDF underlay's flags: neutral, half faded when IMAGE_FADED.
  expect(unpackImageFlags(IMAGE_FADED | IMAGE_MONOCHROME)).toEqual({
    monochrome: true,
    brightness: 50,
    contrast: 50,
    fade: 50,
  });
  expect(unpackImageFlags(0)).toEqual({ monochrome: false, brightness: 50, contrast: 50, fade: 0 });
});

test("the shader is given neutral values for 50, 50, 0 and the extremes past them", () => {
  expect(imageUniforms(packImageFlags(50, 50, 0))).toEqual({
    monochrome: false,
    brightness: 0,
    contrast: 1,
    fade: 0,
  });
  expect(imageUniforms(packImageFlags(0, 100, 100))).toEqual({
    monochrome: false,
    brightness: -1,
    contrast: 2,
    fade: 1,
  });
  expect(imageUniforms(packImageFlags(75, 0, 25))).toEqual({
    monochrome: false,
    brightness: 0.5,
    contrast: 0,
    fade: 0.25,
  });
  // A faded PDF underlay keeps its half fade and is otherwise as it is.
  expect(imageUniforms(IMAGE_FADED)).toEqual({ monochrome: false, brightness: 0, contrast: 1, fade: 0.5 });
});

test("the box of a picture just attached is the one the batch before did not have", () => {
  // Picture 3 covers x 0 to 20, y 0 to 10.
  const b = parseBatch(fromHex(IMAGES_FROM_RUST));
  expect(newPictureBounds(null, b)).toEqual([
    [0, 0],
    [20, 10],
  ]);
  expect(newPictureBounds(b, b)).toBeNull();
  expect(newPictureBounds(b, null)).toBeNull();
});

/** The bytes `Batch::to_bytes` writes for `fill_sections_match_the_frontend_fixture`
 *  (crates/qiyas-render/tests/fills.rs): one line (handle 5), one green fill triangle (handle 6)
 *  and one triangle on the background (handle 7). */
const FILLS_FROM_RUST = [
  "42594951020000000000000000000000000000000000000000000000000000400000000000000040",
  "000000000000f03f000000000000f03f0900000003000000a80000002000000004000000c8000000",
  "0000000005000000c80000001000000006000000d80000000400000007000000e000000000000000",
  "08000000e0000000280000000900000008010000000000000c00000008010000380000000d000000",
  "400100004000000005000000000000000500000000000000ffffffff000000000100000000000000",
  "000080bf000080bf0000803f0000803f0000ff000000000000000000000000000000000000000000",
  "000000000000000000000000000000000000000000000000000080bf000080bf0000803f000080bf",
  "0000803f0000803f00ff0000000080bf000080bf0000803f0000803f000080bf0000803f00000001",
  "06000000000000000600000000000000ffffffff0000000001000000000000000700000000000000",
  "0700000000000000ffffffff010000000100000000000000",
].join("");

test("parseBatch reads the fill triangles and their ranges the Rust side writes", () => {
  const b = parseBatch(fromHex(FILLS_FROM_RUST));
  expect(b.segments.length).toBe(4);
  expect(b.fills.length).toBe(2);
  expect(b.fills.at(0)).toEqual({ corners: [-1, -1, 1, -1, 1, 1], color: 0x00ff00 });
  expect(b.fills.at(1)).toEqual({ corners: [-1, -1, 1, 1, -1, 1], color: FILL_BACKGROUND });
  expect(b.fillRanges.length).toBe(2);
  expect([b.fillRanges.at(0), b.fillRanges.at(1)]).toEqual([
    { top: 6n, entity: 6n, chain: NO_CHAIN, first: 0, count: 1, flags: 0 },
    { top: 7n, entity: 7n, chain: NO_CHAIN, first: 1, count: 1, flags: 0 },
  ]);
  // A batch without them has none.
  const plain = parseBatch(fromHex(FROM_RUST));
  expect([plain.fills.length, plain.fillRanges.length]).toEqual([0, 0]);
  expect(PATTERN).toBe(2);
});

test("parseBatch rejects fill ranges that do not cover the triangles", () => {
  const buf = fromHex(FILLS_FROM_RUST);
  const v = new DataView(buf);
  // Section table entry 8 is the fill ranges (kind 13); the second range's count sits at +32+24.
  const entry = 60 + 12 * 8;
  expect(v.getUint32(entry, true)).toBe(13);
  v.setUint32(v.getUint32(entry + 4, true) + 32 + 24, 2, true);
  expect(() => parseBatch(buf)).toThrow("section 13 too short");
});

/** Ranges of `[entity, first, count]` records. */
function rangesOf(records: [number, number, number][]): Ranges {
  const buf = new ArrayBuffer(32 * records.length);
  const wide = new BigUint64Array(buf);
  const narrow = new Uint32Array(buf);
  records.forEach(([entity, first, count], i) => {
    wide[4 * i] = BigInt(entity);
    wide[4 * i + 1] = BigInt(entity);
    narrow[8 * i + 4] = NO_CHAIN;
    narrow[8 * i + 5] = first;
    narrow[8 * i + 6] = count;
  });
  return new Ranges(wide, narrow);
}

/** Fill triangles with these colours. */
function fillsOf(colors: number[]): Fills {
  const buf = new ArrayBuffer(28 * colors.length);
  const u32 = new Uint32Array(buf);
  colors.forEach((c, i) => (u32[7 * i + 6] = c));
  return new Fills(new Float32Array(buf), u32);
}

test("wipeout frames are drawn apart, after the lines the wipeouts hide", () => {
  // Lines of 5 (0-2), the wipeout 6 (frame 3-6), lines of 7 (7-9); 6 fills with the background,
  // 5 with a colour.
  const b = {
    segments: new Float32Array(4 * 10),
    ranges: rangesOf([
      [5, 0, 3],
      [6, 3, 4],
      [7, 7, 3],
    ]),
    fills: fillsOf([0xff0000, FILL_BACKGROUND, FILL_BACKGROUND]),
    fillRanges: rangesOf([
      [5, 0, 1],
      [6, 1, 2],
    ]),
  } as unknown as Batch;
  expect(wipeoutSpans(b)).toEqual({
    lines: [
      [0, 3],
      [7, 3],
    ],
    frames: [[3, 4]],
  });
  // No wipeout: every line in one span.
  const plain = { ...b, fills: fillsOf([0xff0000, 0xff0000, 0xff0000]) } as Batch;
  expect(wipeoutSpans(plain)).toEqual({ lines: [[0, 10]], frames: [] });
});
