import { useEffect, useRef, useState } from "react";
import { projectCommand, projectHistory, type ProjectReply } from "../api";
import { open } from "../fileDialogs";
import { useT } from "../i18n";
import { Explain } from "../boq/Explain";
import { DotLoader } from "./DotLoader";

export function ProjectsPanel({ active }: { active: boolean }) {
  const t = useT();
  const [projects, setProjects] = useState<NonNullable<ProjectReply["result"]["projects"]>>([]);
  const [selected, setSelected] = useState<ProjectReply | null>(null);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  async function refresh() {
    const reply = await projectCommand({ kind: "list" });
    setProjects(reply.result.projects ?? []);
  }
  useEffect(() => {
    const current = ++generation.current;
    if (active) projectCommand({ kind: "list" }).then((reply) => {
      if (current === generation.current) { setProjects(reply.result.projects ?? []); setError(null); }
    }).catch((e) => { if (current === generation.current) setError(String(e)); });
    return () => { generation.current++; };
  }, [active]);
  async function run(command: Record<string, unknown>) {
    generation.current++;
    setBusy(true); setError(null);
    try {
      const reply = await projectCommand(command);
      if (reply.result.set) setSelected(reply);
      await refresh();
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  async function add() {
    try {
      const drawing = await open({ filters: [{ name: t("projects.drawings"), extensions: ["dwg", "dxf"] }] });
      if (typeof drawing === "string") await run({ kind: "add_drawing", path: selected?.result.path, drawing });
    } catch (e) { setError(String(e)); }
  }
  async function history(redo: boolean) {
    setBusy(true); setError(null);
    try {
      if (!await projectHistory(redo)) setError(t("projects.historyUnavailable"));
      await refresh();
      if (selected?.result.path) {
        try { setSelected(await projectCommand({ kind: "open", path: selected.result.path })); }
        catch { setSelected(null); }
      }
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  const data = selected?.result.set?.project;
  return <section className="projects-panel" aria-label={t("projects.title")}>
    <form onSubmit={(e) => { e.preventDefault(); if (name.trim()) void run({ kind: "create", name: name.trim() }); }}>
      <label>{t("projects.name")}<input value={name} onChange={(e) => setName(e.target.value)} disabled={busy} /></label>
      <button disabled={busy || !name.trim()}>{t("projects.create")}</button>
    </form>
    <div className="projects-actions">
      <button disabled={busy} onClick={() => void run({ kind: "list" })}>{t("projects.refresh")}</button>
      <button disabled={busy} onClick={() => void history(false)}>{t("undo")}</button>
      <button disabled={busy} onClick={() => void history(true)}>{t("redo")}</button>
    </div>
    {busy && <DotLoader label={t("projects.working")} />}
    {error && <p role="alert">{error}</p>}
    {projects.length === 0 && <p>{t("projects.empty")}</p>}
    <ul className="projects-list">{projects.map((project) => <li key={project.path}>
      <button disabled={busy} aria-pressed={selected?.result.path === project.path} onClick={() => void run({ kind: "open", path: project.path })}>{project.name}</button>
      <span>{project.error ?? t("projects.drawingCount", { n: project.drawings ?? 0 })}</span>
    </li>)}</ul>
    {selected?.result.set && <>
      <h3>{selected.result.set.name}</h3>
      <div className="projects-actions">
        <button disabled={busy} onClick={() => void add()}>{t("projects.add")}</button>
        <button disabled={busy || !selected.result.entries?.length} onClick={() => void run({ kind: "run", path: selected.result.path })}>{t("projects.run")}</button>
      </div>
      <p>{t("projects.method", { name: data?.method?.title ?? t("projects.drawingMethods") })}</p>
      <ul className="projects-list">{selected.result.entries?.map((drawing) => {
        const status = data?.drawings.find((row) => row.drawing === drawing.path);
        return <li key={drawing.path}>
          <bdi title={drawing.full}>{drawing.path.split(/[\\/]/).pop()}</bdi>
          <span>{drawing.missing ? t("projects.missing") : t(`projects.status.${status?.status ?? "pending"}`)}</span>
          {status?.error && <p role="alert">{status.error}</p>}
          {status?.warnings.map((warning) => <p key={warning}>{warning}</p>)}
          <button disabled={busy} aria-label={`${t("projects.remove")} ${drawing.path}`} onClick={() => void run({ kind: "remove_drawing", path: selected.result.path, drawing: drawing.path })}>{t("projects.remove")}</button>
        </li>;
      })}</ul>
      <p>{t("projects.summary", { items: data?.boq.length ?? 0, queries: data?.queries.length ?? 0, register: data?.register.length ?? 0 })}</p>
      {data?.boq.map(({ item }, index) => <p key={index}><bdi>{item.code} · {item.quantity} {item.unit}</bdi> <Explain item={`project:${index}`} project={selected.result.path} /></p>)}
      {data?.warnings.map((warning) => <p key={warning}>{warning}</p>)}
      {data?.register.filter((row) => row.flags.length > 0).map((row) => <p key={`${row.drawing}:${row.sheet}`}><bdi>{row.drawing.split(/[\\/]/).pop()} · {row.sheet}</bdi>: {t("projects.registerFlags", { n: row.flags.length })}</p>)}
      {selected.outputs && <div role="status"><p>{t("projects.workbook")}: <bdi>{selected.outputs.workbook}</bdi></p><p>{t("projects.report")}: <bdi>{selected.outputs.morning_report}</bdi></p></div>}
    </>}
  </section>;
}
