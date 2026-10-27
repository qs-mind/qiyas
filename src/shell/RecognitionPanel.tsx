import { TechnicalDetails, ReviewNotes } from "./TechnicalDetails";
import { entityKind } from "../i18n/entityKind";
import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import { runCommand, type Command, type RecognitionCandidate } from "../api";
import { useT } from "../i18n";

export function RecognitionPanel({ active, drawing, revision, run }: {
  active: boolean; drawing: string | null; revision: unknown; run(command: Command): Promise<unknown>;
}) {
  const t = useT();
  const [rows, setRows] = useState<RecognitionCandidate[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [reload, setReload] = useState(0);
  const generation = useRef(0);
  useEffect(() => {
    const current = ++generation.current;
    setRows([]);
    if (!active || !drawing) return;
    runCommand({ type: "recognition", kind: "queue" }).then((result) => {
      if (current !== generation.current) return;
      if (result.type === "recognition") setRows((result.result.candidates ?? []).filter((row) => row.state === "pending" || row.confidence < 0.9 || row.issues.length > 0));
      setError(null);
    }).catch((error) => { if (current === generation.current) setError(uiError(error)); });
    return () => { generation.current++; };
  }, [active, drawing, revision, reload]);

  async function decide(row: RecognitionCandidate, decision: "accepted" | "rejected",
    props?: Record<string, string | number | boolean>, object_type?: string, saveRule = false) {
    const current = generation.current;
    setBusy(true); setError(null);
    try {
      await run({ type: "recognition", kind: "review", handle: row.handle, decision,
        ...(props ? { props } : {}), ...(object_type ? { object_type } : {}),
        ...(saveRule ? { save_as: `review-${row.handle.toString(16)}` } : {}) });
      if (current === generation.current) setReload((n) => n + 1);
    } catch (error) { if (current === generation.current) setError(uiError(error)); }
    finally { setBusy(false); }
  }
  return <section className="recognition-panel" aria-label={t("recognition.review")}>
    <p>{t("recognition.optional")}</p>
    <button disabled={!drawing || busy} onClick={() => setReload((n) => n + 1)}>{t("recognition.refresh")}</button>
    {error && <p role="alert">{error}</p>}
    {rows.length === 0 && <p>{t("recognition.empty")}</p>}
    {rows.map((row) => <ReviewRow key={`${drawing}:${row.handle}`} row={row} busy={busy} decide={decide} />)}
  </section>;
}
function ReviewRow({ row, busy, decide }: {
  row: RecognitionCandidate; busy: boolean;
  decide(row: RecognitionCandidate, decision: "accepted" | "rejected", props?: Record<string, string | number | boolean>, type?: string, save?: boolean): Promise<void>;
}) {
  const t = useT();
  const [fix, setFix] = useState(false);
  const [type, setType] = useState(row.placement.type);
  const [save, setSave] = useState(false);
  const [values, setValues] = useState<Record<string, string>>(() => Object.fromEntries(
    [...new Set([...Object.keys(row.placement.props), ...row.unresolved])].map((key) => [key, String(row.placement.props[key] ?? "")])));
  const props = () => Object.fromEntries(Object.entries(values).map(([key, value]) => {
    const old = row.placement.props[key];
    if (typeof old === "string") return [key, value];
    if (typeof old === "boolean") return [key, value === "true"];
    if (typeof old === "number") return [key, Number(value)];
    try {
      const parsed: unknown = JSON.parse(value);
      if (typeof parsed === "string" || typeof parsed === "number" || typeof parsed === "boolean") return [key, parsed];
    } catch { /* An unresolved text attribute can be entered as plain text. */ }
    return [key, value];
  }));
  return <article>
    <strong>{entityKind(row.placement.type, t)}</strong>
    <TechnicalDetails><pre>{row.rule}{"\n"}{JSON.stringify(row.placement, null, 2)}</pre></TechnicalDetails>
    {row.evidence.map((source, index) => <p key={index}><bdi>{source.path?.split(/[\\/]/).pop()}{source.page ? ` · ${source.page}` : ""}</bdi></p>)}
    <ReviewNotes notes={row.issues} />
    {row.state !== "pending" && <p>{t(row.state === "accepted" ? "recognition.included" : "recognition.excluded")}</p>}
    {fix && row.state === "pending" && <fieldset disabled={busy}>
      <label>{t("recognition.type")}<input value={type} onChange={(e) => setType(e.target.value)} /></label>
      {Object.entries(values).map(([key, value], index) => <label key={key}>{t(`props.${key}`) === `props.${key}` ? t("recognition.property", { n: index + 1 }) : t(`props.${key}`)}<input value={value}
        onChange={(e) => setValues((now) => ({ ...now, [key]: e.target.value }))} /></label>)}
      <button disabled={Object.entries(values).some(([key, value]) => !value.trim()
        || (typeof row.placement.props[key] === "number" && !Number.isFinite(Number(value)))
        || (typeof row.placement.props[key] === "boolean" && value !== "true" && value !== "false"))} onClick={() => decide(row, "accepted", props(), type, save)}>{t("recognition.save")}</button>
    </fieldset>}
    <label><input type="checkbox" disabled={busy || row.state !== "pending"} checked={save} onChange={(e) => setSave(e.target.checked)} />{t("recognition.saveRule")}</label>
    <div className="recognition-actions">
      <button disabled={busy || row.state !== "pending" || row.unresolved.length > 0} onClick={() => decide(row, "accepted", undefined, undefined, save)}>{t("recognition.accept")}</button>
      <button disabled={busy || row.state !== "pending"} onClick={() => setFix(!fix)}>{t("recognition.fix")}</button>
      <button disabled={busy || row.state !== "pending"} onClick={() => decide(row, "rejected", undefined, undefined, save)}>{t("recognition.reject")}</button>
    </div>
  </article>;
}
