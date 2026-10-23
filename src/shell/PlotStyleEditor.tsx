import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import type { PlotStyle, PlotStyleEntry } from "../api";
import { useT } from "../i18n";

type Props = {
  /** A file to open at once (the drawing's plot style); null starts with none. */
  initialPath: string | null;
  /** Asks for a CTB or STB file; null when none was chosen. */
  chooseOpen(): Promise<string | null>;
  /** Asks where a table of `kind` goes, offering `defaultPath`; null when cancelled. */
  chooseSave(kind: PlotStyle["kind"], defaultPath: string): Promise<string | null>;
  load(path: string): Promise<PlotStyle>;
  save(path: string, style: PlotStyle): Promise<void>;
  onClose(): void;
};

const hex = (rgb: [number, number, number]) => `#${rgb.map((c) => c.toString(16).padStart(2, "0")).join("")}`;
const rgbOf = (value: string): [number, number, number] => [1, 3, 5].map((i) => parseInt(value.slice(i, i + 2), 16)) as [number, number, number];

/** The plot style editor (STYLESMANAGER): opens a CTB or STB, lists its pens (a CTB's by colour,
 *  an STB's by name, which can be changed) with their pen colour (or the object's), screening,
 *  lineweight (or the object's) and whether they plot (screening 0 does not), and saves the
 *  table over its file or as another. Keys Qiyas does not edit go back as they were. */
export function PlotStyleEditor(props: Props) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const [path, setPath] = useState<string | null>(null);
  const [style, setStyle] = useState<PlotStyle | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    if (props.initialPath) openPath(props.initialPath);
    return () => opener?.focus();
  }, []);

  async function act(work: () => Promise<void>) {
    setBusy(true);
    setError(null);
    setSaved(null);
    try {
      await work();
    } catch (e) {
      setError(uiError(e));
    } finally {
      setBusy(false);
    }
  }

  const openPath = (file: string) =>
    act(async () => {
      const loaded = await props.load(file);
      setStyle(loaded);
      setPath(file);
    });

  async function open() {
    const file = await props.chooseOpen();
    if (file) await openPath(file);
  }

  const saveTo = (file: string) =>
    act(async () => {
      if (!style) return;
      await props.save(file, style);
      setPath(file);
      setSaved(file);
    });

  async function saveAs() {
    if (!style) return;
    const file = await props.chooseSave(style.kind, path ?? `plot.${style.kind}`);
    if (file) await saveTo(file);
  }

  /** Changes pen `i`. */
  function edit(i: number, change: Partial<PlotStyleEntry>) {
    setSaved(null);
    setStyle((s) => s && { ...s, styles: s.styles.map((e, j) => (j === i ? { ...e, ...change } : e)) });
  }

  const label = (e: PlotStyleEntry, i: number) => {
    if (style?.kind === "stb") return `${t("plotStyle.name")} ${i + 1}`;
    const n = /^Color_(\d+)$/.exec(e.name)?.[1];
    return n ? t("plotStyle.colorN", { n }) : e.name;
  };

  return (
    <dialog
      ref={dialog}
      className="settings plot-styles"
      style={{ width: "min(760px, 92vw)" }}
      aria-labelledby="plot-styles-title"
      onClose={props.onClose}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <form method="dialog" onSubmit={(e) => e.preventDefault()}>
        <h2 id="plot-styles-title" className="heading">
          {t("plotStyle.title")}
        </h2>
        <div className="row">
          <button type="button" disabled={busy} onClick={open}>
            {t("plotStyle.open")}
          </button>
        </div>
        {style && path ? (
          <>
            <p className="status">
              <span>{t(`plotStyle.kind.${style.kind}`)}</span> <span dir="ltr">{path}</span>
            </p>
            <div className="extract-table">
              <table>
                <thead>
                  <tr>
                    <th scope="col">{t(style.kind === "ctb" ? "plotStyle.color" : "plotStyle.name")}</th>
                    <th scope="col">{t("plotStyle.pen")}</th>
                    <th scope="col">{t("plotStyle.screening")}</th>
                    <th scope="col">{t("plotStyle.lineweight")}</th>
                    <th scope="col">{t("plotStyle.plot")}</th>
                  </tr>
                </thead>
                <tbody>
                  {style.styles.map((e, i) => {
                    const row = label(e, i);
                    const plots = e.screen > 0;
                    return (
                      <tr key={i}>
                        <td>
                          {style.kind === "stb" ? (
                            <input
                              aria-label={row}
                              value={e.name}
                              onChange={(ev) => edit(i, { name: ev.target.value, localized_name: ev.target.value })}
                            />
                          ) : (
                            row
                          )}
                        </td>
                        <td>
                          <label className="check">
                            <input
                              type="checkbox"
                              aria-label={`${t("plotStyle.objectColor")}: ${row}`}
                              checked={e.color === null}
                              onChange={(ev) => edit(i, { color: ev.target.checked ? null : [0, 0, 0] })}
                            />
                            <span>{t("plotStyle.objectColor")}</span>
                          </label>{" "}
                          <input
                            type="color"
                            aria-label={`${t("plotStyle.pen")}: ${row}`}
                            disabled={e.color === null}
                            value={hex(e.color ?? [0, 0, 0])}
                            onChange={(ev) => edit(i, { color: rgbOf(ev.target.value) })}
                          />
                        </td>
                        <td>
                          <input
                            type="number"
                            min={1}
                            max={100}
                            step={1}
                            aria-label={`${t("plotStyle.screening")}: ${row}`}
                            disabled={!plots}
                            value={e.screen}
                            onChange={(ev) => {
                              const v = Math.round(Number(ev.target.value));
                              if (ev.target.value !== "" && Number.isFinite(v)) edit(i, { screen: Math.min(100, Math.max(1, v)) });
                            }}
                          />
                        </td>
                        <td>
                          <select
                            aria-label={`${t("plotStyle.lineweight")}: ${row}`}
                            value={e.lineweight}
                            onChange={(ev) => edit(i, { lineweight: Number(ev.target.value) })}
                          >
                            {style.lineweights.map((w, j) => (
                              <option key={j} value={j}>
                                {j === 0 ? t("plotStyle.objectWeight") : `${w.toFixed(2)} mm`}
                              </option>
                            ))}
                          </select>
                        </td>
                        <td>
                          <input
                            type="checkbox"
                            aria-label={`${t("plotStyle.plot")}: ${row}`}
                            checked={plots}
                            onChange={(ev) => edit(i, { screen: ev.target.checked ? 100 : 0 })}
                          />
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </>
        ) : (
          <p className="status">{t("plotStyle.empty")}</p>
        )}
        {saved && <p className="status">{t("plotStyle.saved", { path: `\u2068${saved}\u2069` })}</p>}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="buttons">
          <button type="button" disabled={busy} onClick={() => dialog.current?.close()}>
            {t("plotStyle.close")}
          </button>
          <button type="button" disabled={busy || !style} onClick={saveAs}>
            {t("plotStyle.saveAs")}
          </button>
          <button type="button" className="primary" disabled={busy || !style || !path} onClick={() => path && saveTo(path)}>
            {t("plotStyle.save")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
