// @vitest-environment jsdom
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { LayersPanel } from "../shell/LayersPanel";
import { LanguageProvider } from "../i18n";
import { readPanelState } from "./windowState";
import type { LayerInfo } from "../api";

afterEach(() => { cleanup(); localStorage.clear(); });

test("the panel read follows actual flat/folder controls without changing them", () => {
  const layers = ["0", "A-ONE", "A-TWO"].map(name => ({ name, color: [255,0,0], off: false, frozen: false, locked: false, plot: true, linetype: "Continuous", lineweight: -3 }) as unknown as LayerInfo);
  render(<LanguageProvider><div className="middle" data-open-panel="assistant">
    <LayersPanel layers={layers} hidden={new Set()} current="0" onToggle={() => {}} onIsolate={() => {}} onShowAll={() => {}} />
  </div></LanguageProvider>);
  expect(readPanelState([{handle:42,chain:[]}])).toMatchObject({open_panel:"assistant", layers:{view:"flat",current:"0"},selection:[{handle:42,chain:[]}]});
  fireEvent.click(screen.getByRole("checkbox"));
  const before = document.body.innerHTML;
  expect(readPanelState([])).toMatchObject({layers:{view:"folders"}});
  expect(document.body.innerHTML).toBe(before);
  document.querySelector(".middle")!.classList.add("left-off", "right-off");
  expect(readPanelState([])).toMatchObject({open_panel:null,layers:null,left_visible:false,right_visible:false});
});
