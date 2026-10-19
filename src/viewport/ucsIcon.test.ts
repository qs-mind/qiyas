import { expect, test } from "vitest";
import type { Camera } from "./camera";
import { UCS_ICON_MARGIN, ucsIconPlace } from "./ucsIcon";

/** 800 x 600 pixels, one drawing unit a pixel, centred on (cx, cy). */
const camera = (cx: number, cy: number): Camera => ({ cx, cy, scale: 1, width: 800, height: 600 });

test("on screen, the icon sits on 0,0 and marks it", () => {
  // 0,0 is 100 px left and 100 px below the centre.
  expect(ucsIconPlace(camera(100, 100), null)).toEqual({ x: 300, y: 400, angle: -0, atOrigin: true });
});

test("off screen, or too near an edge for the whole icon, it parks bottom left", () => {
  const corner = { x: UCS_ICON_MARGIN, y: 600 - UCS_ICON_MARGIN, atOrigin: false };
  expect(ucsIconPlace(camera(5000, 5000), null)).toMatchObject(corner);
  // 0,0 on screen, 20 px under the top edge: the Y arm would be cut off.
  expect(ucsIconPlace(camera(0, -280), null)).toMatchObject(corner);
  // 0,0 on screen, 20 px from the right edge: the X arm would be cut off.
  expect(ucsIconPlace(camera(-380, 0), null)).toMatchObject(corner);
});

test("a UCS moves it to its own origin and turns it with its x axis", () => {
  const ucs = { origin: [50, 0] as [number, number], x_axis: [0, 1] as [number, number] };
  const place = ucsIconPlace(camera(0, 0), ucs);
  expect(place.atOrigin).toBe(true);
  expect(place.x).toBe(450);
  expect(place.y).toBe(300);
  // X along world +y: a quarter turn anticlockwise, -90 degrees as CSS turns.
  expect(place.angle).toBeCloseTo(-90);
  // Parked, it still shows the directions.
  expect(ucsIconPlace(camera(9000, 0), ucs).angle).toBeCloseTo(-90);
});
