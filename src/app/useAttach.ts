/** PDF pages and images attached from their dialogs, the insert point or calibration points a
 *  click in the drawing gives them, and the view fitted to a picture just attached. */
import { useDialogOpen, useDialogState } from "./useDialogs";
import { useState, type RefObject } from "react";
import { attachImage, attachPdf, drawingInfo, type DrawingInfo } from "../api";
import type { AttachImageRequest } from "../shell/AttachImageDialog";
import type { AttachRequest } from "../shell/AttachPdfDialog";
import { newPictureBounds, type Batch } from "../viewport/batch";
import type { Point } from "../viewport/camera";
import type { ViewportHandle } from "../viewport/Viewport";
import type { Pending, T } from "./shared";
import type { Sheets } from "./useSheets";

type Deps = Pick<Sheets, "redraw" | "syncPictures"> & {
  t: T;
  setInfo(info: DrawingInfo): void;
  setNotice(notice: string | null): void;
  viewport: RefObject<ViewportHandle | null>;
  batchHeld: RefObject<Batch | null>;
};

export function useAttach({ t, setInfo, setNotice, viewport, batchHeld, redraw, syncPictures }: Deps) {
  const [attachOpen, setAttachOpen] = useDialogOpen("attach");
  const [, setAttachImageOpen] = useDialogOpen("attachImage");
  /** A click the window waits for (see `Pending`), the insert point it gave, and the two
   *  calibration points once both are picked. */
  const [pending, setPending] = useState<Pending | null>(null);
  const [pickedInsert, setPickedInsert] = useState<Point | null>(null);
  const [calibration, setCalibration] = useDialogState("calibration");

  /** Attaches a PDF page from the dialog, then draws it: an underlay's picture is fetched before
   *  the dialog closes, so its progress shows there. Errors go to the dialog. */
  async function attachPdfPage(r: AttachRequest) {
    const before = batchHeld.current;
    const added = await attachPdf(r.path, r.page, r.at, r.scale, r.mode);
    setInfo(await drawingInfo());
    await redraw();
    const view = viewport.current;
    if (r.mode === "underlay" && view) {
      showNewPicture(before);
      await syncPictures(batchHeld.current, view.pxPerUnit(), view.visible());
    } else {
      setNotice(t("attachPdf.imported", { n: added }));
    }
  }

  /** Attaches an image from the dialog, then draws it with its picture. Errors go to the dialog. */
  async function attachImageFile(r: AttachImageRequest) {
    const before = batchHeld.current;
    await attachImage(r.path, r.at, r.width);
    setInfo(await drawingInfo());
    await redraw();
    const view = viewport.current;
    showNewPicture(before);
    if (view) await syncPictures(batchHeld.current, view.pxPerUnit(), view.visible());
  }

  /** Fits the view to the picture the batch on screen has and `before` did not (one just
   *  attached), so it never lands off screen. */
  function showNewPicture(before: Batch | null) {
    const box = newPictureBounds(before, batchHeld.current);
    if (box) viewport.current?.fitTo(box);
  }

  /** Gives `p` to the PDF or image insert point or the calibration waiting for one. False when
   *  none is. */
  function pendingPoint(p: Point): boolean {
    if (pending?.kind === "insert") {
      setPickedInsert(p);
      setPending(null);
    } else if (pending?.kind === "calibrate" && !pending.first) {
      setPending({ kind: "calibrate", first: p });
    } else if (pending?.kind === "calibrate" && pending.first) {
      setCalibration([pending.first, p]);
      setPending(null);
    } else {
      return false;
    }
    return true;
  }

  /** Forgets what waits for a click, the points given, and closes both dialogs (another drawing
   *  is coming). */
  function reset() {
    setPending(null);
    setPickedInsert(null);
    setCalibration(null);
    setAttachOpen(false);
    setAttachImageOpen(false);
  }

  return {
    attachOpen,
    setAttachOpen,
    setAttachImageOpen,
    pending,
    setPending,
    pickedInsert,
    setPickedInsert,
    calibration,
    setCalibration,
    attachPdfPage,
    attachImageFile,
    pendingPoint,
    reset,
  };
}

export type Attach = ReturnType<typeof useAttach>;
