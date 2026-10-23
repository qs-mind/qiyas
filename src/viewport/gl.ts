/**
 * WebGL2 renderer for a `Batch`: its underlay pictures (one RGBA texture per picture id), then its
 * fill triangles, then its line segments (pattern lines among them), then the wipeouts in the
 * background colour over all of those and the wipeouts' frames, then its text as glyph quads
 * textured from the glyph atlas (one R8 texture per page; the texel is the glyph's coverage, drawn
 * as alpha). Wipeouts hide whatever lines are under them, drawn before or after them (there is no
 * draw order yet).
 *
 * Precision: drawings can sit far from (0, 0) (survey grids in metres or millimetres), where f32
 * keeps only a few useful digits. So no absolute coordinate ever reaches the GPU. The segments arrive
 * relative to `batch.origin` and are uploaded once as f32. On each draw the view transform is built
 * in f64 on the JS side relative to that same origin (camera centre minus origin), and only that
 * small offset is passed to the shader as f32.
 */
import {
  FILL_BACKGROUND,
  imageUniforms,
  wipeoutSpans,
  matchingRanges,
  TEXT_BOX,
  type AtlasPage,
  type Batch,
  type Picture,
  type Target,
} from "./batch";
import type { PaperView, Ucs } from "../api";
import type { Camera } from "./camera";
import { GRID_MAJOR, gridLevels, offsetInPeriod } from "./grid";
import type { Palette } from "./colours";
import { scissorBox, viewCamera, viewSpans } from "./inset";

export type { Theme } from "./colours";

export type Renderer = {
  /** `null` clears the drawing. A batch whose `segments` are the very array of the one before
   *  (only its glyphs drawn again for another zoom) keeps the lines already uploaded. */
  setBatch(b: Batch | null): void;
  /** Uploads atlas pages the textures don't hold yet (by page index; a page with other data is
   *  uploaded again). */
  setAtlas(pages: AtlasPage[]): void;
  /** The underlay pictures by image id; one with other data is uploaded again, one left out is
   *  dropped. */
  setPictures(pictures: Map<number, Picture>): void;
  /** Draws these entities violet (see `Target`). */
  setHighlight(targets: Target[]): void;
  /** Violet segments `x0 y0 x1 y1` in absolute drawing coordinates, drawn on top; `null` for none. */
  setOverlay(segments: Float64Array | null): void;
  /** Draws a dot grid `spacing` drawing units apart under the drawing, along `ucs` (the
   *  world's when left out); `null` for none. */
  setGrid(spacing: number | null, ucs?: Ucs | null): void;
  /** Model space drawn inside a layout's viewports: `batch` through each of `views`, clipped to
   *  its frame (fills, then lines, then text; wipeouts there hide no lines), under the layout's
   *  own drawing. `null` for none. A batch with the same `segments` as the last uploads only its
   *  glyphs. */
  setInset(batch: Batch | null, views: PaperView[]): void;
  /** Draws the view with `look`'s colours (`palette`: the theme's, or the user's). */
  draw(camera: Camera, look: Palette): void;
  /** The canvas's pixels as last drawn, rows from the bottom (WebGL's order). Call it in the same
   *  task as `draw`: the browser clears the canvas once it has shown it. */
  read(): { width: number; height: number; rgba: Uint8Array };
  dispose(): void;
};

const rgb = (hex: number): [number, number, number] => [
  ((hex >> 16) & 255) / 255,
  ((hex >> 8) & 255) / 255,
  (hex & 255) / 255,
];
/** Highlight width in CSS pixels. */
const HIGHLIGHT_PX = 2;

const VERTEX = `#version 300 es
in vec2 a_pos;
in vec4 a_color;
uniform vec2 u_center;  // camera centre minus batch origin
uniform vec2 u_toClip;  // 2 / (canvas size in world units)
uniform vec2 u_offset;  // clip-space nudge, used to thicken the highlight
out vec4 v_color;
void main() {
  gl_Position = vec4((a_pos - u_center) * u_toClip + u_offset, 0.0, 1.0);
  v_color = a_color;
}`;

const FRAGMENT = `#version 300 es
precision mediump float;
in vec4 v_color;
uniform vec4 u_color;  // overrides the vertex colour when alpha > 0
uniform bool u_light;
uniform vec3 u_ink;
uniform vec3 u_paper;
out vec4 outColor;
void main() {
  // A text's box (vertex alpha 0) is only for picking and highlighting.
  if (u_color.a == 0.0 && v_color.a == 0.0) discard;
  // Half alpha: a wipeout drawn inside a viewport, in the background colour.
  if (u_color.a == 0.0 && v_color.a > 0.25 && v_color.a < 0.75) {
    outColor = vec4(u_light ? u_paper : u_ink, 1.0);
    return;
  }
  vec3 c = u_color.a > 0.0 ? u_color.rgb : v_color.rgb;
  // White lines (ACI 7 and friends) would vanish on Paper; draw them in Ink.
  if (u_light && all(greaterThan(c, vec3(0.999)))) c = u_ink;
  // Black lines would vanish on Ink; draw them in Paper.
  if (!u_light && all(lessThan(c, vec3(0.001)))) c = u_paper;
  outColor = vec4(c, 1.0);
}`;

const TEXT_VERTEX = `#version 300 es
in vec2 a_pos;
in vec2 a_uv;
in vec4 a_color;
uniform vec2 u_center;
uniform vec2 u_toClip;
out vec2 v_uv;
out vec4 v_color;
void main() {
  gl_Position = vec4((a_pos - u_center) * u_toClip, 0.0, 1.0);
  v_uv = a_uv;
  v_color = a_color;
}`;

