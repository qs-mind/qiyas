/** Where the UCS icon goes (AutoCAD's UCSICON ON, ORigin): on the UCS origin (0,0 without a UCS)
 *  while the whole icon fits on the canvas, else parked in the bottom-left corner. Kept apart from
 *  the DOM so it can be tested. */
import type { Ucs } from "../api";
import { worldToScreen, type Camera, type Point } from "./camera";

/** Length of each arm, in CSS pixels. */
export const UCS_ICON_PX = 40;
/** Room kept between the icon and the canvas edge, and the corner it parks in. */
export const UCS_ICON_MARGIN = 12;
/** Room past each arm's tip for its label. */
const LABEL_PX = 12;

export type UcsIconPlace = {
  /** The corner of the L, in CSS pixels from the canvas's top left. */
  x: number;
  y: number;
  /** Screen rotation in degrees (clockwise, as CSS turns): the UCS x axis's angle, negated
   *  because screen y points down. */
  angle: number;
  /** On the origin itself (it then marks it with a square); false when parked in the corner. */
  atOrigin: boolean;
};

export function ucsIconPlace(camera: Camera, ucs: Ucs | null | undefined): UcsIconPlace {
  const [ax, ay] = ucs?.x_axis ?? [1, 0];
  const turn = Math.atan2(ay, ax);
  const angle = (-turn * 180) / Math.PI;
  const [x, y] = worldToScreen(camera, ucs?.origin ?? [0, 0]);
  // The two tips with their labels, turned with the axes (screen y down).
  const reach = UCS_ICON_PX + LABEL_PX;
  const tips: Point[] = [
    [x + reach * Math.cos(turn), y - reach * Math.sin(turn)],
    [x - reach * Math.sin(turn), y - reach * Math.cos(turn)],
  ];
  const m = UCS_ICON_MARGIN;
  const fits = [[x, y] as Point, ...tips].every(
    ([px, py]) => px >= m && px <= camera.width - m && py >= m && py <= camera.height - m,
  );
  if (Number.isFinite(x) && Number.isFinite(y) && fits) return { x, y, angle, atOrigin: true };
  return { x: m, y: camera.height - m, angle, atOrigin: false };
}
