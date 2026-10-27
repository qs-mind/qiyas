/** Where the pointer is over the drawing, outside React's state. A pointer move used to be a state
 *  change in the app's root hook, which rendered the whole window (the ribbon's 30 tooltips, Mizan's
 *  thread, the tables) on every move: 6 ms of script per move in the production build, 21 ms in the
 *  dev build (W2). Now a move is a field write and a notice to whoever listens: the status bar
 *  always, the app itself only while a tool needs the cursor for what it draws (`useTool`). */
import { useSyncExternalStore } from "react";
import type { Snap } from "../api";
import type { Point } from "../viewport/camera";

export type RawPointer = { p: Point; wpp: number } | null;
export type HoverSnap = { snap: Snap; wpp: number } | null;

export class PointerStore {
  /** The cursor in drawing coordinates as it is, and drawing units per CSS pixel there. */
  raw: RawPointer = null;
  /** The object snap under the cursor, and the zoom it was found at. */
  hover: HoverSnap = null;
  private version = 0;
  private readonly listeners = new Set<() => void>();
  /** Set on every render of `useTool`: where a click would place the next point now, and the snap
   *  used for it, from the latest `raw` and `hover`. */
  derive: () => { cursor: Point | null; snap: Snap | null } = () => ({ cursor: null, snap: null });

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => void this.listeners.delete(listener);
  };
  /** Changes with every move or snap. */
  getVersion = () => this.version;

  setRaw(raw: RawPointer) {
    this.raw = raw;
    this.notify();
  }
  setHover(hover: HoverSnap) {
    if (this.hover === hover) return;
    this.hover = hover;
    this.notify();
  }
  private notify() {
    this.version += 1;
    this.listeners.forEach((listener) => listener());
  }
}

const NO_LISTENER = () => () => {};
const NO_VERSION = () => 0;

/** The cursor and the snap under it, live: the component that calls this renders on every move, so
 *  keep it small. With no store, null. */
export function usePointer(store?: PointerStore) {
  useSyncExternalStore(store?.subscribe ?? NO_LISTENER, store?.getVersion ?? NO_VERSION);
  return store ? store.derive() : null;
}
