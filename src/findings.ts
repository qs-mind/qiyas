import type { Conflict, PackCheck } from "./api";

/** Mirrors qiyas_core::finding::Finding. Saved wire formats are adapted only at the edge. */
export type Finding = {
  id: string;
  rule: string;
  pack: string;
  source: string;
  severity: "error" | "warning" | "info";
  message: string;
  message_ar: string;
  message_key: string;
  params: unknown;
  handles: number[];
  sheet: string | null;
  at?: [number, number] | null;
  quantities: { name: string; value: number; unit: string }[];
  citations: { source: string; clause: string }[];
  passed: boolean | null;
  details: unknown;
};

/** Old conversations have only these fields; new events carry the complete finding. */
export type DrawingFindingRecord = Pick<Finding, "rule" | "message" | "handles"> & Partial<Finding>;

function findingId(source: string, pack: string, rule: string, sheet: string | null, handles: number[], subject: unknown): string {
  const key = JSON.stringify([source, pack, rule, sheet, [...new Set(handles)].sort((a, b) => a - b), subject]);
  let hash = 0xcbf29ce484222325n;
  for (const byte of new TextEncoder().encode(key)) hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100000001b3n);
  return hash.toString(16).padStart(16, "0");
}

export function drawingFinding(record: DrawingFindingRecord): Finding {
  const handles = [...new Set(record.handles)].sort((a, b) => a - b);
  return {
    id: record.id ?? findingId("check", record.pack ?? "general", record.rule, record.sheet ?? null, handles, [record.at ?? null, null, null]),
    source: "check", pack: "general", severity: "warning", message_ar: "",
    message_key: `ai.finding.${record.rule}`, params: {}, sheet: null,
    quantities: [], citations: [], passed: null, details: {},
    ...record,
  };
}

export type DiscrepancyFinding = Finding & { details: Conflict };

export function conflictFinding(record: Conflict): DiscrepancyFinding {
  const query = record.query;
  return {
    id: record.id, source: "discrepancy", pack: "discrepancy",
    rule: query?.rule ?? record.kind,
    severity: query?.severity === "information" ? "info" : query?.severity ?? "warning",
    message: record.reason, message_ar: record.reason_ar,
    message_key: "", params: {},
    handles: [...new Set(query?.references.map(r => r.handle) ?? [])],
    sheet: record.sheets[0] ?? null,
    quantities: (["expected", "found"] as const).flatMap(name =>
      record[name] == null ? [] : [{ name, value: record[name], unit: record.unit }]),
    citations: query?.citations ?? [], passed: null, details: record,
  };
}

export type PackFindings = { ready: boolean; findings: Finding[] };
export function packFindings(card: PackCheck): PackFindings {
  return {
    ready: card.ready,
    findings: card.lines.map(line => ({
      id: findingId("pack_check", "measurement", line.id, null, [], null), rule: line.id, pack: "measurement", source: "pack_check",
      severity: line.passes ? "info" : "warning", message: line.found_en, message_ar: line.found_ar,
      message_key: "", params: {}, handles: [], sheet: null, quantities: [], citations: [],
      passed: line.passes, details: { fix_en: line.fix_en, fix_ar: line.fix_ar },
    })),
  };
}

export const findingText = (finding: Finding, language: "en" | "ar") =>
  language === "ar" && finding.message_ar ? finding.message_ar : finding.message;

export const findingQuantity = (finding: Finding, name: string) =>
  finding.quantities.find(quantity => quantity.name === name);

export function findingFix(finding: Finding, language: "en" | "ar"): string {
  if (!finding.details || typeof finding.details !== "object") return "";
  const details = finding.details as Record<string, unknown>;
  const value = details[language === "ar" ? "fix_ar" : "fix_en"];
  return typeof value === "string" ? value : "";
}
