// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { RibbonLayout } from "../api";
import { Ribbon } from "./Ribbon";

afterEach(cleanup);

test("a registry command added in Customize starts typed prompts without a window handler", () => {
  const layout: RibbonLayout = {
    tabs: [{
      id: "registry",
      label: "Registry",
      groups: [{ id: "new", label: "New commands", buttons: [{ kind: "command", command: "WIDGET", label: "WIDGET" }] }],
    }],
  };
  const onCommandLine = vi.fn();
  const onTool = vi.fn();
  render(<Ribbon active="select" onOpen={() => {}} onZoomExtents={() => {}} onTool={onTool}
    layout={layout} onCommandLine={onCommandLine} />);
  const button = screen.getByRole("button", { name: "WIDGET" });
  expect(button.querySelector("svg rect")).not.toBeNull();
  fireEvent.click(button);
  expect(onCommandLine).toHaveBeenCalledWith("WIDGET");
  expect(onTool).not.toHaveBeenCalled();
});
