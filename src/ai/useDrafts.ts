import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type { PdfAttachment } from "../api";
import type { Picture } from "./pictures";

/** What waits in Mizan's box for one drawing: the words, the pictures and PDFs to send with them,
 *  and the options chosen on Mizan's open questions (by question id). */
export type Draft = { text: string; pictures: Picture[]; documents: PdfAttachment[]; picks: Record<string, string> };

const EMPTY: Draft = { text: "", pictures: [], documents: [], picks: {} };

/** In a text box WebKitGTK raises an alef carrying tanween ("مستطيلاً") above the line whatever the
 *  font (its text keeps every space, which takes another path through the shaper). The same word with
 *  the tanween first ("مستطيلًا") sits on the line. */
export function boxArabic(text: string) {
  return text.replace(/اً/g, "ًا");
}

/** One draft per drawing (workspace), so switching tabs keeps what was being written; a closed
 *  drawing's draft goes with it. */
export function useDrafts(workspace: unknown) {
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const key = String(workspace ?? "lobby");
  const draft = drafts[key] ?? EMPTY;

  useEffect(() => {
    let gone = false;
    let off: (() => void) | undefined;
    void listen<number>("workspace:closed", ({ payload }) => {
      setDrafts(all => { const kept = { ...all }; delete kept[String(payload)]; return kept; });
    }).then(unlisten => { if (gone) unlisten(); else off = unlisten; }).catch(() => {});
    return () => { gone = true; off?.(); };
  }, []);

  function edit(change: (before: Draft) => Partial<Draft>) {
    setDrafts(all => {
      const before = all[key] ?? EMPTY;
      return { ...all, [key]: { ...before, ...change(before) } };
    });
  }

  return {
    draft,
    setText: (text: string) => edit(() => ({ text: boxArabic(text) })),
    setPictures: (next: (old: Picture[]) => Picture[]) => edit(before => ({ pictures: next(before.pictures) })),
    setDocuments: (next: (old: PdfAttachment[]) => PdfAttachment[]) => edit(before => ({ documents: next(before.documents) })),
    setPicks: (picks: Record<string, string>) => edit(() => ({ picks })),
    /** After a message went: the box starts again empty. */
    clear: () => edit(() => EMPTY),
  };
}

export type Drafts = ReturnType<typeof useDrafts>;
