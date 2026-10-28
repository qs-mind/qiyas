import { Tabs } from "@base-ui/react/tabs";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useRef, useState, type RefObject } from "react";
import { fileViewBytes, fileViewInfo, fileViewPdfPage, fileViewWorkbook, type FileViewInfo, type FileViewWorkbook } from "../api";
import { Markdown } from "../ai/Markdown";
import { useLanguage, useT } from "../i18n";
import { uiError } from "../i18n/errors";
import { parsePicture } from "../viewport/batch";
import { DotLoader } from "./DotLoader";

function Loading() {
  const t = useT();
  return <p className="file-status" role="status"><DotLoader />{t("fileView.loading")}</p>;
}

/** A new path owns its requests and releases its pictures when it leaves the column. */
export function FileViewer({ path }: { path: string }) {
  return <FileContent key={path} path={path} />;
}

function FileContent({ path }: { path: string }) {
  const { lang } = useLanguage();
  const [info, setInfo] = useState<FileViewInfo | null>(null);
  const [book, setBook] = useState<FileViewWorkbook | null>(null);
  const [bytes, setBytes] = useState<ArrayBuffer | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    async function read() {
      try {
        const next = await fileViewInfo(path);
        if (cancelled) return;
        setInfo(next);
        if (next.kind === "workbook") {
          const value = await fileViewWorkbook(path);
          if (!cancelled) setBook(value);
        } else if (next.kind !== "pdf") {
          const value = await fileViewBytes(path);
          if (!cancelled) setBytes(value);
        }
      } catch (e) { if (!cancelled) setError(uiError(e, lang)); }
    }
    void read();
    return () => { cancelled = true; };
  }, [path, lang]);
  return <div className="file-viewer">
    {error ? <p className="file-status" role="alert">{error}</p>
      : info?.kind === "pdf" ? <PdfViewer path={path} pages={info.pages ?? 0} />
      : book ? <WorkbookViewer book={book} />
      : bytes && info ? info.kind === "image" ? <ImageViewer bytes={bytes} name={info.name} />
        : <TextViewer bytes={bytes} name={info.name} />
      : <Loading />}
  </div>;
}

function PdfViewer({ path, pages }: { path: string; pages: number }) {
  const t = useT();
  const scroller = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [dpr, setDpr] = useState(window.devicePixelRatio || 1);
  const [zoom, setZoom] = useState(1);
  useEffect(() => {
    const element = scroller.current!;
    const resize = () => {
      setWidth(Math.max(0, element.clientWidth - 24));
      setDpr(window.devicePixelRatio || 1);
    };
    const observer = new ResizeObserver(resize);
    observer.observe(element);
    window.addEventListener("resize", resize);
    resize();
    return () => { observer.disconnect(); window.removeEventListener("resize", resize); };
  }, []);
  const displayWidth = width * zoom;
  const widthPx = Math.max(200, Math.min(2400, Math.round(displayWidth * dpr)));
  return <>
    <div className="file-controls">
      <button type="button" onClick={() => setZoom(z => Math.max(0.25, z - 0.25))} disabled={zoom <= 0.25}>{t("fileView.zoomOut")}</button>
      <button type="button" onClick={() => setZoom(z => Math.min(4, z + 0.25))} disabled={zoom >= 4}>{t("fileView.zoomIn")}</button>
      <button type="button" onClick={() => setZoom(1)}>{t("fileView.fitWidth")}</button>
    </div>
    <div className="file-scroll file-pages" ref={scroller}>
      {pages === 0 && <p className="file-status">{t("fileView.empty")}</p>}
      {Array.from({ length: pages }, (_, i) => <PdfPage key={i} path={path} page={i + 1} pages={pages}
        root={scroller} width={displayWidth} widthPx={widthPx} />)}
    </div>
  </>;
}