const TEXT_FRAGMENT = `#version 300 es
precision highp float;
in vec2 v_uv;      // texels
in vec4 v_color;
uniform sampler2D u_atlas;
uniform vec4 u_color;  // overrides the vertex colour when alpha > 0
uniform bool u_light;
uniform vec3 u_ink;
uniform vec3 u_paper;
out vec4 outColor;
void main() {
  float coverage = texture(u_atlas, v_uv / vec2(textureSize(u_atlas, 0))).r;
  if (coverage <= 0.0) discard;
  vec3 c = u_color.a > 0.0 ? u_color.rgb : v_color.rgb;
  if (u_light && all(greaterThan(c, vec3(0.999)))) c = u_ink;
  if (!u_light && all(lessThan(c, vec3(0.001)))) c = u_paper;
  outColor = vec4(c, coverage);
}`;

const IMAGE_VERTEX = `#version 300 es
in vec2 a_pos;
in vec2 a_uv;
uniform vec2 u_center;
uniform vec2 u_toClip;
out vec2 v_uv;
void main() {
  gl_Position = vec4((a_pos - u_center) * u_toClip, 0.0, 1.0);
  v_uv = a_uv;
}`;

/** A PDF page or an image under the drawing (uniforms from `imageUniforms`). Grey when
 *  monochrome; then contrast about mid grey and brightness added; on Ink its lightness is turned
 *  over (white paper goes dark, black lines go light, colours keep their hue); then mixed into the
 *  background by the fade. */
const IMAGE_FRAGMENT = `#version 300 es
precision mediump float;
in vec2 v_uv;
uniform sampler2D u_image;
uniform bool u_mono;
uniform float u_brightness;
uniform float u_contrast;
uniform float u_fade;
uniform bool u_light;
uniform vec3 u_background;
out vec4 outColor;
void main() {
  vec3 c = texture(u_image, v_uv).rgb;
  if (u_mono) c = vec3(dot(c, vec3(0.299, 0.587, 0.114)));
  c = clamp((c - 0.5) * u_contrast + 0.5 + u_brightness, 0.0, 1.0);
  float luma = dot(c, vec3(0.299, 0.587, 0.114));
  if (!u_light) c = clamp(c + 1.0 - 2.0 * luma, 0.0, 1.0);
  c = mix(c, u_background, u_fade);
  outColor = vec4(c, 1.0);
}`;

/** One triangle over the whole canvas; the grid's dots are worked out per pixel. */
const GRID_VERTEX = `#version 300 es
void main() {
  vec2 corner = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  gl_Position = vec4(corner * 2.0 - 1.0, 0.0, 1.0);
}`;

const GRID_FRAGMENT = `#version 300 es
precision highp float;
uniform vec2 u_half;          // half the canvas, device pixels
uniform float u_worldPerPx;   // drawing units per device pixel
uniform vec2 u_offset;        // camera centre less the nearest major grid point
uniform float u_spacing;
uniform float u_major;        // u_spacing * 10
uniform bool u_minor;         // minor dots far enough apart to draw
uniform float u_dot;          // half a minor dot, device pixels
uniform vec4 u_color;
uniform vec2 u_axis;          // the UCS x axis (cos, sin): the grid turns with it
out vec4 outColor;
void main() {
  vec2 d = (gl_FragCoord.xy - u_half) * u_worldPerPx;
  vec2 w = u_offset + vec2(d.x * u_axis.x + d.y * u_axis.y, -d.x * u_axis.y + d.y * u_axis.x);
  vec2 toMajor = abs(w - u_major * floor(w / u_major + 0.5)) / u_worldPerPx;
  if (max(toMajor.x, toMajor.y) <= 1.5 * u_dot) {
    outColor = u_color;
    return;
  }
  vec2 toMinor = abs(w - u_spacing * floor(w / u_spacing + 0.5)) / u_worldPerPx;
  if (u_minor && max(toMinor.x, toMinor.y) <= u_dot) {
    outColor = vec4(u_color.rgb, u_color.a * 0.6);
    return;
  }
  discard;
}`;

/** Grid dots: Slate on Paper, Stone on Ink, half seen. */
const GRID_LIGHT: [number, number, number, number] = [...rgb(0x6e6a64), 0.55];
const GRID_DARK: [number, number, number, number] = [...rgb(0xf2eee7), 0.35];

function compile(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader {
  const shader = gl.createShader(type)!;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    throw new Error(`Shader did not compile: ${gl.getShaderInfoLog(shader)}`);
  }
  return shader;
}

function link(gl: WebGL2RenderingContext, vertex: string, fragment: string): WebGLProgram {
  const program = gl.createProgram();
  const vs = compile(gl, gl.VERTEX_SHADER, vertex);
  const fs = compile(gl, gl.FRAGMENT_SHADER, fragment);
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new Error(`Shader program did not link: ${gl.getProgramInfoLog(program)}`);
  }
  gl.deleteShader(vs);
  gl.deleteShader(fs);
  return program;
}

/** Element indices of the quads `[first, first + count)` per atlas page, two triangles each. */
function quadIndices(b: Batch, spans: Iterable<[number, number]>): Map<number, number[]> {
  const byPage = new Map<number, number[]>();
  for (const [first, count] of spans) {
    for (let i = first; i < first + count; i++) {
      const page = b.quads.page(i);
      let list = byPage.get(page);
      if (!list) byPage.set(page, (list = []));
      const v = 4 * i;
      list.push(v, v + 1, v + 2, v, v + 2, v + 3);
    }
  }
  return byPage;
}

/** One index buffer holding the pages' lists back to back, and where each page's list starts. */
type PagedIndices = { pages: { page: number; offset: number; count: number }[] };

