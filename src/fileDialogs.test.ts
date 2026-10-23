// @vitest-environment jsdom
import { beforeEach, expect, test, vi } from "vitest";
import { open, save } from "./fileDialogs";

const native = vi.hoisted(() => ({ open: vi.fn(), save: vi.fn(), documents: vi.fn(), drawing: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: native.open, save: native.save }));
vi.mock("@tauri-apps/api/path", () => ({ documentDir: native.documents }));
vi.mock("./api", () => ({ drawingInfo: native.drawing }));

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  native.drawing.mockRejectedValue("No drawing is open");
  native.documents.mockResolvedValue("/home/qs/Documents");
  native.open.mockResolvedValue(null);
  native.save.mockResolvedValue(null);
});

test("an untitled drawing starts in Documents and keeps only the offered name", async () => {
  native.drawing.mockResolvedValue({ path: "/app/crates/qiyas-desktop/Untitled-1.dwg", untitled: true });
  await save({ defaultPath: "/app/crates/qiyas-desktop/Untitled-1.dwg" });
  expect(native.save).toHaveBeenCalledWith({ defaultPath: "/home/qs/Documents/Untitled-1.dwg" });
});

test("open and save use the drawing on screen, including after a tab switch", async () => {
  localStorage.setItem("qiyas.files.lastFolder", "/old/");
  native.drawing.mockResolvedValue({ path: "/plans/a.dwg", untitled: false });
  const filters = [{ name: "PDF", extensions: ["pdf"] }];
  await open({ filters });
  expect(native.open).toHaveBeenCalledWith({ defaultPath: "/plans/", filters });
  native.drawing.mockResolvedValue({ path: "/other/b.dxf", untitled: false });
  await save({ defaultPath: "/plans/a.pdf", filters });
  expect(native.save).toHaveBeenCalledWith({ defaultPath: "/other/a.pdf", filters });
  expect(native.documents).not.toHaveBeenCalled();
});

test("a chosen file remembers its folder across dialogs; cancelling keeps it", async () => {
  native.open.mockResolvedValueOnce("/picked/block.dwg");
  await open();
  await save({ defaultPath: "export.xlsx" });
  expect(native.save).toHaveBeenCalledWith({ defaultPath: "/picked/export.xlsx" });
  await open();
  expect(native.open).toHaveBeenLastCalledWith({ defaultPath: "/picked/" });
});

test("a relative remembered folder cannot fall back to the process directory", async () => {
  localStorage.setItem("qiyas.files.lastFolder", "crates/qiyas-desktop");
  await open();
  expect(native.open).toHaveBeenCalledWith({ defaultPath: "/home/qs/Documents" });
});

test("an unavailable Documents folder fails before a picker can use the working directory", async () => {
  native.documents.mockRejectedValueOnce(new Error("Documents is unavailable"));
  await expect(save({ defaultPath: "Untitled-1.dwg" })).rejects.toThrow("Documents is unavailable");
  expect(native.save).not.toHaveBeenCalled();
});

test("Windows folders and a multiple selection keep their platform spelling", async () => {
  native.drawing.mockResolvedValue({ path: "C:\\Plans\\villa.dwg", untitled: false });
  native.open.mockResolvedValue(["D:\\Images\\one.png", "D:\\Images\\two.png"]);
  await open({ multiple: true });
  expect(native.open).toHaveBeenCalledWith({ multiple: true, defaultPath: "C:\\Plans\\" });
  native.drawing.mockRejectedValue("No drawing is open");
  await save({ defaultPath: "villa.pdf" });
  expect(native.save).toHaveBeenCalledWith({ defaultPath: "D:\\Images\\villa.pdf" });
});

test("only the shared helper imports the native dialog plugin", () => {
  const sources = import.meta.glob("./**/*.{ts,tsx}", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
  const importsPlugin = /(?:\bfrom\s*|\bimport\s*(?:\(\s*)?|\brequire\s*\(\s*)["']@tauri-apps\/plugin-dialog["']/;
  const bypasses = Object.entries(sources)
    .filter(([path, source]) => path !== "./fileDialogs.ts" && importsPlugin.test(source))
    .map(([path]) => path);
  expect(bypasses).toEqual([]);
});
