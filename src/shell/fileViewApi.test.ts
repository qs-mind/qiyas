import { afterEach, expect, test, vi } from "vitest";
import { fileViewBytes, fileViewInfo, fileViewPdfPage, fileViewWorkbook } from "../api";
import { setWorkspace } from "../workspaceIpc";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
afterEach(() => { setWorkspace(null); vi.clearAllMocks(); });

test("file viewing uses the exact app-wide contract even with an active drawing", async () => {
  setWorkspace(42);
  const path = "/outputs/takeoff.xlsx";
  const metadata = { kind: "workbook", name: "takeoff.xlsx", size_bytes: 1024, pages: null };
  invoke.mockResolvedValueOnce(metadata);
  expect(await fileViewInfo(path)).toBe(metadata);
  expect(invoke).toHaveBeenLastCalledWith("file_view_info", { path });
  const workbook = { sheets: [] };
  invoke.mockResolvedValueOnce(workbook);
  expect(await fileViewWorkbook(path)).toBe(workbook);
  expect(invoke).toHaveBeenLastCalledWith("file_view_workbook", { path });
  const bytes = new ArrayBuffer(16);
  invoke.mockResolvedValueOnce(bytes);
  expect(await fileViewPdfPage("/plot.pdf", 3, 2400)).toBe(bytes);
  expect(invoke).toHaveBeenLastCalledWith("file_view_pdf_page", { path: "/plot.pdf", page: 3, widthPx: 2400 });
  invoke.mockResolvedValueOnce(bytes);
  expect(await fileViewBytes("/notes.txt")).toBe(bytes);
  expect(invoke).toHaveBeenLastCalledWith("file_view_bytes", { path: "/notes.txt" });
});
