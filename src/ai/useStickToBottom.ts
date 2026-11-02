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

  const observer = useRef<ResizeObserver | null>(null);
  const observed = useRef(new Set<Element>());
  const previousChanged = useRef<readonly unknown[] | null>(null);

  const toBottom = useCallback((smooth: boolean) => {
    const el = box.current;
    if (!el) return;
    el.scrollTo?.({ top: el.scrollHeight, behavior: smooth && !window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "smooth" : "auto" });
  }, []);

  const measureAway = useCallback(() => {
    const el = box.current;
    setAway(!!el && el.scrollHeight > el.clientHeight && el.scrollHeight - el.scrollTop - el.clientHeight > NEAR);
  }, []);

  // One observer per mounted box, including when history replaces the conversation.
  const ref = useCallback((el: HTMLDivElement | null) => {
    observer.current?.disconnect();
    observer.current = null;
    observed.current.clear();
    previousChanged.current = null;
    box.current = el;
    pinned.current = true;
    intentUp.current = false;
    previousTop.current = 0;
    setAway(false);
    if (!el) return;
    observer.current = new ResizeObserver(() => {
      if (pinned.current) toBottom(false);
      measureAway();
    });
    observer.current.observe(el);
    for (const child of el.children) {
      observer.current.observe(child);
      observed.current.add(child);
    }
    toBottom(false);
    measureAway();
  }, [toBottom, measureAway]);

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

  // Reconcile subscriptions only after content or child-count changes. Existing children keep
  // their subscriptions, so unrelated renders do not trigger initial resize notifications.
  useLayoutEffect(() => {
    const el = box.current;
    const current = observer.current;
    if (!el || !current) return;
    if (previousChanged.current && changed.every((value, i) => Object.is(value, previousChanged.current![i]))
      && observed.current.size === el.children.length) return;
    previousChanged.current = changed;
    const children = new Set(el.children);
    for (const child of observed.current) {
      if (!children.has(child)) current.unobserve(child);
    }
    for (const child of children) {
      if (!observed.current.has(child)) current.observe(child);
    }
    observed.current = children;
  });

  const jump = (smooth = true) => {
    pinned.current = true;
    intentUp.current = false;
    toBottom(smooth);
    measureAway();
  };

  return { ref, onScroll, onWheel, onKeyDown, away, jump };
}
