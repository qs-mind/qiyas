import { gridNavigation, useCompositeRoot } from "@base-ui/react/internals/composite";
import { DirectionContext } from "@base-ui/react/internals/direction-context";
import { useContext, useLayoutEffect, useRef, useState, type KeyboardEvent, type FocusEvent, type RefObject } from "react";
import { flushSync } from "react-dom";
import type { BaseUIEvent } from "@base-ui/react/types";

/** Compatibility for callers that ask for an index: one row of Base UI's grid navigator. */
export function nextIndex(key: string, i: number, n: number, rtl = false): number | null {
  if (n <= 0) return null;
  if (key === "Home") return 0;
  if (key === "End") return n - 1;
  if (key !== "ArrowLeft" && key !== "ArrowRight") return null;
  return gridNavigation({ cols: n })({
    // Horizontal navigation reads only the key; there is no DOM event to cancel here.
    event: { key } as KeyboardEvent,
    elementsRef: { current: Array<null>(n).fill(null) },
    highlightedIndex: i, minIndex: 0, maxIndex: n - 1,
    orientation: "both", loopFocus: true, disabledIndices: [], rtl,
  });
}

/** Bridge the existing DOM-based callers to the same composite used by Tabs and Toolbar.
 *  Base UI chooses the next item. Focus stays synchronous, and tabs reset on leaving the strip.
 *  The exported hook contract also supports callers with plain native tab buttons. */
function useCompositeRoving<T extends HTMLElement>(selector: string, selected: number | null, externalRef?: RefObject<T | null>, count?: number) {
  const ownRef = useRef<T>(null);
  const ref = externalRef ?? ownRef;
  const [index, setIndex] = useState(Math.max(selected ?? 0, 0));
  const current = count !== undefined && index >= count ? Math.max(selected ?? 0, 0) : index;
  const elements = useRef<HTMLElement[]>([]);
  const inherited = useContext(DirectionContext);
  const composite = useCompositeRoot({
    rootRef: ref,
    direction: inherited?.direction ?? (document.documentElement.dir === "rtl" ? "rtl" : "ltr"),
    orientation: "horizontal",
    enableHomeAndEndKeys: true,
    loopFocus: true,
    modifierKeys: ["Shift", "Control", "Alt", "Meta"],
    highlightedIndex: current,
    onHighlightedIndexChange(next) {
      setIndex(next);
      elements.current[next]?.focus();
    },
  });
  const collect = () => Array.from(ref.current?.querySelectorAll<HTMLElement>(selector) ?? []);
  useLayoutEffect(() => {
    if (selected !== null && !ref.current?.contains(document.activeElement)) setIndex(Math.max(selected, 0));
  }, [selected, ref, count]);

  return {
    props: {
      ref: composite.props.ref,
      onFocus() {
        const at = collect().indexOf(document.activeElement as HTMLElement);
        if (at >= 0) setIndex(at);
      },
      onBlur(e: FocusEvent) {
        if (selected !== null && !ref.current?.contains(e.relatedTarget)) setIndex(Math.max(selected, 0));
      },
      onKeyDown(e: KeyboardEvent & Partial<Pick<BaseUIEvent<KeyboardEvent>, "preventBaseUIHandler">>) {
        const all = collect();
        const at = all.indexOf(e.target as HTMLElement);
        if (at < 0) return;
        // Base UI tabs already activate Space. Native legacy callers need the same activation.
        if (e.key === " ") {
          if (selected !== null && !e.defaultPrevented) { e.preventDefault(); all[at].click(); }
          return;
        }
        if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) return;
        if (at !== current) flushSync(() => setIndex(at));
        elements.current = all;
        composite.elementsRef.current = all;
        composite.relayKeyboardEvent(e);
        // We focused in the callback above. Do not schedule a second focus after a later blur.
        composite.elementsRef.current = [];
        e.preventDefault();
        e.preventBaseUIHandler?.();
      },
      onKeyUp(e: KeyboardEvent) {
        if (selected !== null && e.key === " ") e.preventDefault();
      },
    },
    tabIndex: (i: number) => i === current ? 0 : -1,
  };
}

export function useRoving<T extends HTMLElement = HTMLDivElement>(n: number, active: number, ref?: RefObject<T | null>) {
  return useCompositeRoving('[role="tab"]', active >= 0 && active < n ? active : 0, ref, n);
}

export function useToolbarRoving<T extends HTMLElement = HTMLDivElement>() {
  return useCompositeRoving<T>('button:not([role="menuitem"]):not(:disabled)', null);
}
