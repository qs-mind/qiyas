/** Tracing a quantity back to the drawing: a BOQ row, a take-off row, or a cell of an exported
 *  Excel or PDF shown inside Qiyas sends `trace` (src/boq/trace.ts). The item is explained once
 *  by the core, its elements light up on their sheet with what each gives as a tag beside it
 *  (voids deducted in their own colour), and "How was it calculated" shows the working. */
import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { drawingInfo, explainItem, type DrawingInfo, type Explanation } from "../api";
import { TRACE_EVENT, type TraceTarget } from "../boq/trace";
import { num, unitText } from "../boq/howCalculated";
import { uiError } from "../i18n/errors";
import { handlesBounds } from "../tools/overlay";
import type { Batch } from "../viewport/batch";
import type { Bounds } from "../viewport/camera";
import type { DrawingLabel, ViewportHandle } from "../viewport/Viewport";

type Deps = {
  info: DrawingInfo | null;
  sheet: string;
  batch: Batch | null;
  viewport: RefObject<ViewportHandle | null>;
  run(f: () => Promise<void>): unknown;
  chooseSheet(block: string): unknown;
  openPath(path: string): Promise<unknown>;
  setAiHighlight(handles: number[]): void;
  setError(message: string | null): void;
};

export type Traced = { target: TraceTarget; explanation: Explanation; panel: boolean };

export function useTrace({ info, sheet, batch, viewport, run, chooseSheet, openPath, setAiHighlight, setError }: Deps) {
  const [traced, setTraced] = useState<Traced | null>(null);
  const [focus, setFocus] = useState<number | null>(null);
  /** Fit the view once the traced sheet's batch is drawn. */
  const fitPending = useRef(false);
  const live = useRef({ info, sheet, run, chooseSheet, openPath, setAiHighlight, setError });
  live.current = { info, sheet, run, chooseSheet, openPath, setAiHighlight, setError };

  useEffect(() => {
    const onTrace = (event: Event) => {
      const target = (event as CustomEvent<TraceTarget & { panel?: boolean }>).detail;
      const { run, openPath, chooseSheet, setError } = live.current;
      void run(async () => {
        if (target.drawing && target.drawing !== live.current.info?.path) {
          await openPath(target.drawing);
          // A cancelled open must never light handles in the drawing still on screen.
          if ((await drawingInfo()).path !== target.drawing) return;
        }
        let explanation: Explanation;
        try {
          explanation = await explainItem(target.item);
        } catch (error) {
          setError(uiError(error));
          return;
        }
        const shares = explanation.by_element ?? [];
        const focusOn = target.line ? (shares.find((s) => s.lines.includes(target.line!))?.handle ?? null) : null;
        const sheets = shares.map((s) => s.sheet).concat(explanation.evidence.map((e) => e.sheet));
        const here = sheets.find((s) => s.toLowerCase() === live.current.sheet.toLowerCase());
        const goTo = here ?? sheets[0];
        setTraced({ target, explanation, panel: target.panel ?? true });
        setFocus(focusOn);
        fitPending.current = true;
        if (goTo && goTo.toLowerCase() !== live.current.sheet.toLowerCase()) await chooseSheet(goTo);
      });
    };
    window.addEventListener(TRACE_EVENT, onTrace);
    return () => window.removeEventListener(TRACE_EVENT, onTrace);
  }, []);

  // A drawing switched under the trace leaves it: its handles belong to the other drawing.
  useEffect(() => {
    setTraced((t) => (t && t.target.drawing && t.target.drawing !== info?.path ? null : t));
  }, [info?.path]);

  /** The handles lit on the sheet in view: every element (or the one in focus), its drawn parts, its voids. */
  const lit = useMemo(() => {
    if (!traced) return [] as number[];
    const shares = (traced.explanation.by_element ?? []).filter(
      (s) => s.sheet.toLowerCase() === sheet.toLowerCase() && (focus == null || s.handle === focus),
    );
    if (!shares.length && focus == null)
      return traced.explanation.evidence.filter((e) => e.sheet.toLowerCase() === sheet.toLowerCase()).map((e) => e.handle);
    return [...new Set(shares.flatMap((s) => [s.handle, ...(s.parts ?? []), ...s.voids.map((v) => v.handle)]))];
  }, [traced, focus, sheet]);

  useEffect(() => {
    if (traced) setAiHighlight(lit);
  }, [lit]);

  const labels = useMemo((): DrawingLabel[] => {
    if (!traced || !batch) return [];
    const out: DrawingLabel[] = [];
    const centre = (b: Bounds) => [(b[0][0] + b[1][0]) / 2, (b[0][1] + b[1][1]) / 2] as [number, number];
    for (const share of traced.explanation.by_element ?? []) {
      if (share.sheet.toLowerCase() !== sheet.toLowerCase()) continue;
      const box = handlesBounds([share.handle, ...(share.parts ?? [])], batch);
      if (box)
        out.push({
          key: `e${share.handle}`,
          at: centre(box),
          text: `${num(share.value)} ${unitText(share.unit)}`,
          tone: focus === share.handle ? "focus" : "host",
        });
      for (const v of share.voids) {
        const vb = handlesBounds([v.handle], batch);
        if (vb && v.included) out.push({ key: `v${share.handle}-${v.handle}`, at: centre(vb), text: `${num(v.value)} ${unitText(share.unit)}`, tone: "void" });
      }
    }
    return out;
  }, [traced, batch, sheet, focus]);

  // Fit once to what is lit, as soon as the sheet it is on is drawn; and again on a focus change.
  useEffect(() => {
    if (!traced || !batch || !lit.length) return;
    if (!fitPending.current) return;
    const box = handlesBounds(lit, batch);
    if (!box) return;
    fitPending.current = false;
    viewport.current?.fitTo(box);
  }, [traced, batch, lit]);
  useEffect(() => {
    fitPending.current = true;
  }, [focus]);

  const close = () => {
    setTraced(null);
    setFocus(null);
    setAiHighlight([]);
  };
  /** Lights one source entity (a note, table or block a value was read from), on its own sheet. */
  const showSource = (handle: number) => {
    setFocus(null);
    fitPending.current = false;
    setAiHighlight([handle]);
    const box = handlesBounds([handle], batch);
    if (box) viewport.current?.fitTo(box);
  };
  return { traced, focus, setFocus, labels, close, showSource, openPanel: () => setTraced((t) => (t ? { ...t, panel: true } : t)) };
}
export type Trace = ReturnType<typeof useTrace>;
