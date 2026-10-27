// The two side panels (layers and properties; Mizan's column with its outputs) can be dragged
// wider or narrower and folded away to give the drawing the whole width. Between each panel and
// the drawing is a handle: drag it to resize, double-click it (or its round button) to fold or
// unfold, arrows on the keyboard to resize. Widths and folds are remembered on this computer.
import { useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { useT } from "../i18n";

export type PanelSide = "left" | "right";
/** What a width is kept for: each panel, and the right column while it shows an output. */
export type WidthSlot = PanelSide | "rightWide";

const KEY = "qiyas.panels.hidden";
const WIDTH_KEY = "qiyas.panels.width";

/** Each panel's width in CSS pixels: the default, the narrowest and the widest it can be. */
export const PANEL_WIDTH: Record<WidthSlot, { initial: number; min: number; max: number }> = {
  left: { initial: 220, min: 180, max: 480 },
  right: { initial: 440, min: 300, max: 760 },
  rightWide: { initial: 672, min: 360, max: 1000 },
};
/** A drag that leaves less than this much of a panel folds it. */
const FOLD_BELOW = 0.6;
/** One arrow key press. */
const STEP = 16;

const clampWidth = (slot: WidthSlot, w: number) => Math.round(Math.min(PANEL_WIDTH[slot].max, Math.max(PANEL_WIDTH[slot].min, w)));
const SLOTS: WidthSlot[] = ["left", "right", "rightWide"];
const initialWidths = () => Object.fromEntries(SLOTS.map((k) => [k, PANEL_WIDTH[k].initial])) as Record<WidthSlot, number>;

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
    typeof v === "number" && Number.isFinite(v) ? clampWidth(slot, v) : PANEL_WIDTH[slot].initial;
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
  /** Folds or unfolds a panel; without `folded`, the other way round from now. */
  const setFolded = (side: PanelSide, folded?: boolean) =>
    setHidden((was) => {
      const to = folded ?? !was[side];
      if (was[side] === to) return was;
      const next = { ...was, [side]: to };
      keep(KEY, next);
      return next;
    });
  const toggle = (side: PanelSide) => setFolded(side);
  const resize = (slot: WidthSlot, w: number) =>
    setWidth((was) => {
      const next = { ...was, [slot]: clampWidth(slot, w) };
      keep(WIDTH_KEY, next);
      return next;
    });
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
  const drag = useRef<{ x: number; width: number; rtl: boolean; moved: boolean } | null>(null);
  const [dragging, setDragging] = useState(false);
  const label = t(`panels.${hidden ? "show" : "hide"}.${side}`);

  // The left panel is on the inline start: dragging towards the drawing widens it. In Arabic the
  // sides swap, so the pointer's direction does too.
  const grow = (dx: number, rtl: boolean) => (side === "left" ? 1 : -1) * (rtl ? -1 : 1) * dx;

  function onPointerDown(e: PointerEvent<HTMLDivElement>) {
    if (e.button !== 0 || (e.target as Element).closest("button")) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    const rtl = getComputedStyle(e.currentTarget).direction === "rtl";
    drag.current = { x: e.clientX, width: hidden ? 0 : width, rtl, moved: false };
    setDragging(true);
  }
  function onPointerMove(e: PointerEvent<HTMLDivElement>) {
    const d = drag.current;
    if (!d) return;
    // A few pixels of jitter is still a click.
    if (!d.moved && Math.abs(e.clientX - d.x) < 3) return;
    d.moved = true;
    const w = d.width + grow(e.clientX - d.x, d.rtl);
    if (w < limits.min * FOLD_BELOW) {
      onFold(true);
      return;
    }
    onFold(false);
    onResize(w);
  }
  function onPointerUp(e: PointerEvent<HTMLDivElement>) {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    setDragging(false);
    e.currentTarget.releasePointerCapture?.(e.pointerId);
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
      className={`panel-handle at-${side}${hidden ? " folded" : ""}${dragging ? " dragging" : ""}`}
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
