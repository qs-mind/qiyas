// The two side panels (layers and properties; Mizan's column with its outputs) can be dragged
// wider or narrower and folded away to give the drawing the whole width. Between each panel and
// the drawing is a handle: drag it to resize, double-click it (or its round button) to fold or
// unfold, arrows on the keyboard to resize. Widths and folds are remembered on this computer.
import { useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { useT } from "../i18n";

export type PanelSide = "left" | "right";
/** What a width is kept for: each panel, and the right column while it shows an output. */
export type WidthSlot = PanelSide | "rightWide";

const KEY = "qiyas.panels.hidden";
const WIDTH_KEY = "qiyas.panels.width";

/** Each panel's width in CSS pixels: the default, the narrowest and the widest it can be. */
export const PANEL_WIDTH: Record<WidthSlot, { initial: number; min: number; max: number }> = {
  left: { initial: 260, min: 200, max: 480 },
  right: { initial: 440, min: 300, max: 760 },
  rightWide: { initial: 672, min: 360, max: 1000 },
};
/** A drag that leaves less than this much of a panel folds it. */
const FOLD_BELOW = 0.6;
/** One arrow key press. */
const STEP = 16;

const clampWidth = (slot: WidthSlot, w: number) => Math.round(Math.min(PANEL_WIDTH[slot].max, Math.max(PANEL_WIDTH[slot].min, w)));
const SLOTS: WidthSlot[] = ["left", "right", "rightWide"];
/** Below this window width Mizan's column starts narrower, so the drawing keeps its room (a choice
 *  the user made is kept as it was). */
const NARROW_WINDOW = 1300;
const NARROW_START: Partial<Record<WidthSlot, number>> = { right: 360, rightWide: 560 };

/** A panel's width before the user chose one: its `initial`, or the narrow start in a narrow window. */
export function defaultWidth(slot: WidthSlot): number {
  const narrow = typeof window !== "undefined" && window.innerWidth > 0 && window.innerWidth < NARROW_WINDOW;
  return (narrow && NARROW_START[slot]) || PANEL_WIDTH[slot].initial;
}

const initialWidths = () => Object.fromEntries(SLOTS.map((k) => [k, defaultWidth(k)])) as Record<WidthSlot, number>;

function storedHidden(): Record<PanelSide, boolean> {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    return { left: raw.left === true, right: raw.right === true };
  } catch {
    // Storage blocked or not JSON: both panels shown.
    return { left: false, right: false };
  }
}

function storedWidth(): Record<WidthSlot, number> {
  const width = (slot: WidthSlot, v: unknown) =>
    typeof v === "number" && Number.isFinite(v) ? clampWidth(slot, v) : defaultWidth(slot);
  try {
    const raw = JSON.parse(localStorage.getItem(WIDTH_KEY) ?? "{}") ?? {};
    return Object.fromEntries(SLOTS.map((k) => [k, width(k, raw[k])])) as Record<WidthSlot, number>;
  } catch {
    return initialWidths();
  }
}

function keep(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Not remembered; the panel still changes for this session.
  }
}

/** Which side panels are hidden and how wide each is, with the changes that also remember them. */
export function usePanelRails() {
  const [hidden, setHidden] = useState(storedHidden);
  const [width, setWidth] = useState(storedWidth);
  const hiddenRef = useRef(hidden);
  const widthRef = useRef(width);
  /** Folds or unfolds a panel; without `folded`, the other way round from now. */
  const setFolded = (side: PanelSide, folded?: boolean) => {
    const was = hiddenRef.current;
    const to = folded ?? !was[side];
    if (was[side] === to) return;
    const next = { ...was, [side]: to };
    hiddenRef.current = next;
    setHidden(next);
    keep(KEY, next);
  };
  const toggle = (side: PanelSide) => setFolded(side);
  const resize = (slot: WidthSlot, w: number) => {
    const next = { ...widthRef.current, [slot]: clampWidth(slot, w) };
    widthRef.current = next;
    setWidth(next);
    keep(WIDTH_KEY, next);
  };
  return { hidden, width, toggle, setFolded, resize };
}

type HandleProps = {
  side: PanelSide;
  /** Whose limits the width keeps to; the side's own by default. */
  slot?: WidthSlot;
  hidden: boolean;
  width: number;
  onToggle(): void;
  onFold(folded: boolean): void;
  onResize(width: number): void;
};

