// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { fileViewBytes, fileViewInfo, fileViewPdfPage, fileViewWorkbook } from "../api";
import en from "../i18n/en.json";
import { FileViewer, parseCsv } from "./FileViewer";

vi.mock("../api", () => ({ fileViewBytes: vi.fn(), fileViewInfo: vi.fn(), fileViewPdfPage: vi.fn(), fileViewWorkbook: vi.fn() }));
const intersections: { target: Element; notify(visible: boolean): void }[] = [];
const putImageData = vi.fn();
beforeEach(() => {
  vi.resetAllMocks();
  intersections.length = 0;
  vi.stubGlobal("IntersectionObserver", class {
    constructor(private callback: IntersectionObserverCallback) {}
    observe(target: Element) { intersections.push({ target, notify: visible => this.callback([{ isIntersecting: visible } as IntersectionObserverEntry], this as unknown as IntersectionObserver) }); }
    disconnect() {}
  });
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(624);
  vi.stubGlobal("devicePixelRatio", 2);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    createImageData: (width: number, height: number) => ({ data: new Uint8ClampedArray(width * height * 4) }), putImageData,
  } as unknown as CanvasRenderingContext2D);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
const info = (kind: "pdf" | "workbook" | "text" | "image", name: string, pages: number | null = null) => ({ kind, name, pages, size_bytes: 128 });
const bytes = (text: string) => new TextEncoder().encode(text).buffer;

test("three PDF pages have labels and zoom controls; only nearby pages render at capped device resolution", async () => {
  vi.mocked(fileViewInfo).mockResolvedValue(info("pdf", "plot.pdf", 3));
  const picture = new ArrayBuffer(16);
  const header = new DataView(picture);
  header.setUint32(0, 1, true); header.setUint32(4, 2, true);
  new Uint8Array(picture, 8).set([255, 0, 0, 255, 0, 0, 255, 255]);
  vi.mocked(fileViewPdfPage).mockResolvedValue(picture);
  render(<FileViewer path="/plot.pdf" />);
  expect(await screen.findByText("Page 1 of 3")).toBeTruthy();
  expect(screen.getByText("Page 2 of 3")).toBeTruthy();
  expect(screen.getByText("Page 3 of 3")).toBeTruthy();
  expect(fileViewPdfPage).not.toHaveBeenCalled();
  act(() => intersections[0].notify(true));
  await waitFor(() => expect(fileViewPdfPage).toHaveBeenLastCalledWith("/plot.pdf", 1, 1200));
  await waitFor(() => expect(putImageData).toHaveBeenCalled());
  expect(Array.from(putImageData.mock.calls[0][0].data)).toEqual([255, 0, 0, 255, 0, 0, 255, 255]);
  fireEvent.click(screen.getByRole("button", { name: en["fileView.zoomIn"] }));
  await waitFor(() => expect(fileViewPdfPage).toHaveBeenLastCalledWith("/plot.pdf", 1, 1500));
  fireEvent.click(screen.getByRole("button", { name: en["fileView.zoomOut"] }));
  await waitFor(() => expect(fileViewPdfPage).toHaveBeenLastCalledWith("/plot.pdf", 1, 1200));
  for (let i = 0; i < 6; i++) fireEvent.click(screen.getByRole("button", { name: en["fileView.zoomIn"] }));
  await waitFor(() => expect(fileViewPdfPage).toHaveBeenLastCalledWith("/plot.pdf", 1, 2400));
  fireEvent.click(screen.getByRole("button", { name: en["fileView.fitWidth"] }));
  await waitFor(() => expect(fileViewPdfPage).toHaveBeenLastCalledWith("/plot.pdf", 1, 1200));
  act(() => { intersections[0].notify(false); intersections[2].notify(true); });
  await waitFor(() => expect(fileViewPdfPage).toHaveBeenLastCalledWith("/plot.pdf", 3, 1200));
  expect(screen.queryByRole("img", { name: "Page 1 of 3" })).toBeNull();
  expect(vi.mocked(fileViewPdfPage).mock.calls.every(call => call[1] !== 2)).toBe(true);
});

test("workbook hides record sheets, gives Arabic cells their own direction, and switches sheets", async () => {
  vi.mocked(fileViewInfo).mockResolvedValue(info("workbook", "takeoff.xlsx"));
  const sheet = { hidden: false, rows_total: 10, cols_total: 2, truncated: true };
  vi.mocked(fileViewWorkbook).mockResolvedValue({ sheets: [
    { ...sheet, name: "الحصر", rows: [["الوصف", "الكمية"], ["خرسانة", "12"]] },
    { ...sheet, name: "Qiyas record", hidden: true, rows: [["private record"]] },
    { ...sheet, name: "Summary", rows: [["Total"], ["99"]], truncated: false },
  ] });
  render(<FileViewer path="/takeoff.xlsx" />);
  expect(await screen.findByRole("tab", { name: "الحصر" })).toBeTruthy();
  expect(screen.queryByText("Qiyas record")).toBeNull();
  expect(screen.queryByText("private record")).toBeNull();
  expect(screen.getByRole("cell", { name: "خرسانة" }).getAttribute("dir")).toBe("auto");
  expect(screen.getByRole("columnheader", { name: "الوصف" }).getAttribute("dir")).toBe("auto");
  expect(screen.getByRole("note").textContent).toContain("2 of 10 rows");
  fireEvent.click(screen.getByRole("tab", { name: "Summary" }));
  expect(await screen.findByRole("cell", { name: "99" })).toBeTruthy();
});