function PdfPage({ path, page, pages, root, width, widthPx }: {
  path: string; page: number; pages: number; root: RefObject<HTMLDivElement | null>; width: number; widthPx: number;
}) {
  const t = useT();
  const { lang } = useLanguage();
  const figure = useRef<HTMLElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [near, setNear] = useState(false);
  const [ratio, setRatio] = useState(1 / Math.SQRT2);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const observer = new IntersectionObserver(([entry]) => setNear(entry.isIntersecting), { root: root.current, rootMargin: "600px 0px" });
    observer.observe(figure.current!);
    return () => observer.disconnect();
  }, [root]);
  useEffect(() => {
    if (!near || width <= 0) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    async function draw() {
      try {
        const picture = parsePicture(await fileViewPdfPage(path, page, widthPx));
        if (cancelled || !canvas.current) return;
        const target = canvas.current;
        const context = target.getContext("2d");
        if (!context || !picture.width || !picture.height) throw new Error("Could not be read");
        target.width = picture.width;
        target.height = picture.height;
        const data = context.createImageData(picture.width, picture.height);
        data.data.set(picture.data);
        context.putImageData(data, 0, 0);
        setRatio(picture.width / picture.height);
        setLoading(false);
      } catch (e) { if (!cancelled) { setError(uiError(e, lang)); setLoading(false); } }
    }
    void draw();
    return () => { cancelled = true; };
  }, [path, page, widthPx, near, width > 0, lang]);
  const label = t("fileView.page", { n: page, total: pages });
  return <figure ref={figure} className="file-page" style={{ width: width || "calc(100% - 24px)" }}>
    <figcaption>{label}</figcaption>
    <div className="file-page-picture" style={{ aspectRatio: ratio }}>
      {near && <canvas ref={canvas} role="img" aria-label={label} />}
      {near && loading && <Loading />}
      {error && <p className="file-status" role="alert">{error}</p>}
    </div>
  </figure>;
}

function WorkbookViewer({ book }: { book: FileViewWorkbook }) {
  const t = useT();
  const sheets = book.sheets.filter(sheet => !sheet.hidden);
  const [selected, setSelected] = useState(0);
  if (!sheets.length) return <p className="file-status">{t("fileView.empty")}</p>;
  const sheet = sheets[selected];
  return <Tabs.Root className="file-workbook" value={selected} onValueChange={value => setSelected(Number(value))}>
    <Tabs.List className="file-controls" aria-label={t("fileView.sheets")}>
      {sheets.map((item, i) => <Tabs.Tab key={i} value={i}><bdi>{item.name}</bdi></Tabs.Tab>)}
    </Tabs.List>
    <Tabs.Panel value={selected} className="file-sheet">
      {sheet.truncated && <p className="file-status" role="note">{t("fileView.truncated", { rows: sheet.rows.length, total: sheet.rows_total, cols: sheet.cols_total })}</p>}
      <FileTable key={selected} rows={sheet.rows} name={sheet.name} />
    </Tabs.Panel>
  </Tabs.Root>;
}