function uploadPaged(gl: WebGL2RenderingContext, byPage: Map<number, number[]>): PagedIndices {
  const all: number[] = [];
  const pages = [];
  for (const [page, list] of byPage) {
    pages.push({ page, offset: 4 * all.length, count: list.length });
    for (const i of list) all.push(i);
  }
  gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, new Uint32Array(all), gl.STATIC_DRAW);
  return { pages };
}

/** One RGBA byte quad per segment end; both ends of a segment share its colour. A text's box
 *  gets alpha 0: it is drawn only when highlighted. */
function lineColors(b: Batch | null): Uint8Array {
  const segmentColors = b?.colors ?? new Uint32Array();
  const n = segmentColors.length;
  const rgba = new Uint8Array(8 * n);
  for (let i = 0; i < n; i++) {
    const c = segmentColors[i];
    for (const at of [8 * i, 8 * i + 4]) {
      rgba[at] = (c >> 16) & 255;
      rgba[at + 1] = (c >> 8) & 255;
      rgba[at + 2] = c & 255;
      rgba[at + 3] = 255;
    }
  }
  if (b) {
    const { ranges } = b;
    for (let r = 0; r < ranges.length; r++) {
      if (!(ranges.flags(r) & TEXT_BOX)) continue;
      const first = ranges.first(r);
      for (let i = first; i < first + ranges.count(r); i++) {
        rgba[8 * i + 3] = 0;
        rgba[8 * i + 7] = 0;
      }
    }
  }
  return rgba;
}

/** The glyph quads' vertices (x y u v each) and colours, four vertices a quad. */
function textData(b: Batch | null): { vertices: Float32Array; rgba: Uint8Array } {
  const n = b?.quads.length ?? 0;
  const vertices = new Float32Array(16 * n);
  const rgba = new Uint8Array(16 * n);
  if (b) {
    const q = b.quads;
    // Texel corners in the order of the position corners: top-left, top-right, bottom-right,
    // bottom-left.
    const corners = [
      [0, 1],
      [2, 1],
      [2, 3],
      [0, 3],
    ];
    for (let i = 0; i < n; i++) {
      const c = q.color(i);
      for (let k = 0; k < 4; k++) {
        const at = 16 * i + 4 * k;
        vertices[at] = q.x(i, k);
        vertices[at + 1] = q.y(i, k);
        vertices[at + 2] = q.uv(i, corners[k][0]);
        vertices[at + 3] = q.uv(i, corners[k][1]);
        rgba[at] = (c >> 16) & 255;
        rgba[at + 1] = (c >> 8) & 255;
        rgba[at + 2] = c & 255;
        rgba[at + 3] = 255;
      }
    }
  }
  return { vertices, rgba };
}

