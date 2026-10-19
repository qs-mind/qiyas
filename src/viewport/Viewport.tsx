import { useEffect, useImperativeHandle, useRef, useState, type PointerEvent, type Ref } from "react";
import {
  curvesNeedRedraw,
  glyphsNeedRedraw,
  picturesToFetch,
  picturesToRelease,
  type AtlasPage,
  type Batch,
  type HeldPicture,
  type Target,
} from "./batch";
import {
  fitBounds,
  pan,
  resized,
  screenToWorld,
  visibleBounds,
  worldToScreen,
  zoomAt,
  type Bounds,
  type Camera,
  type Point,
} from "./camera";
import { isClick } from "./click";
import type { ConstraintTool } from "../tools/constraintTools";
import type { DimKind } from "../tools/dimTools";
import type { DrawKind } from "../tools/drawTools";
import type { EditKind } from "../tools/editTools";
import type { ModifyKind } from "../tools/modifyTools";
import type { TextKind } from "../tools/textTools";
import type { PaperView, Ucs } from "../api";
import { palette, type CanvasColours } from "./colours";
import { createRenderer, type Renderer, type Theme } from "./gl";
import { UCS_ICON_PX, ucsIconPlace } from "./ucsIcon";
import { wheelFactor } from "./wheel";

export type Tool =
  /** The draw tools (`DRAW_TOOLS`). */
  | DrawKind
  /** The modify tools (`MODIFY_TOOLS`). */
  | ModifyKind
  /** The edit tools (`EDIT_TOOLS`): trim, extend, fillet… */
  | EditKind
  /** The text tools (`TEXT_TOOLS`): TEXT and MTEXT. */
  | TextKind
  /** The annotate tools (`DIM_TOOLS`): dimensions, their edits, leaders and tolerances. */
  | DimKind
  /** The parametric tools (`CONSTRAINT_TOOLS`): constraints, AUTOCONSTRAIN, DELCONSTRAINT. */
  | ConstraintTool
  /** Click a text to edit it. */
  | "textEdit"
  /** Click a table to edit its cells. */
  | "tableEdit"
  /** Click a block reference to edit its block in place. */
  | "refedit"
  | "select"
  | "distance"
  | "area"
  | "count"
  | "zoomWindow"
  | "geometry"
  | "divide"
  | "measureAlong"
  | "idPoint"
  /** PASTECLIP (Ctrl+V) and PASTEBLOCK: the clipboard pasted at each click. */
  | "pasteclip"
  | "pasteblock"
  /** MVIEW: a paper-space viewport by two corners. */
  | "mview"
  /** UCS: an origin, then a point on its x axis; UCS OB: an entity's. */
  | "ucs"
  | "ucsObject";

export type ViewportHandle = {
  fitExtents(): void;
  fitTo(bounds: Bounds): void;
  /** Zooms about the middle of the view: `factor` below 1 zooms in. */
  zoomBy(factor: number): void;
  /** Screen (device) pixels per drawing unit at the current zoom. */
  pxPerUnit(): number;
  /** The drawing area in view. */
  visible(): Bounds;
  /** Where the drawing point `p` is on the canvas, in CSS pixels from its top left corner. */
  toScreen(p: Point): Point;
};

type Props = {
  batch: Batch | null;
  /** Every glyph atlas page received so far; the batch's quads draw from them. */
  atlas?: AtlasPage[];
  highlight: Target[];
  tool: Tool;
  theme: Theme;
  /** The user's background and highlight colours over the theme's (the Options dialog's). */
  colours?: CanvasColours | null;
  /** Violet segments `x0 y0 x1 y1` in absolute drawing coordinates, drawn over the drawing. */
  overlay?: Float64Array | null;
  /** Drawing units between the grid's dots; null or left out for no grid. */
  grid?: number | null;
  /** The user coordinate system the grid runs along; the UCS icon sits on its origin. */
  ucs?: Ucs | null;
  /** The UCS icon (UCSICON): on the origin while it is on screen, else bottom left. On when left
   *  out. */
  ucsIcon?: boolean;
  /** On a layout: model space (`batch`) drawn inside each of its viewports (`views`). */
  inset?: { batch: Batch; views: PaperView[] } | null;
  /** A left click; `worldPerPixel` is the zoom at that moment, for pick tolerances; `shift` when
   *  Shift was held, `ctrl` when Ctrl (or Cmd) was. */
  onClick(world: Point, screen: Point, worldPerPixel: number, shift: boolean, ctrl?: boolean): void;
  /** A left drag draws a selection box (the select tool, a modify tool picking). */
  boxes?: boolean;
  /** A selection box was dragged: `crossing` when drawn right to left (it takes what it touches;
   *  left to right only what lies wholly inside), `shift` when Shift was held at the press. */
  onBox?(bounds: Bounds, crossing: boolean, shift: boolean): void;
  /** The cursor moved; `worldPerPixel` is the zoom at that moment, for snap tolerances. */
  onMove(world: Point, worldPerPixel: number): void;
  /** A zoom window was dragged and shown: a corner typed before it is dropped. */
  onWindowZoomed?(): void;
  /** The zoom has more than doubled or halved since the batch's glyphs were drawn: draw them
   *  again for `pxPerUnit` screen pixels per drawing unit. Called once the zoom settles. */
  onZoomHint?(pxPerUnit: number): void;
  /** The underlay pictures held, by image id. */
  pictures?: Map<number, HeldPicture>;
  /** Some underlay picture in view is missing or was drawn for a zoom too far off, or one held is
   *  out of view: fetch and let go for `pxPerUnit` and the area `visible`. Called once the view
   *  settles. */
  onPictureHint?(pxPerUnit: number, visible: Bounds): void;
  ref?: Ref<ViewportHandle>;
};