function FileTable({ rows, name }: { rows: string[][]; name: string }) {
  const t = useT();
  const scroller = useRef<HTMLDivElement>(null);
  const header = useRef<HTMLTableSectionElement>(null);
  const [headerHeight, setHeaderHeight] = useState(36);
  const many = rows.length > 200;
  useEffect(() => {
    if (!header.current) return;
    const observer = new ResizeObserver(() => setHeaderHeight(header.current?.getBoundingClientRect().height || 36));
    observer.observe(header.current);
    return () => observer.disconnect();
  }, []);
  const virtual = useVirtualizer({
    count: Math.max(0, rows.length - 1), getScrollElement: () => scroller.current,
    estimateSize: () => 36, overscan: 8, enabled: many, scrollMargin: headerHeight,
    initialRect: { width: 600, height: 600 },
  });
  const columns = rows.reduce((max, row) => Math.max(max, row.length), 0);
  const items = virtual.getVirtualItems();
  const top = items.length ? Math.max(0, items[0].start - headerHeight) : 0;
  const bottom = items.length ? Math.max(0, virtual.getTotalSize() - (items.at(-1)!.end - headerHeight)) : 0;
  const cells = (row: string[], head = false) => Array.from({ length: columns }, (_, i) => head
    ? <th key={i} scope="col" dir="auto">{row[i] ?? ""}</th> : <td key={i} dir="auto">{row[i] ?? ""}</td>);
  if (!rows.length) return <p className="file-status">{t("fileView.empty")}</p>;
  return <div className="file-scroll" ref={scroller} tabIndex={0} role="region" aria-label={name}>
    <table className="file-table" aria-label={name} aria-rowcount={rows.length} style={{ width: Math.max(1, columns) * 180 }}>
      <thead ref={header}><tr aria-rowindex={1}>{cells(rows[0], true)}</tr></thead>
      <tbody>
        {many ? <>
          {top > 0 && <tr aria-hidden="true"><td colSpan={columns} className="file-spacer" style={{ height: top }} /></tr>}
          {items.map(item => <tr key={item.key} data-index={item.index} ref={virtual.measureElement} aria-rowindex={item.index + 2}>{cells(rows[item.index + 1])}</tr>)}
          {bottom > 0 && <tr aria-hidden="true"><td colSpan={columns} className="file-spacer" style={{ height: bottom }} /></tr>}
        </> : rows.slice(1).map((row, i) => <tr key={i}>{cells(row)}</tr>)}
      </tbody>
    </table>
  </div>;
}

function ImageViewer({ bytes, name }: { bytes: ArrayBuffer; name: string }) {
  const t = useT();
  const { lang } = useLanguage();
  const [url, setUrl] = useState<string>();
  const [actual, setActual] = useState(false);
  const [error, setError] = useState<string>();
  useEffect(() => {
    const types: Record<string, string> = { png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", webp: "image/webp", gif: "image/gif" };
    const objectUrl = URL.createObjectURL(new Blob([bytes], { type: types[name.split(".").pop()!.toLowerCase()] ?? "application/octet-stream" }));
    setUrl(objectUrl);
    return () => URL.revokeObjectURL(objectUrl);
  }, [bytes, name]);
  return <div className="file-scroll">
    {error ? <p className="file-status" role="alert">{error}</p> : url && <button type="button" className={`file-image${actual ? " actual" : ""}`}
      aria-label={t(actual ? "fileView.fitWidth" : "fileView.actualSize")} aria-pressed={actual} onClick={() => setActual(value => !value)}>
      <img src={url} alt={name} onError={() => setError(uiError(new Error("Could not be read"), lang))} />
    </button>}
  </div>;
}

/** Quoted commas, doubled quotes and embedded newlines follow RFC 4180. */
export function parseCsv(text: string): string[][] {
  if (!text) return [];
  const rows: string[][] = [];
  let row: string[] = [], cell = "", quoted = false;
  for (let i = 0; i < text.length; i++) {
    const char = text[i];
    if (char === '"') {
      if (quoted && text[i + 1] === '"') { cell += '"'; i++; }
      else if (quoted || !cell) quoted = !quoted;
      else cell += char;
    } else if (char === "," && !quoted) { row.push(cell); cell = ""; }
    else if ((char === "\r" || char === "\n") && !quoted) {
      row.push(cell); rows.push(row); row = []; cell = "";
      if (char === "\r" && text[i + 1] === "\n") i++;
    } else cell += char;
  }
  if (cell || row.length || !/[\r\n]$/.test(text)) { row.push(cell); rows.push(row); }
  return rows;
}

function TextViewer({ bytes, name }: { bytes: ArrayBuffer; name: string }) {
  const text = new TextDecoder().decode(bytes);
  const extension = name.split(".").pop()?.toLowerCase();
  if (extension === "csv") return <FileTable rows={parseCsv(text)} name={name} />;
  return <div className="file-scroll file-text">{extension === "md" ? <Markdown text={text} /> : <pre dir="auto">{text}</pre>}</div>;
}