test("large workbook sheets mount a limited number of rows", async () => {
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (this: HTMLElement) { return this.tagName === "TR" ? 36 : 600; });
  vi.mocked(fileViewInfo).mockResolvedValue(info("workbook", "large.xlsm"));
  vi.mocked(fileViewWorkbook).mockResolvedValue({ sheets: [{ name: "Many", hidden: false, truncated: false,
    rows_total: 1001, cols_total: 1, rows: [["Name"], ...Array.from({ length: 1000 }, (_, i) => [`Row ${i}`])] }] });
  render(<FileViewer path="/large.xlsm" />);
  expect(await screen.findByRole("table")).toBeTruthy();
  expect(await screen.findByRole("cell", { name: "Row 0" })).toBeTruthy();
  expect(screen.getAllByRole("row").length).toBeLessThan(100);
  expect(screen.queryByText("Row 999")).toBeNull();
});

test("CSV is a table with quoted commas, escaped quotes, multiline cells and Arabic direction", async () => {
  vi.mocked(fileViewInfo).mockResolvedValue(info("text", "items.csv"));
  vi.mocked(fileViewBytes).mockResolvedValue(bytes('Name,Note\r\n"خرسانة","first, second\n""quoted"""\r\n'));
  render(<FileViewer path="/items.csv" />);
  expect(await screen.findByRole("table", { name: "items.csv" })).toBeTruthy();
  expect(screen.getByRole("cell", { name: "خرسانة" }).getAttribute("dir")).toBe("auto");
  expect(screen.getByRole("cell", { name: /first, second\s+"quoted"/ }).textContent).toBe('first, second\n"quoted"');
  expect(parseCsv("a,b\r\n1,\r\n")).toEqual([["a", "b"], ["1", ""]]);
});

test.each(["txt", "json"])("%s preserves text in a pre with automatic direction", async extension => {
  vi.mocked(fileViewInfo).mockResolvedValue(info("text", `notes.${extension}`));
  vi.mocked(fileViewBytes).mockResolvedValue(bytes('نص <script>example</script>'));
  render(<FileViewer path={`/notes.${extension}`} />);
  const text = await screen.findByText('نص <script>example</script>');
  expect(text.tagName).toBe("PRE"); expect(text.getAttribute("dir")).toBe("auto");
});

test("Markdown uses the existing renderer", async () => {
  vi.mocked(fileViewInfo).mockResolvedValue(info("text", "notes.md"));
  vi.mocked(fileViewBytes).mockResolvedValue(bytes("# Notes\n\n**Ready**"));
  render(<FileViewer path="/notes.md" />);
  expect(await screen.findByRole("heading", { name: "Notes" })).toBeTruthy();
  expect(screen.getByText("Ready").closest("strong")).not.toBeNull();
});

test("pictures toggle actual size and release their URL on close", async () => {
  const revoke = vi.fn();
  vi.stubGlobal("URL", { createObjectURL: vi.fn(() => "blob:picture"), revokeObjectURL: revoke });
  vi.mocked(fileViewInfo).mockResolvedValue(info("image", "picture.png"));
  vi.mocked(fileViewBytes).mockResolvedValue(new ArrayBuffer(4));
  const { unmount } = render(<FileViewer path="/picture.png" />);
  fireEvent.click(await screen.findByRole("button", { name: en["fileView.actualSize"] }));
  expect(screen.getByRole("button", { name: en["fileView.fitWidth"] }).getAttribute("aria-pressed")).toBe("true");
  unmount(); expect(revoke).toHaveBeenCalledWith("blob:picture");
});

test("errors use uiError and late reads do not replace a newly selected file", async () => {
  let resolve!: (value: Awaited<ReturnType<typeof fileViewInfo>>) => void;
  vi.mocked(fileViewInfo).mockImplementationOnce(() => new Promise(done => { resolve = done; }));
  const { rerender } = render(<FileViewer path="/old.pdf" />);
  expect(screen.getByRole("status").textContent).toBe(en["fileView.loading"]);
  vi.mocked(fileViewInfo).mockRejectedValueOnce(new Error("file not found"));
  rerender(<FileViewer path="/missing.pdf" />);
  expect((await screen.findByRole("alert")).textContent).toBe(en["err.missing"]);
  await act(async () => resolve(info("pdf", "old.pdf", 3)));
  expect(screen.queryByText("Page 1 of 3")).toBeNull();
});