export function createRenderer(canvas: HTMLCanvasElement): Renderer {
  const context = canvas.getContext("webgl2", { antialias: true });
  if (!context) throw new Error("WebGL2 is not available");
  const gl = context;

  const program = link(gl, VERTEX, FRAGMENT);
  const u = (name: string) => gl.getUniformLocation(program, name);
  const uCenter = u("u_center");
  const uToClip = u("u_toClip");
  const uOffset = u("u_offset");
  const uColor = u("u_color");
  const uLight = u("u_light");
  const uInk = u("u_ink");
  const uPaper = u("u_paper");

  const gridProgram = link(gl, GRID_VERTEX, GRID_FRAGMENT);
  const gu = (name: string) => gl.getUniformLocation(gridProgram, name);
  const gHalf = gu("u_half");
  const gWorldPerPx = gu("u_worldPerPx");
  const gOffset = gu("u_offset");
  const gSpacing = gu("u_spacing");
  const gMajor = gu("u_major");
  const gMinor = gu("u_minor");
  const gDot = gu("u_dot");
  const gColor = gu("u_color");
  const gAxis = gu("u_axis");
  const gridVao = gl.createVertexArray();
  let grid: number | null = null;
  let gridUcs: Ucs | null = null;

  const vao = gl.createVertexArray();
  const positions = gl.createBuffer();
  const colors = gl.createBuffer();
  const highlightIndices = gl.createBuffer();
  gl.bindVertexArray(vao);
  gl.bindBuffer(gl.ARRAY_BUFFER, positions);
  const aPos = gl.getAttribLocation(program, "a_pos");
  gl.enableVertexAttribArray(aPos);
  gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);
  gl.bindBuffer(gl.ARRAY_BUFFER, colors);
  const aColor = gl.getAttribLocation(program, "a_color");
  gl.enableVertexAttribArray(aColor);
  gl.vertexAttribPointer(aColor, 4, gl.UNSIGNED_BYTE, true, 0, 0);
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, highlightIndices);
  gl.bindVertexArray(null);

  // Fill triangles with their colours, and the background ones (positions only, u_color gives the
  // background), with the line program.
  const fillVao = gl.createVertexArray();
  const fillPositions = gl.createBuffer();
  const fillColors = gl.createBuffer();
  gl.bindVertexArray(fillVao);
  gl.bindBuffer(gl.ARRAY_BUFFER, fillPositions);
  gl.enableVertexAttribArray(aPos);
  gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);
  gl.bindBuffer(gl.ARRAY_BUFFER, fillColors);
  gl.enableVertexAttribArray(aColor);
  gl.vertexAttribPointer(aColor, 4, gl.UNSIGNED_BYTE, true, 0, 0);
  gl.bindVertexArray(null);
  const backgroundVao = gl.createVertexArray();
  const backgroundPositions = gl.createBuffer();
  gl.bindVertexArray(backgroundVao);
  gl.bindBuffer(gl.ARRAY_BUFFER, backgroundPositions);
  gl.enableVertexAttribArray(aPos);
  gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);
  gl.bindVertexArray(null);
  let fillCount = 0;
  let backgroundCount = 0;

  // The overlay has positions only; u_color gives its colour.
  const overlayVao = gl.createVertexArray();
  const overlayPositions = gl.createBuffer();
  gl.bindVertexArray(overlayVao);
  gl.bindBuffer(gl.ARRAY_BUFFER, overlayPositions);
  gl.enableVertexAttribArray(aPos);
  gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);
  gl.bindVertexArray(null);

  // Glyph quads: positions and texels interleaved (x y u v per vertex), colours, and index buffers
  // per atlas page for all quads and for the highlighted ones.
  const textProgram = link(gl, TEXT_VERTEX, TEXT_FRAGMENT);
  const tu = (name: string) => gl.getUniformLocation(textProgram, name);
  const tCenter = tu("u_center");
  const tToClip = tu("u_toClip");
  const tColor = tu("u_color");
  const tLight = tu("u_light");
  const tInk = tu("u_ink");
  const tPaper = tu("u_paper");
  const tAtlas = tu("u_atlas");
  const textVao = gl.createVertexArray();
  const textVertices = gl.createBuffer();
  const textColors = gl.createBuffer();
  const textIndices = gl.createBuffer();
  const textHighlightIndices = gl.createBuffer();
  gl.bindVertexArray(textVao);
  gl.bindBuffer(gl.ARRAY_BUFFER, textVertices);
  const taPos = gl.getAttribLocation(textProgram, "a_pos");
  const taUv = gl.getAttribLocation(textProgram, "a_uv");
  gl.enableVertexAttribArray(taPos);
  gl.vertexAttribPointer(taPos, 2, gl.FLOAT, false, 16, 0);
  gl.enableVertexAttribArray(taUv);
  gl.vertexAttribPointer(taUv, 2, gl.FLOAT, false, 16, 8);
  gl.bindBuffer(gl.ARRAY_BUFFER, textColors);
  const taColor = gl.getAttribLocation(textProgram, "a_color");
  gl.enableVertexAttribArray(taColor);
  gl.vertexAttribPointer(taColor, 4, gl.UNSIGNED_BYTE, true, 0, 0);
  gl.bindVertexArray(null);
  let textAll: PagedIndices = { pages: [] };
  let textHighlight: PagedIndices = { pages: [] };
  /** Atlas page index to its texture and the bytes uploaded into it. */
  const textures = new Map<number, { texture: WebGLTexture; data: Uint8Array }>();

  // Underlay pictures: positions and texels interleaved (x y u v per vertex), four vertices and
  // six indices per quad, drawn a quad at a time with its picture's texture.
  const imageProgram = link(gl, IMAGE_VERTEX, IMAGE_FRAGMENT);
  const iu = (name: string) => gl.getUniformLocation(imageProgram, name);
  const iCenter = iu("u_center");
  const iToClip = iu("u_toClip");
  const iImage = iu("u_image");
  const iMono = iu("u_mono");
  const iBrightness = iu("u_brightness");
  const iContrast = iu("u_contrast");
  const iFade = iu("u_fade");
  const iLight = iu("u_light");
  const iBackground = iu("u_background");
  const imageVao = gl.createVertexArray();
  const imageVertices = gl.createBuffer();
  const imageIndices = gl.createBuffer();
  gl.bindVertexArray(imageVao);
  gl.bindBuffer(gl.ARRAY_BUFFER, imageVertices);
  const iaPos = gl.getAttribLocation(imageProgram, "a_pos");
  const iaUv = gl.getAttribLocation(imageProgram, "a_uv");
  gl.enableVertexAttribArray(iaPos);
  gl.vertexAttribPointer(iaPos, 2, gl.FLOAT, false, 16, 0);
  gl.enableVertexAttribArray(iaUv);
  gl.vertexAttribPointer(iaUv, 2, gl.FLOAT, false, 16, 8);
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, imageIndices);
  gl.bindVertexArray(null);
  /** Picture id to its texture and the bytes uploaded into it. */
  const pictures = new Map<number, { texture: WebGLTexture; data: Uint8Array }>();

  // Model space inside a layout's viewports (`setInset`): lines, fills and text of one batch,
  // and per view the spans it draws and the index buffer of its glyph quads.
  const insetVao = gl.createVertexArray();
  const insetPositions = gl.createBuffer();
  const insetColors = gl.createBuffer();
  const insetFillVao = gl.createVertexArray();
  const insetFillPositions = gl.createBuffer();
  const insetFillColors = gl.createBuffer();
  for (const [v, xy, rgba] of [
    [insetVao, insetPositions, insetColors],
    [insetFillVao, insetFillPositions, insetFillColors],
  ] as const) {
    gl.bindVertexArray(v);
    gl.bindBuffer(gl.ARRAY_BUFFER, xy);
    gl.enableVertexAttribArray(aPos);
    gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);
    gl.bindBuffer(gl.ARRAY_BUFFER, rgba);
    gl.enableVertexAttribArray(aColor);
    gl.vertexAttribPointer(aColor, 4, gl.UNSIGNED_BYTE, true, 0, 0);
  }
  const insetTextVao = gl.createVertexArray();
  const insetTextVertices = gl.createBuffer();
  const insetTextColors = gl.createBuffer();
  gl.bindVertexArray(insetTextVao);
  gl.bindBuffer(gl.ARRAY_BUFFER, insetTextVertices);
  gl.enableVertexAttribArray(taPos);
  gl.vertexAttribPointer(taPos, 2, gl.FLOAT, false, 16, 0);
  gl.enableVertexAttribArray(taUv);
  gl.vertexAttribPointer(taUv, 2, gl.FLOAT, false, 16, 8);
  gl.bindBuffer(gl.ARRAY_BUFFER, insetTextColors);
  gl.enableVertexAttribArray(taColor);
  gl.vertexAttribPointer(taColor, 4, gl.UNSIGNED_BYTE, true, 0, 0);
  gl.bindVertexArray(null);
  let inset: Batch | null = null;
  let insetViews: {
    view: PaperView;
    lines: [number, number][];
    fills: [number, number][];
    text: PagedIndices;
    buffer: WebGLBuffer;
  }[] = [];

  let batch: Batch | null = null;
  // The segments drawn under the wipeouts and the wipeouts' frames drawn over them.
  let spans: ReturnType<typeof wipeoutSpans> = { lines: [], frames: [] };
  let highlight: Target[] = [];
  let highlightCount = 0;
  let overlay: Float64Array | null = null;

  function uploadHighlight() {
    const indices: number[] = [];
    if (batch) {
      for (const [first, count] of matchingRanges(batch, highlight)) {
        for (let v = 2 * first; v < 2 * (first + count); v++) indices.push(v);
      }
    }
    highlightCount = indices.length;
    gl.bindVertexArray(vao);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, new Uint32Array(indices), gl.STATIC_DRAW);
    gl.bindVertexArray(textVao);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, textHighlightIndices);
    textHighlight = batch
      ? uploadPaged(gl, quadIndices(batch, matchingRanges(batch, highlight, batch.textRanges)))
      : { pages: [] };
    gl.bindVertexArray(null);
  }

  function uploadLines(b: Batch | null) {
    spans = b ? wipeoutSpans(b) : { lines: [], frames: [] };
    gl.bindBuffer(gl.ARRAY_BUFFER, positions);
    gl.bufferData(gl.ARRAY_BUFFER, b?.segments ?? new Float32Array(), gl.STATIC_DRAW);
    gl.bindBuffer(gl.ARRAY_BUFFER, colors);
    gl.bufferData(gl.ARRAY_BUFFER, lineColors(b), gl.STATIC_DRAW);
  }

  /** The fill triangles: coloured ones and background ones apart, three vertices each. */
  function uploadFills(b: Batch | null) {
    const n = b?.fills.length ?? 0;
    let background = 0;
    for (let i = 0; i < n; i++) if (b!.fills.color(i) & FILL_BACKGROUND) background++;
    const colored = new Float32Array(6 * (n - background));
    const rgba = new Uint8Array(12 * (n - background));
    const behind = new Float32Array(6 * background);
    let [c, w] = [0, 0];
    for (let i = 0; i < n; i++) {
      const f = b!.fills;
      const color = f.color(i);
      const target = color & FILL_BACKGROUND ? behind : colored;
      const at = 6 * (color & FILL_BACKGROUND ? w++ : c);
      for (let k = 0; k < 3; k++) {
        target[at + 2 * k] = f.x(i, k);
        target[at + 2 * k + 1] = f.y(i, k);
      }
      if (color & FILL_BACKGROUND) continue;
      for (let k = 0; k < 3; k++) {
        const o = 12 * c + 4 * k;
        rgba[o] = (color >> 16) & 255;
        rgba[o + 1] = (color >> 8) & 255;
        rgba[o + 2] = color & 255;
        rgba[o + 3] = 255;
      }
      c++;
    }
    fillCount = 3 * c;
    backgroundCount = 3 * w;
    gl.bindBuffer(gl.ARRAY_BUFFER, fillPositions);
    gl.bufferData(gl.ARRAY_BUFFER, colored, gl.STATIC_DRAW);
    gl.bindBuffer(gl.ARRAY_BUFFER, fillColors);
    gl.bufferData(gl.ARRAY_BUFFER, rgba, gl.STATIC_DRAW);
    gl.bindBuffer(gl.ARRAY_BUFFER, backgroundPositions);
    gl.bufferData(gl.ARRAY_BUFFER, behind, gl.STATIC_DRAW);
  }

  function uploadText(b: Batch | null) {
    const n = b?.quads.length ?? 0;
    const { vertices, rgba } = textData(b);
    gl.bindVertexArray(textVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, textVertices);
    gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);
    gl.bindBuffer(gl.ARRAY_BUFFER, textColors);
    gl.bufferData(gl.ARRAY_BUFFER, rgba, gl.STATIC_DRAW);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, textIndices);
    textAll = b ? uploadPaged(gl, quadIndices(b, [[0, n]])) : { pages: [] };
    gl.bindVertexArray(null);
  }

  /** Model space for the viewports: its lines, its fills in triangle order (the background ones
   *  at half alpha, see the fragment shader) and its glyph quads. */
  function uploadInset(b: Batch) {
    gl.bindBuffer(gl.ARRAY_BUFFER, insetPositions);
    gl.bufferData(gl.ARRAY_BUFFER, b.segments, gl.STATIC_DRAW);
    gl.bindBuffer(gl.ARRAY_BUFFER, insetColors);
    gl.bufferData(gl.ARRAY_BUFFER, lineColors(b), gl.STATIC_DRAW);
    const n = b.fills.length;
    const xy = new Float32Array(6 * n);
    const rgba = new Uint8Array(12 * n);
    for (let i = 0; i < n; i++) {
      const color = b.fills.color(i);
      for (let k = 0; k < 3; k++) {
        xy[6 * i + 2 * k] = b.fills.x(i, k);
        xy[6 * i + 2 * k + 1] = b.fills.y(i, k);
        const o = 12 * i + 4 * k;
        rgba[o] = (color >> 16) & 255;
        rgba[o + 1] = (color >> 8) & 255;
        rgba[o + 2] = color & 255;
        rgba[o + 3] = color & FILL_BACKGROUND ? 128 : 255;
      }
    }
    gl.bindBuffer(gl.ARRAY_BUFFER, insetFillPositions);
    gl.bufferData(gl.ARRAY_BUFFER, xy, gl.STATIC_DRAW);
    gl.bindBuffer(gl.ARRAY_BUFFER, insetFillColors);
    gl.bufferData(gl.ARRAY_BUFFER, rgba, gl.STATIC_DRAW);
    uploadInsetText(b);
  }

  /** Model space's glyph quads for the viewports (again alone when only the glyphs changed). */
  function uploadInsetText(b: Batch) {
    const text = textData(b);
    gl.bindBuffer(gl.ARRAY_BUFFER, insetTextVertices);
    gl.bufferData(gl.ARRAY_BUFFER, text.vertices, gl.STATIC_DRAW);
    gl.bindBuffer(gl.ARRAY_BUFFER, insetTextColors);
    gl.bufferData(gl.ARRAY_BUFFER, text.rgba, gl.STATIC_DRAW);
  }

  function uploadImages(b: Batch | null) {
    const n = b?.images.length ?? 0;
    const vertices = new Float32Array(16 * n);
    const indices = new Uint32Array(6 * n);
    // Texel corners in the order of the position corners: top-left, top-right, bottom-right,
    // bottom-left; the picture's rows run from the top.
    const uv = [0, 0, 1, 0, 1, 1, 0, 1];
    for (let i = 0; i < n; i++) {
      for (let k = 0; k < 4; k++) {
        const at = 16 * i + 4 * k;
        vertices[at] = b!.images.x(i, k);
        vertices[at + 1] = b!.images.y(i, k);
        vertices[at + 2] = uv[2 * k];
        vertices[at + 3] = uv[2 * k + 1];
      }
      const v = 4 * i;
      indices.set([v, v + 1, v + 2, v, v + 2, v + 3], 6 * i);
    }
    gl.bindVertexArray(imageVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, imageVertices);
    gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW);
    gl.bindVertexArray(null);
  }

  /** Draws the quads of `indices` from the bound text VAO, a page texture at a time. */
  function drawText(indices: PagedIndices, buffer: WebGLBuffer) {
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, buffer);
    for (const { page, offset, count } of indices.pages) {
      const texture = textures.get(page);
      if (!texture || count === 0) continue;
      gl.bindTexture(gl.TEXTURE_2D, texture.texture);
      gl.drawElements(gl.TRIANGLES, count, gl.UNSIGNED_INT, offset);
    }
  }

  return {
    setBatch(b) {
      const sameLines = !!b && !!batch && b.segments === batch.segments;
      batch = b;
      if (!sameLines) {
        uploadLines(b);
        uploadFills(b);
      }
      uploadText(b);
      uploadImages(b);
      uploadHighlight();
    },

    setAtlas(pages) {
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
      for (const page of pages) {
        let held = textures.get(page.index);
        if (held?.data === page.data) continue;
        if (!held) {
          held = { texture: gl.createTexture(), data: page.data };
          textures.set(page.index, held);
        }
        held.data = page.data;
        gl.bindTexture(gl.TEXTURE_2D, held.texture);
        gl.texImage2D(
          gl.TEXTURE_2D,
          0,
          gl.R8,
          page.width,
          page.height,
          0,
          gl.RED,
          gl.UNSIGNED_BYTE,
          page.data,
        );
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      }
      gl.bindTexture(gl.TEXTURE_2D, null);
    },

    setPictures(next) {
      for (const [id, held] of pictures) {
        if (!next.has(id)) {
          gl.deleteTexture(held.texture);
          pictures.delete(id);
        }
      }
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
      for (const [id, picture] of next) {
        let held = pictures.get(id);
        if (held?.data === picture.data) continue;
        if (!held) {
          held = { texture: gl.createTexture(), data: picture.data };
          pictures.set(id, held);
        }
        held.data = picture.data;
        gl.bindTexture(gl.TEXTURE_2D, held.texture);
        gl.texImage2D(
          gl.TEXTURE_2D,
          0,
          gl.RGBA8,
          picture.width,
          picture.height,
          0,
          gl.RGBA,
          gl.UNSIGNED_BYTE,
          picture.data,
        );
        // Mipmaps: a page seen small keeps its thin lines instead of shimmering.
        gl.generateMipmap(gl.TEXTURE_2D);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      }
      gl.bindTexture(gl.TEXTURE_2D, null);
    },

    setHighlight(targets) {
      highlight = targets;
      uploadHighlight();
    },

    setOverlay(segments) {
      overlay = segments;
    },

    setGrid(spacing, ucs) {
      grid = spacing !== null && spacing > 0 ? spacing : null;
      gridUcs = ucs ?? null;
    },

    setInset(b, views) {
      if (!b || views.length === 0) {
        for (const v of insetViews) gl.deleteBuffer(v.buffer);
        insetViews = [];
        inset = null;
        return;
      }
      // The same lines (only the glyphs drawn again for another zoom): the text alone.
      if (b.segments !== inset?.segments) uploadInset(b);
      else if (b.quads !== inset?.quads) uploadInsetText(b);
      inset = b;
      for (const v of insetViews) gl.deleteBuffer(v.buffer);
      gl.bindVertexArray(insetTextVao);
      insetViews = views.map((view) => {
        const buffer = gl.createBuffer();
        const spans = viewSpans(view, b);
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, buffer);
        const text = uploadPaged(gl, quadIndices(b, spans.texts));
        return { view, lines: spans.lines, fills: spans.fills, text, buffer };
      });
      gl.bindVertexArray(null);
    },

    draw(camera, look) {
      const { light, ink, paper, background, highlight: violet } = look;
      gl.viewport(0, 0, canvas.width, canvas.height);
      gl.clearColor(...background, 1);
      gl.clear(gl.COLOR_BUFFER_BIT);

      // The grid under the drawing, its dots hidden when they would crowd together.
      const levels = grid === null ? null : gridLevels(grid, camera.scale);
      if (grid !== null && levels?.major) {
        const dprGrid = canvas.width / camera.width;
        const major = grid * GRID_MAJOR;
        gl.useProgram(gridProgram);
        gl.bindVertexArray(gridVao);
        gl.uniform2f(gHalf, canvas.width / 2, canvas.height / 2);
        gl.uniform1f(gWorldPerPx, camera.scale / dprGrid);
        // The camera's centre in the UCS, so the dots sit on its axes.
        const [ox, oy] = gridUcs?.origin ?? [0, 0];
        const [ax, ay] = gridUcs?.x_axis ?? [1, 0];
        const [dx, dy] = [camera.cx - ox, camera.cy - oy];
        const [ux, uy] = [dx * ax + dy * ay, -dx * ay + dy * ax];
        gl.uniform2f(gOffset, offsetInPeriod(ux, major), offsetInPeriod(uy, major));
        gl.uniform2f(gAxis, ax, ay);
        gl.uniform1f(gSpacing, grid);
        gl.uniform1f(gMajor, major);
        gl.uniform1i(gMinor, levels.minor ? 1 : 0);
        gl.uniform1f(gDot, Math.max(0.75, 0.75 * dprGrid));
        gl.uniform4f(gColor, ...(light ? GRID_LIGHT : GRID_DARK));
        gl.enable(gl.BLEND);
        gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
        gl.drawArrays(gl.TRIANGLES, 0, 3);
        gl.disable(gl.BLEND);
        gl.bindVertexArray(null);
      }
      if (!batch) return;

      // Model space inside each viewport of the layout first, through its own camera, clipped to
      // its frame: the layout's own pictures, fills, lines and text (labels, title blocks) draw
      // over it, each pass setting the layout's camera again.
      if (inset && insetViews.length > 0) {
        const m = inset;
        gl.enable(gl.SCISSOR_TEST);
        for (const v of insetViews) {
          const box = scissorBox(camera, v.view, canvas.width, canvas.height);
          if (!box) continue;
          gl.scissor(...box);
          const mc = viewCamera(camera, v.view);
          const toClip: [number, number] = [2 / (mc.width * mc.scale), 2 / (mc.height * mc.scale)];
          gl.useProgram(program);
          gl.uniform2f(uCenter, mc.cx - m.origin[0], mc.cy - m.origin[1]);
          gl.uniform2f(uToClip, ...toClip);
          gl.uniform4f(uColor, 0, 0, 0, 0);
          gl.uniform2f(uOffset, 0, 0);
          gl.uniform1i(uLight, light ? 1 : 0);
          gl.uniform3f(uInk, ...ink);
          gl.uniform3f(uPaper, ...paper);
          gl.bindVertexArray(insetFillVao);
          for (const [first, count] of v.fills) gl.drawArrays(gl.TRIANGLES, 3 * first, 3 * count);
          gl.bindVertexArray(insetVao);
          for (const [first, count] of v.lines) gl.drawArrays(gl.LINES, 2 * first, 2 * count);
          if (v.text.pages.length > 0) {
            gl.useProgram(textProgram);
            gl.bindVertexArray(insetTextVao);
            gl.uniform2f(tCenter, mc.cx - m.origin[0], mc.cy - m.origin[1]);
            gl.uniform2f(tToClip, ...toClip);
            gl.uniform1i(tLight, light ? 1 : 0);
            gl.uniform3f(tInk, ...ink);
            gl.uniform3f(tPaper, ...paper);
            gl.uniform4f(tColor, 0, 0, 0, 0);
            gl.uniform1i(tAtlas, 0);
            gl.activeTexture(gl.TEXTURE0);
            gl.enable(gl.BLEND);
            gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
            drawText(v.text, v.buffer);
            gl.disable(gl.BLEND);
          }
        }
        gl.disable(gl.SCISSOR_TEST);
        gl.bindVertexArray(null);
      }

      // Underlay pictures under everything else, a quad at a time.
      if (batch.images.length > 0) {
        gl.useProgram(imageProgram);
        gl.bindVertexArray(imageVao);
        gl.uniform2f(iCenter, camera.cx - batch.origin[0], camera.cy - batch.origin[1]);
        gl.uniform2f(iToClip, 2 / (camera.width * camera.scale), 2 / (camera.height * camera.scale));
        gl.uniform1i(iLight, light ? 1 : 0);
        gl.uniform3f(iBackground, ...background);
        gl.uniform1i(iImage, 0);
        gl.activeTexture(gl.TEXTURE0);
        for (let i = 0; i < batch.images.length; i++) {
          const picture = pictures.get(batch.images.id(i));
          if (!picture) continue;
          gl.bindTexture(gl.TEXTURE_2D, picture.texture);
          const look = imageUniforms(batch.images.flags(i));
          gl.uniform1i(iMono, look.monochrome ? 1 : 0);
          gl.uniform1f(iBrightness, look.brightness);
          gl.uniform1f(iContrast, look.contrast);
          gl.uniform1f(iFade, look.fade);
          gl.drawElements(gl.TRIANGLES, 6, gl.UNSIGNED_INT, 24 * i);
        }
        gl.bindVertexArray(null);
      }

      gl.useProgram(program);
      gl.bindVertexArray(vao);
      gl.uniform2f(uCenter, camera.cx - batch.origin[0], camera.cy - batch.origin[1]);
      gl.uniform2f(uToClip, 2 / (camera.width * camera.scale), 2 / (camera.height * camera.scale));
      gl.uniform1i(uLight, light ? 1 : 0);
      gl.uniform3f(uInk, ...ink);
      gl.uniform3f(uPaper, ...paper);
      gl.uniform2f(uOffset, 0, 0);
      // Fills under the lines; wipeouts over both in the background colour, their frames last.
      gl.uniform4f(uColor, 0, 0, 0, 0);
      if (fillCount > 0) {
        gl.bindVertexArray(fillVao);
        gl.drawArrays(gl.TRIANGLES, 0, fillCount);
      }
      gl.bindVertexArray(vao);
      for (const [first, count] of spans.lines) gl.drawArrays(gl.LINES, 2 * first, 2 * count);
      if (backgroundCount > 0) {
        gl.bindVertexArray(backgroundVao);
        gl.uniform4f(uColor, ...background, 1);
        gl.drawArrays(gl.TRIANGLES, 0, backgroundCount);
        gl.uniform4f(uColor, 0, 0, 0, 0);
        gl.bindVertexArray(vao);
      }
      for (const [first, count] of spans.frames) gl.drawArrays(gl.LINES, 2 * first, 2 * count);

      // The text over the lines, its coverage blended onto them.
      const text = (indices: PagedIndices, buffer: WebGLBuffer, color: number[] | null) => {
        if (indices.pages.length === 0) return;
        gl.useProgram(textProgram);
        gl.bindVertexArray(textVao);
        gl.uniform2f(tCenter, camera.cx - batch!.origin[0], camera.cy - batch!.origin[1]);
        gl.uniform2f(tToClip, 2 / (camera.width * camera.scale), 2 / (camera.height * camera.scale));
        gl.uniform1i(tLight, light ? 1 : 0);
        gl.uniform3f(tInk, ...ink);
        gl.uniform3f(tPaper, ...paper);
        gl.uniform4f(tColor, ...((color ?? [0, 0, 0, 0]) as [number, number, number, number]));
        gl.uniform1i(tAtlas, 0);
        gl.activeTexture(gl.TEXTURE0);
        gl.enable(gl.BLEND);
        // The canvas keeps opaque alpha where text is drawn over it.
        gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
        drawText(indices, buffer);
        gl.disable(gl.BLEND);
        gl.bindVertexArray(null);
        gl.useProgram(program);
        gl.bindVertexArray(vao);
      };
      text(textAll, textIndices, null);

      text(textHighlight, textHighlightIndices, [...violet, 1]);

      // Line width is 1 device pixel on most platforms, so violet work is thickened by redrawing
      // it nudged one device pixel at a time across and down.
      const dpr = canvas.width / camera.width;
      const steps = Math.max(2, Math.round(HIGHLIGHT_PX * dpr));
      const px = 2 / canvas.width;
      const py = 2 / canvas.height;
      const thick = (drawOnce: () => void) => {
        gl.uniform4f(uColor, ...violet, 1);
        for (let i = 0; i < steps; i++) {
          for (const [dx, dy] of i === 0 ? [[0, 0]] : [[i, 0], [0, -i]]) {
            gl.uniform2f(uOffset, dx * px, dy * py);
            drawOnce();
          }
        }
      };

      if (highlightCount > 0) {
        thick(() => gl.drawElements(gl.LINES, highlightCount, gl.UNSIGNED_INT, 0));
      }

      if (overlay && overlay.length > 0) {
        // Relative to the batch origin in f64 first, like the batch itself (see the top comment).
        const relative = new Float32Array(overlay.length);
        for (let i = 0; i < overlay.length; i += 2) {
          relative[i] = overlay[i] - batch.origin[0];
          relative[i + 1] = overlay[i + 1] - batch.origin[1];
        }
        gl.bindVertexArray(overlayVao);
        gl.bindBuffer(gl.ARRAY_BUFFER, overlayPositions);
        gl.bufferData(gl.ARRAY_BUFFER, relative, gl.DYNAMIC_DRAW);
        thick(() => gl.drawArrays(gl.LINES, 0, relative.length / 2));
      }
      gl.bindVertexArray(null);
    },

    read() {
      const { width, height } = canvas;
      const rgba = new Uint8Array(width * height * 4);
      gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, rgba);
      return { width, height, rgba };
    },

    dispose() {
      // The context stays: React may mount the same canvas again (StrictMode), and a lost
      // context can't be recovered from the same element.
      gl.deleteBuffer(positions);
      gl.deleteBuffer(colors);
      gl.deleteBuffer(highlightIndices);
      gl.deleteVertexArray(vao);
      gl.deleteBuffer(overlayPositions);
      gl.deleteVertexArray(overlayVao);
      gl.deleteBuffer(fillPositions);
      gl.deleteBuffer(fillColors);
      gl.deleteVertexArray(fillVao);
      gl.deleteBuffer(backgroundPositions);
      gl.deleteVertexArray(backgroundVao);
      gl.deleteProgram(program);
      gl.deleteBuffer(textVertices);
      gl.deleteBuffer(textColors);
      gl.deleteBuffer(textIndices);
      gl.deleteBuffer(textHighlightIndices);
      gl.deleteVertexArray(textVao);
      gl.deleteProgram(textProgram);
      gl.deleteVertexArray(gridVao);
      gl.deleteProgram(gridProgram);
      for (const { texture } of textures.values()) gl.deleteTexture(texture);
      textures.clear();
      gl.deleteBuffer(imageVertices);
      gl.deleteBuffer(imageIndices);
      gl.deleteVertexArray(imageVao);
      gl.deleteProgram(imageProgram);
      for (const { texture } of pictures.values()) gl.deleteTexture(texture);
      pictures.clear();
      for (const v of insetViews) gl.deleteBuffer(v.buffer);
      insetViews = [];
      for (const b of [insetPositions, insetColors, insetFillPositions, insetFillColors]) gl.deleteBuffer(b);
      gl.deleteBuffer(insetTextVertices);
      gl.deleteBuffer(insetTextColors);
      gl.deleteVertexArray(insetVao);
      gl.deleteVertexArray(insetFillVao);
      gl.deleteVertexArray(insetTextVao);
    },
  };
}
