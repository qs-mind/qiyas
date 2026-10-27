import { useRef } from "react";

/** `props` with every function replaced by one that never changes and calls the newest one, so a
 *  `memo` component below it re-renders for a changed value and not for a changed closure. The
 *  app's root renders on every pointer move while a tool is in use, and builds its handlers
 *  inline; the ribbon (30 tooltips, 8 menus) was drawn again each time (W2). A prop that is
 *  absent or not a function (the ribbon's "no drawing" buttons are `undefined`) is passed as it is,
 *  so a button disabled by a missing handler stays disabled. */
export function useStableCallbacks<P extends object>(props: P): P {
  const latest = useRef(props as Record<string, unknown>);
  latest.current = props as Record<string, unknown>;
  const wrappers = useRef(new Map<string, (...args: unknown[]) => unknown>());
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(props)) {
    if (typeof value !== "function") {
      out[key] = value;
      continue;
    }
    let wrapper = wrappers.current.get(key);
    if (!wrapper) {
      wrapper = (...args: unknown[]) => (latest.current[key] as (...a: unknown[]) => unknown)(...args);
      wrappers.current.set(key, wrapper);
    }
    out[key] = wrapper;
  }
  return out as P;
}
