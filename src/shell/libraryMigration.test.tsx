// @vitest-environment jsdom
import { DirectionProvider } from "@base-ui/react/direction-provider";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { FileTabs } from "./FileTabs";
import { parseCsv } from "./FileViewer";

afterEach(cleanup);

test("file tabs mirror three stops in Arabic without activating on arrow focus", () => {
  const onSelect = vi.fn();
  render(
    <DirectionProvider direction="rtl">
      <FileTabs
        tabs={[1, 2, 3].map((id) => ({ id, title: `${id}.dwg`, path: `/${id}.dwg`, dirty: false }))}
        active={1}
        onSelect={onSelect}
        onClose={() => {}}
      />
    </DirectionProvider>,
  );
  const tabs = screen.getAllByRole("tab");
  tabs[0].focus();
  fireEvent.keyDown(tabs[0], { key: "ArrowLeft" });
  expect(document.activeElement).toBe(tabs[1]);
  expect(onSelect).not.toHaveBeenCalled();
  fireEvent.keyDown(tabs[1], { key: "Home" });
  expect(document.activeElement).toBe(tabs[0]);
  fireEvent.keyDown(tabs[0], { key: "End" });
  expect(document.activeElement).toBe(tabs[2]);
  fireEvent.keyDown(tabs[2], { key: " " });
  fireEvent.keyUp(tabs[2], { key: " " });
  expect(onSelect.mock.calls).toEqual([[3]]);
});

test("CSV removes a BOM and preserves codes, large numbers and booleans as text", () => {
  expect(parseCsv("\ufeffcode,qty,flag\r\n001,9007199254740993,true\r\n")).toEqual([
    ["code", "qty", "flag"], ["001", "9007199254740993", "true"],
  ]);
});

test("CSV preserves blank records and empty fields, excluding the final record separator", () => {
  expect(parseCsv("a,b\n\n,\n")).toEqual([["a", "b"], [""], ["", ""]]);
  expect(parseCsv("")).toEqual([]);
});

test("CSV keeps quoted commas, doubled quotes and embedded newlines", () => {
  expect(parseCsv('name,note\n"a,b","line 1\nline 2 with ""quotes"""\n')).toEqual([
    ["name", "note"], ["a,b", 'line 1\nline 2 with "quotes"'],
  ]);
});

test("CSV still displays recovered text from an unterminated quoted field", () => {
  expect(parseCsv('code,"unfinished\n')).toEqual([["code", "unfinished\n"]]);
});
