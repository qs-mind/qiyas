export const TRACE_EVENT = "qiyas:trace";

export type TraceTarget = {
  item: string;
  line?: string;
  drawing?: string | null;
  /** false: light the elements and show a small bar; true or left out: show the working too. */
  panel?: boolean;
};

export function trace(target: TraceTarget): void {
  window.dispatchEvent(new CustomEvent<TraceTarget>(TRACE_EVENT, {
    detail: { ...target, drawing: target.drawing ?? null },
  }));
}