/** The handle between a side panel and the drawing. */
export function PanelHandle({ side, slot = side, hidden, width, onToggle, onFold, onResize }: HandleProps) {
  const t = useT();
  const limits = PANEL_WIDTH[slot];
  const drag = useRef<{
    x: number; width: number; rtl: boolean; moved: boolean; pointerId: number;
    nextWidth: number; folded: boolean; restore(): void; preview(width: number, folded: boolean): void;
  } | null>(null);
  const handleRef = useRef<HTMLDivElement>(null);
  const label = t(`panels.${hidden ? "show" : "hide"}.${side}`);

  useLayoutEffect(() => {
    handleRef.current?.classList.toggle("folded", hidden);
    handleRef.current?.setAttribute("aria-valuenow", String(hidden ? 0 : width));
    return () => {
      drag.current?.restore();
      drag.current = null;
    };
  }, [side, slot, hidden, width]);

  // The left panel is on the inline start: dragging towards the drawing widens it. In Arabic the
  // sides swap, so the pointer's direction does too.
  const grow = (dx: number, rtl: boolean) => (side === "left" ? 1 : -1) * (rtl ? -1 : 1) * dx;

  function onPointerDown(e: PointerEvent<HTMLDivElement>) {
    if (drag.current || e.button !== 0 || (e.target as Element).closest("button")) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    const rtl = getComputedStyle(e.currentTarget).direction === "rtl";
    const handle = e.currentTarget;
    const middle = handle.closest<HTMLElement>(".middle");
    const panel = middle?.querySelector<HTMLElement>(side === "left" ? ":scope > .left" : ":scope > .side");
    const savedWidth = panel?.style.width ?? "";
    const savedDisplay = panel?.style.display ?? "";
    drag.current = {
      x: e.clientX, width: hidden ? 0 : width, rtl, moved: false, pointerId: e.pointerId,
      nextWidth: width, folded: hidden,
      preview: (w, folded) => {
        if (panel) {
          panel.style.width = `${w}px`;
          panel.style.display = folded ? "none" : "flex";
        }
        handle.classList.toggle("folded", folded);
        handle.setAttribute("aria-valuenow", String(folded ? 0 : w));
      },
      restore: () => {
        if (panel) {
          panel.style.width = savedWidth;
          panel.style.display = savedDisplay;
        }
        handle.classList.remove("dragging");
        handle.classList.toggle("folded", hidden);
        handle.setAttribute("aria-valuenow", String(hidden ? 0 : width));
      },
    };
    handle.classList.add("dragging");
  }
  function onPointerMove(e: PointerEvent<HTMLDivElement>) {
    const d = drag.current;
    if (!d || d.pointerId !== e.pointerId) return;
    // A few pixels of jitter is still a click.
    if (!d.moved && Math.abs(e.clientX - d.x) < 3) return;
    d.moved = true;
    const w = d.width + grow(e.clientX - d.x, d.rtl);
    d.folded = w < limits.min * FOLD_BELOW;
    if (!d.folded) d.nextWidth = clampWidth(slot, w);
    d.preview(d.nextWidth, d.folded);
  }
  function onPointerUp(e: PointerEvent<HTMLDivElement>) {
    const d = drag.current;
    if (!d || d.pointerId !== e.pointerId) return;
    drag.current = null;
    d.restore();
    if (e.currentTarget.hasPointerCapture?.(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
    if (d.moved) {
      onFold(d.folded);
      if (!d.folded) onResize(d.nextWidth);
    }
    // A click on a folded panel's strip opens it.
    if (hidden && !d.moved && e.type === "pointerup") onToggle();
  }
  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.target !== e.currentTarget) return;
    const rtl = getComputedStyle(e.currentTarget).direction === "rtl";
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      const more = grow(e.key === "ArrowRight" ? STEP : -STEP, rtl);
      if (hidden) {
        // Folded, only the arrow that widens opens it.
        if (more <= 0) return;
        onFold(false);
        onResize(limits.min);
        return;
      }
      onResize(width + more);
    } else if (e.key === "Enter") {
      e.preventDefault();
      onToggle();
    }
  }

  return (
    <div
      ref={handleRef}
      className={`panel-handle at-${side}${hidden ? " folded" : ""}`}
      role="separator"
      aria-orientation="vertical"
      aria-label={t(`panels.resize.${side}`)}
      aria-valuemin={0}
      aria-valuemax={limits.max}
      aria-valuenow={hidden ? 0 : width}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onLostPointerCapture={onPointerUp}
      onDoubleClick={onToggle}
      onKeyDown={onKeyDown}
    >
      <button
        type="button"
        className="panel-grip"
        // The separator itself takes the keyboard (Enter folds); the grip is for the pointer.
        tabIndex={-1}
        aria-expanded={!hidden}
        aria-label={label}
        title={label}
        onClick={onToggle}
        onDoubleClick={(e) => e.stopPropagation()}
      >
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path d="M10 3.5L5.5 8l4.5 4.5" />
        </svg>
      </button>
    </div>
  );
}