const FIT_MARGIN_PX = 20;
const DOUBLE_CLICK_MS = 400;
/** A zoom window or middle drag smaller than this many pixels is no drag. */
const DRAG_PX = 4;
/** How long the zoom must rest before glyphs are asked for at the new size. */
const ZOOM_SETTLE_MS = 200;
const NO_PAGES: AtlasPage[] = [];
const NO_PICTURES = new Map<number, HeldPicture>();

type Drag = { kind: "pan" | "zoomWindow" | "select"; start: Point; last: Point };

/** The drawing canvas: wheel zoom at the cursor, middle or space drag to pan, double middle click
 *  for extents, and a drag rectangle to zoom into when the tool is `zoomWindow`. */
export function Viewport({
  batch,
  atlas = NO_PAGES,
  highlight,
  tool,
  theme,
  colours = null,
  overlay = null,
  grid = null,
  ucs = null,
  ucsIcon = true,
  inset = null,
  onClick,
  boxes = false,
  onBox,
  onMove,
  onWindowZoomed,
  onZoomHint,
  pictures = NO_PICTURES,
  onPictureHint,
  ref,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const renderer = useRef<Renderer | null>(null);
  const camera = useRef<Camera>({ cx: 0, cy: 0, scale: 1, width: 1, height: 1 });
  /** The world box the user (or the app) last chose to show; a resize keeps it in view. */
  const view = useRef<Bounds | null>(null);
  const lookRef = useRef(palette(theme, colours));
  /** The UCS icon, moved on every frame without a React render (the camera is a ref). */
  const iconRef = useRef<SVGGElement>(null);
  const iconMarkRef = useRef<SVGRectElement>(null);
  const ucsRef = useRef(ucs);
  ucsRef.current = ucs;
  const batchRef = useRef(batch);
  const frame = useRef(0);
  const drag = useRef<Drag | null>(null);
  /** Where a left press began; it is a click only if the release is within a few pixels. */
  const press = useRef<Point | null>(null);
  const space = useRef(false);
  const lastMiddle = useRef(0);
  const [zoomBox, setZoomBox] = useState<[Point, Point] | null>(null);
  const [selectBox, setSelectBox] = useState<[Point, Point] | null>(null);
  /** Shift was held at the left press. */
  const pressShift = useRef(false);
  const zoomTimer = useRef(0);
  const onZoomHintRef = useRef(onZoomHint);
  onZoomHintRef.current = onZoomHint;
  const picturesRef = useRef(pictures);
  picturesRef.current = pictures;
  const pictureTimer = useRef(0);
  const onPictureHintRef = useRef(onPictureHint);
  onPictureHintRef.current = onPictureHint;

  const pxPerUnit = () => window.devicePixelRatio / camera.current.scale;

  /** Asks for glyphs at the current size once the zoom has rested, if they are off by over 2x,
   *  and for curves drawn again when the zoom shows their corners (or no longer needs them so
   *  fine), as AutoCAD regenerates. */
  function checkGlyphs() {
    const stale = () => {
      const b = batchRef.current;
      if (!b) return false;
      const glyphs = b.quads.length > 0 && glyphsNeedRedraw(b.pxPerUnit, pxPerUnit());
      return glyphs || curvesNeedRedraw(b.baseTol ?? 0, b.curveTol ?? 0, pxPerUnit());
    };
    if (!stale()) return;
    clearTimeout(zoomTimer.current);
    zoomTimer.current = window.setTimeout(() => {
      if (stale()) onZoomHintRef.current?.(pxPerUnit());
    }, ZOOM_SETTLE_MS);
  }

  /** Asks for underlay pictures once the view has rested, when some in view are missing or were
   *  drawn for a zoom over 2x off, or some held are out of view. */
  function checkPictures() {
    const stale = () => {
      const [b, held, visible] = [batchRef.current, picturesRef.current, visibleBounds(camera.current)];
      return (
        picturesToFetch(b, held, pxPerUnit(), visible).length > 0 ||
        picturesToRelease(b, held, visible).length > 0
      );
    };
    if (!stale()) return;
    clearTimeout(pictureTimer.current);
    pictureTimer.current = window.setTimeout(() => {
      if (stale()) onPictureHintRef.current?.(pxPerUnit(), visibleBounds(camera.current));
    }, ZOOM_SETTLE_MS);
  }

  function requestDraw() {
    if (frame.current) return;
    frame.current = requestAnimationFrame(() => {
      frame.current = 0;
      renderer.current?.draw(camera.current, lookRef.current);
      placeIcon();
    });
  }

  /** Puts the UCS icon where `ucsIconPlace` says, in a colour that reads on the background. */
  function placeIcon() {
    const icon = iconRef.current;
    if (!icon) return;
    const place = ucsIconPlace(camera.current, ucsRef.current);
    icon.setAttribute("transform", `translate(${place.x} ${place.y}) rotate(${place.angle})`);
    const look = lookRef.current;
    const [r, g, b] = (look.light ? look.ink : look.paper).map((c) => Math.round(c * 255));
    icon.style.color = `rgb(${r}, ${g}, ${b})`;
    iconMarkRef.current?.setAttribute("visibility", place.atOrigin ? "visible" : "hidden");
  }

  function setCamera(c: Camera, keepView = false) {
    camera.current = c;
    if (!keepView) view.current = visibleBounds(c);
    requestDraw();
    checkGlyphs();
    checkPictures();
  }

  function fitExtents() {
    const b = batchRef.current;
    if (b) setCamera(fitBounds(camera.current, b.bounds, FIT_MARGIN_PX));
  }

  useImperativeHandle(
    ref,
    () => ({
      fitExtents,
      fitTo: (b) => setCamera(fitBounds(camera.current, b, FIT_MARGIN_PX)),
      zoomBy: (factor) => {
        const c = camera.current;
        setCamera(zoomAt(c, [c.width / 2, c.height / 2], factor));
      },
      pxPerUnit,
      visible: () => visibleBounds(camera.current),
      toScreen: (p) => worldToScreen(camera.current, p),
    }),
    [],
  );

  useEffect(() => {
    const canvas = canvasRef.current!;
    renderer.current = createRenderer(canvas);

    const resize = () => {
      const { width, height } = canvas.getBoundingClientRect();
      const dpr = window.devicePixelRatio;
      canvas.width = Math.max(1, Math.round(width * dpr));
      canvas.height = Math.max(1, Math.round(height * dpr));
      // A drawing fitted while the canvas had no size (hidden at load) is fitted again once it has.
      const hadNoSize = camera.current.width <= 1 || camera.current.height <= 1;
      const [w, h] = [Math.max(1, width), Math.max(1, height)];
      // A side panel widening or narrowing the canvas keeps what was in view, centred.
      if (hadNoSize || !view.current) setCamera({ ...camera.current, width: w, height: h });
      else setCamera(resized(camera.current, w, h, view.current), true);
      if (hadNoSize) fitExtents();
    };
    // Measure now, so a batch arriving on the first render fits the real size.
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    // Moving the window to another screen, or zooming the page, changes the pixel ratio without
    // resizing the canvas: measure again. A query fires once per change, so arm a new one each time.
    let ratio: MediaQueryList | undefined;
    const onRatio = () => {
      resize();
      watchRatio();
    };
    const watchRatio = () => {
      ratio = window.matchMedia?.(`(resolution: ${window.devicePixelRatio}dppx)`);
      ratio?.addEventListener("change", onRatio, { once: true });
    };
    watchRatio();

    const local = (e: MouseEvent): Point => {
      const r = canvas.getBoundingClientRect();
      return [e.clientX - r.left, e.clientY - r.top];
    };
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      if (e.deltaY === 0) return;
      setCamera(zoomAt(camera.current, local(e), wheelFactor(e.deltaY, e.deltaMode)));
    };
    // React's wheel listener is passive, so it can't stop the page from scrolling.
    canvas.addEventListener("wheel", onWheel, { passive: false });

    const typing = (e: KeyboardEvent) =>
      e.target instanceof HTMLElement &&
      (e.target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(e.target.tagName));
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.code === "Space" && !typing(e)) space.current = true;
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.code === "Space") space.current = false;
    };
    const onBlur = () => (space.current = false);
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);

    return () => {
      observer.disconnect();
      ratio?.removeEventListener("change", onRatio);
      canvas.removeEventListener("wheel", onWheel);
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onBlur);
      cancelAnimationFrame(frame.current);
      frame.current = 0;
      clearTimeout(zoomTimer.current);
      clearTimeout(pictureTimer.current);
      renderer.current?.dispose();
      renderer.current = null;
    };
  }, []);

  // Before the batch, so its quads find their pages.
  useEffect(() => {
    renderer.current!.setAtlas(atlas);
    requestDraw();
  }, [atlas]);

  // No fit here: hiding a layer gives a new batch but should keep the view. The app fits on open.
  useEffect(() => {
    batchRef.current = batch;
    renderer.current!.setBatch(batch);
    requestDraw();
    checkGlyphs();
    checkPictures();
  }, [batch]);

  useEffect(() => {
    renderer.current!.setPictures(pictures);
    requestDraw();
  }, [pictures]);

  useEffect(() => {
    lookRef.current = palette(theme, colours);
    requestDraw();
  }, [theme, colours?.background, colours?.highlight]);

  useEffect(() => {
    renderer.current!.setHighlight(highlight);
    requestDraw();
  }, [highlight]);

  useEffect(() => {
    renderer.current!.setOverlay(overlay);
    requestDraw();
  }, [overlay]);

  useEffect(() => {
    renderer.current!.setGrid(grid, ucs);
    requestDraw();
  }, [grid, ucs]);

  useEffect(() => {
    if (ucsIcon) requestDraw();
  }, [ucsIcon]);

  useEffect(() => {
    renderer.current!.setInset(inset?.batch ?? null, inset?.views ?? []);
    requestDraw();
  }, [inset]);

  const local = (e: PointerEvent): Point => {
    const r = e.currentTarget.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  };

  function startDrag(e: PointerEvent<HTMLCanvasElement>, kind: Drag["kind"], p: Point) {
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { kind, start: p, last: p };
  }

  function onPointerDown(e: PointerEvent<HTMLCanvasElement>) {
    const p = local(e);
    if (e.button === 1) {
      e.preventDefault(); // no middle-click autoscroll or paste
      const now = performance.now();
      if (now - lastMiddle.current < DOUBLE_CLICK_MS) {
        lastMiddle.current = 0;
        fitExtents();
        return;
      }
      lastMiddle.current = now;
      startDrag(e, "pan", p);
    } else if (e.button === 0 && space.current) {
      startDrag(e, "pan", p);
    } else if (e.button === 0 && tool === "zoomWindow") {
      startDrag(e, "zoomWindow", p);
      setZoomBox([p, p]);
    } else if (e.button === 0) {
      // The click is made on release, and only if the pointer stayed put.
      e.currentTarget.setPointerCapture(e.pointerId);
      press.current = p;
      pressShift.current = e.shiftKey;
    }
  }

  function onPointerMove(e: PointerEvent<HTMLCanvasElement>) {
    const p = local(e);
    const down = press.current;
    // A left press that moves becomes a selection box where boxes are drawn.
    if (down && !drag.current && boxes && Math.max(Math.abs(p[0] - down[0]), Math.abs(p[1] - down[1])) > DRAG_PX) {
      press.current = null;
      drag.current = { kind: "select", start: down, last: p };
    }
    const d = drag.current;
    if (d?.kind === "select") {
      setSelectBox([d.start, p]);
    } else if (d?.kind === "pan") {
      setCamera(pan(camera.current, p[0] - d.last[0], p[1] - d.last[1]));
    } else if (d?.kind === "zoomWindow") {
      setZoomBox([d.start, p]);
    }
    if (d) d.last = p;
    onMove(screenToWorld(camera.current, p), camera.current.scale);
  }

  function onPointerUp(e: PointerEvent<HTMLCanvasElement>) {
    const down = press.current;
    if (down && e.button === 0) {
      press.current = null;
      const p = local(e);
      if (isClick(down, p)) {
        onClick(screenToWorld(camera.current, p), p, camera.current.scale, e.shiftKey, e.ctrlKey || e.metaKey);
      }
    }
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    const [dx, dy] = [Math.abs(d.last[0] - d.start[0]), Math.abs(d.last[1] - d.start[1])];
    // A middle drag is a pan, not the first half of a double click.
    if (d.kind === "pan" && Math.max(dx, dy) > DRAG_PX) lastMiddle.current = 0;
    if (d.kind === "select") {
      setSelectBox(null);
      const [ax, ay] = screenToWorld(camera.current, d.start);
      const [bx, by] = screenToWorld(camera.current, d.last);
      const bounds: [Point, Point] = [
        [Math.min(ax, bx), Math.min(ay, by)],
        [Math.max(ax, bx), Math.max(ay, by)],
      ];
      onBox?.(bounds, d.last[0] < d.start[0], pressShift.current);
    }
    if (d.kind === "zoomWindow") {
      setZoomBox(null);
      if (dx > DRAG_PX && dy > DRAG_PX) {
        const [ax, ay] = screenToWorld(camera.current, d.start);
        const [bx, by] = screenToWorld(camera.current, d.last);
        const bounds: [Point, Point] = [
          [Math.min(ax, bx), Math.min(ay, by)],
          [Math.max(ax, bx), Math.max(ay, by)],
        ];
        setCamera(fitBounds(camera.current, bounds, 0));
        onWindowZoomed?.();
      }
    }
  }

  return (
    <div style={{ position: "relative", width: "100%", height: "100%", overflow: "hidden" }}>
      <canvas
        ref={canvasRef}
        role="img"
        aria-label="Drawing"
        style={{ display: "block", width: "100%", height: "100%", touchAction: "none" }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={() => {
          drag.current = null;
          press.current = null;
          setZoomBox(null);
          setSelectBox(null);
        }}
      />
      {ucsIcon && (
        <svg
          data-testid="ucs-icon"
          aria-hidden="true"
          // The drawing never mirrors in Arabic, so neither does its icon.
          style={{
            direction: "ltr",
            position: "absolute",
            inset: 0,
            width: "100%",
            height: "100%",
            pointerEvents: "none",
            overflow: "visible",
          }}
        >
          <g ref={iconRef} opacity={0.7} fill="none" stroke="currentColor" strokeWidth={1.5} strokeLinecap="round">
            <path d={`M0 0H${UCS_ICON_PX}M${UCS_ICON_PX - 6} -4L${UCS_ICON_PX} 0L${UCS_ICON_PX - 6} 4`} />
            <path d={`M0 0V${-UCS_ICON_PX}M-4 ${6 - UCS_ICON_PX}L0 ${-UCS_ICON_PX}L4 ${6 - UCS_ICON_PX}`} />
            <rect ref={iconMarkRef} x={-3} y={-3} width={6} height={6} />
            <text x={UCS_ICON_PX + 4} y={4} fill="currentColor" stroke="none" fontSize={11}>
              X
            </text>
            <text x={-4} y={-UCS_ICON_PX - 4} fill="currentColor" stroke="none" fontSize={11}>
              Y
            </text>
          </g>
        </svg>
      )}
      {selectBox && (
        <div
          data-testid="select-box"
          data-crossing={selectBox[1][0] < selectBox[0][0]}
          style={{
            position: "absolute",
            left: Math.min(selectBox[0][0], selectBox[1][0]),
            top: Math.min(selectBox[0][1], selectBox[1][1]),
            width: Math.abs(selectBox[1][0] - selectBox[0][0]),
            height: Math.abs(selectBox[1][1] - selectBox[0][1]),
            // A window (left to right) is solid, a crossing (right to left) dashed, as in AutoCAD.
            border: `1px ${selectBox[1][0] < selectBox[0][0] ? "dashed" : "solid"} #6E4A9E`,
            background:
              selectBox[1][0] < selectBox[0][0] ? "rgba(201, 166, 224, 0.18)" : "rgba(110, 74, 158, 0.10)",
            pointerEvents: "none",
          }}
        />
      )}
      {zoomBox && (
        <div
          style={{
            position: "absolute",
            left: Math.min(zoomBox[0][0], zoomBox[1][0]),
            top: Math.min(zoomBox[0][1], zoomBox[1][1]),
            width: Math.abs(zoomBox[1][0] - zoomBox[0][0]),
            height: Math.abs(zoomBox[1][1] - zoomBox[0][1]),
            border: "1px dashed #6E4A9E",
            pointerEvents: "none",
          }}
        />
      )}
    </div>
  );
}
