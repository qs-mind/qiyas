import type { ReactNode } from "react";
import { useT } from "../i18n";

export function TechnicalDetails({ children }: { children: ReactNode }) {
  const t = useT();
  return <details className="technical-details"><summary>{t("technical.details")}</summary>{children}</details>;
}

export function ReviewNotes({ notes }: { notes: string[] }) {
  const t = useT();
  if (!notes.length) return null;
  return <div><p>{t("evidence.review", { n: notes.length })}</p>
    <TechnicalDetails><ul>{notes.map((note, i) => <li key={i} dir="auto">{note}</li>)}</ul></TechnicalDetails>
  </div>;
}
