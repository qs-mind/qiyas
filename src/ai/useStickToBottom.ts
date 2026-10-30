import { useCallback, useLayoutEffect, useRef, useState, type KeyboardEvent, type WheelEvent } from "react";

/** How close to the end, in pixels, still counts as being at it. */
const NEAR = 32;

/** A scrolling thread that follows its newest message: while the reader is at the bottom, what is
 *  added (a streamed word, a new card) scrolls into view; once they have scrolled up to read, it
 *  leaves them where they are, and `away` says they are not at the bottom, for a "Jump to latest" button.
 *  `jump` goes to the bottom and follows again. `changed` lists what changes when the content does
 *  (always the same number of things).
 *  Put `ref`, `onScroll`, `onWheel` and `onKeyDown` on the scrolling box. */
export function useStickToBottom(changed: readonly unknown[]) {
  const box = useRef<HTMLDivElement | null>(null);
  const pinned = useRef(true);
  const previousTop = useRef(0);
  const intentUp = useRef(false);
  const [away, setAway] = useState(false);

  // A new box (the conversation came back from the log) starts at the bottom again.
  const ref = useCallback((el: HTMLDivElement | null) => {
    box.current = el;
    pinned.current = true;
    intentUp.current = false;
    previousTop.current = 0;
    setAway(false);
  }, []);

  const toBottom = (smooth: boolean) => {
    const el = box.current;
    if (!el) return;
    el.scrollTo?.({ top: el.scrollHeight, behavior: smooth && !window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "smooth" : "auto" });
  };

  const measureAway = () => {
    const el = box.current;
    setAway(!!el && el.scrollHeight > el.clientHeight && el.scrollHeight - el.scrollTop - el.clientHeight > NEAR);
  };

  const onScroll = () => {
    const el = box.current;
    if (!el) return;
    const movedUp = el.scrollTop < previousTop.current;
    const movedDown = el.scrollTop > previousTop.current;
    if (movedUp) intentUp.current = true;
    if (movedDown) intentUp.current = false;
    previousTop.current = el.scrollTop;
    pinned.current = !intentUp.current && el.scrollHeight - el.scrollTop - el.clientHeight <= NEAR;
    measureAway();
  };

  const leaveBottom = () => {
    intentUp.current = true;
    pinned.current = false;
    measureAway();
  };
  const onWheel = (event: WheelEvent<HTMLDivElement>) => {
    if (event.deltaY < 0) {
      previousTop.current = box.current?.scrollTop ?? 0;
      leaveBottom();
    }
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const target = event.target;
    if (target instanceof HTMLElement && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName))) return;
    if (["ArrowUp", "PageUp", "Home"].includes(event.key) || (event.key === " " && event.shiftKey)) {
      previousTop.current = box.current?.scrollTop ?? 0;
      leaveBottom();
    }
  };

  // After the content changed, before it is painted: follow it if the reader is at the bottom.
  useLayoutEffect(() => {
    if (pinned.current) toBottom(false);
    measureAway();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, changed);

  // Both the viewport and its contents can resize without a transcript update (e.g. an image).
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const observer = new ResizeObserver(() => {
      if (pinned.current) toBottom(false);
      measureAway();
    });
    observer.observe(el);
    for (const child of el.children) observer.observe(child);
    return () => observer.disconnect();
  });

  const jump = (smooth = true) => {
    pinned.current = true;
    intentUp.current = false;
    toBottom(smooth);
    measureAway();
  };

  return { ref, onScroll, onWheel, onKeyDown, away, jump };
}
