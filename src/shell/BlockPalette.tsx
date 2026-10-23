import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import type { BlockInfo } from "../api";
import { useT } from "../i18n";

type Props = {
  /** The drawing's blocks. */
  load(): Promise<BlockInfo[]>;
  /** A picture of the block (a URL the palette may show), drawn by the core. */
  thumbnail(name: string): Promise<string>;
  /** Starts placing a reference to `block` (INSERT). */
  onInsert(block: BlockInfo): void;
  /** Writes the block to a drawing of its own (WBLOCK). */
  onWrite(name: string): void;
  /** Puts a button for the block on the tool palette. */
  onAddToPalette(name: string): void;
  onClose(): void;
};

/** The blocks in the drawing, each with its picture: click to insert, or write it to a file. */
export function BlockPalette({ load, thumbnail, onInsert, onWrite, onAddToPalette, onClose }: Props) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [blocks, setBlocks] = useState<BlockInfo[] | null>(null);
  const [pictures, setPictures] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    let gone = false;
    load().then(
      async (found) => {
        if (gone) return;
        setBlocks(found);
        for (const b of found) {
          try {
            const url = await thumbnail(b.name);
            if (!gone) setPictures((p) => ({ ...p, [b.name]: url }));
          } catch {
            // A block that can't be drawn just shows its name.
          }
        }
      },
      (e) => !gone && setError(uiError(e)),
    );
    return () => {
      gone = true;
      opener?.focus();
    };
  }, []);

  /** Closes the palette, then does `then`. */
  const leave = (then: () => void) => {
    dialog.current?.close();
    then();
  };

  return (
    <dialog
      ref={dialog}
      className="settings block-palette"
      aria-labelledby="block-palette-title"
      onClose={onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <h2 id="block-palette-title" className="heading">
        {t("block.paletteTitle")}
      </h2>
      {blocks?.length === 0 && <p>{t("block.none")}</p>}
      <ul className="block-tiles">
        {blocks?.map((b) => (
          <li key={b.name}>
            <button
              type="button"
              className="block-tile"
              aria-label={t("block.insertOne", { name: b.name })}
              onClick={() => leave(() => onInsert(b))}
            >
              {pictures[b.name] ? <img src={pictures[b.name]} alt={b.name} /> : <span className="block-blank" />}
              <span dir="auto">{b.name}</span>
            </button>
            <button
              type="button"
              aria-label={t("block.writeOne", { name: b.name })}
              title={t("block.write")}
              onClick={() => leave(() => onWrite(b.name))}
            >
              {t("block.write")}
            </button>
            <button
              type="button"
              aria-label={t("block.addOne", { name: b.name })}
              title={t("block.addToPalette")}
              onClick={() => onAddToPalette(b.name)}
            >
              +
            </button>
          </li>
        ))}
      </ul>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="buttons">
        <button type="button" onClick={() => dialog.current?.close()}>
          {t("block.close")}
        </button>
      </div>
    </dialog>
  );
}
