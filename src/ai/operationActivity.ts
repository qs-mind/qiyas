/** Presentation of tool inputs. These labels never change or execute an operation. */
import type { Activity } from "../api";
import type { ToolCardItem } from "./messages";

type Detail = NonNullable<Activity["detail"]>;
type Data = Record<string, unknown>;
function data(value: unknown): Data {
  if (typeof value === "string") {
    try { return data(JSON.parse(value)); } catch { return {}; }
  }
  return value && typeof value === "object" && !Array.isArray(value) ? value as Data : {};
}
const line = (verb: string, what: string): Detail => ({ key: `activity.line.${verb}`, params: { what } });

const operations: Record<string, [string, string]> = {
  measure_area: ["measure", "area"], measure_length: ["measure", "length"], measure_distance: ["measure", "length"],
  measure_room: ["measure", "room"], add_measurement: ["add", "takeoff"], remove_measurement: ["remove", "takeoff"],
  make_sheet: ["make", "sheet"], new_layout: ["make", "sheet"], set_sheet: ["show", "sheet"],
  table: ["add", "table"], room_schedule: ["add", "table"], opening_schedule: ["add", "table"],
  object_schedule: ["add", "table"], drafting_knowledge: ["read", "drafting_guidance"],
  state: ["read", "state"], document: ["read", "drawing"], capabilities: ["read", "capability"],
  commands: ["lookup", "command"], validate: ["check", "command"], records: ["read", "item"],
  record_schema: ["read", "record_schema"], query: ["read", "query"], measure: ["measure", "item"],
  check: ["check", "drawing"], history: ["read", "history"], events: ["read", "history"],
  drawings: ["read", "drawing"], selection: ["read", "selection"], rooms: ["read", "room"],
  panel_state: ["read", "panel"], artifact: ["read", "output"], explain: ["read", "explanation"],
  digest: ["read", "overview"], takeoff_kit: ["read", "takeoff_method"], workflow: ["read", "workflow"],
  brief_check: ["check", "brief"], takeoff_check: ["check", "takeoff"],
  plan: ["edit", "plan"], ask: ["ask", "question"], finish: ["finish", "work"],
  type: ["run", "command"], set_records: ["edit", "item"], create_entities: ["add", "item"],
  undo_own: ["undo", "change"], redo_own: ["redo", "change"], select: ["select", "item"],
  view: ["show", "view"], switch_drawing: ["show", "drawing"], new_drawing: ["make", "drawing"],
  open_drawing: ["open", "drawing"], close_drawing: ["close", "drawing"], save_drawing: ["save", "drawing"],
  import_boq: ["import", "boq"], compare: ["compare", "boq"],
  export_takeoff: ["export", "takeoff"], export_conflicts: ["export", "conflicts"],
  export_evidence: ["export", "evidence"], plot: ["export", "sheet"],
  "measurement.set_method": ["set", "measurement_method"],
  "measurement.set_mappings": ["set", "mapping"],
  "measurement.report": ["read", "takeoff"], "measurement.pack_check": ["check", "drawing_pack"],
};

function commandDetail(value: unknown): Detail {
  const command = data(value);
  const type = String(command.type ?? command.op ?? "");
  const args = command.op ? data(command.args) : command;
  if (type === "run") return commandDetail(args.command);
  if (type === "batch") {
    const commands = Array.isArray(args.commands) ? args.commands : [];
    const parts = commands.map(commandDetail);
    // One line per kind of work, even when a batch repeats the same command many times.
    const unique = parts.filter((part, i) => parts.findIndex((other) => JSON.stringify(other) === JSON.stringify(part)) === i);
    return { key: "activity.line.batch", params: unique.length > 3 ? { more: unique.length - 3 } : {}, parts: unique.slice(0, 3) };
  }
  const kind = typeof args.kind === "string" ? args.kind : "";
  if (type === "deliverable" || type === "deliverable.export") {
    const report = String(args.report ?? (kind !== "export" ? kind : "report"));
    const reports: Record<string, string> = { boq: "boq", dimensions: "dimension_sheets", queries: "drawing_queries", variation: "variation", register: "register" };
    return line("export", reports[report] ?? "report");
  }
  const name = kind ? `${type}.${kind}` : type;
  const known = operations[name] ?? operations[type];
  if (known) return line(...known);
  if (type === "draw" || type === "symbol") return line("draw", kind || "item");
  if (["wall", "walls"].includes(type)) return line("draw", "wall");
  return { key: "activity.line.run", params: { command: name, what: "command" } };
}

/** Keep refined geometry counts and scopes. Inputs fill old cards and correct export subjects,
 * which older event metadata described as generic files or measurement checks. */
export function operationDetail(card: ToolCardItem): Detail | null {
  const input = data(card.input);
  const args = data(input.args);
  const op = typeof input.op === "string" ? input.op : card.label ?? "";
  if (card.name === "qiyas_capture") return card.activity?.detail ?? line("capture", "view");
  const command = op === "run" ? data(args.command) : { ...args, type: op };
  const type = String(command.type ?? "");
  const specialized = type === "batch" || type.startsWith("deliverable") || type.startsWith("measurement") || op.startsWith("export_") || op === "plot";
  // A detailed geometry batch is already grouped with counts and scope. Rebuild only batches
  // whose input includes exports or measurement setup, including nested batches.
  const needsCorrection = (value: unknown): boolean => {
    const entry = data(value);
    const name = String(entry.type ?? entry.op ?? "");
    const payload = entry.op ? data(entry.args) : entry;
    return /^(deliverable|measurement|export_|plot)/.test(name) ||
      (name === "run" && needsCorrection(payload.command)) ||
      (Array.isArray(payload.commands) && payload.commands.some(needsCorrection));
  };
  if (card.activity?.detail && (!specialized || (type === "batch" && !needsCorrection(command)))) return card.activity.detail;
  if (!op) return card.activity?.detail ?? null;
  // Legacy cards may retain only a diagnostic label and generic category/effect.
  // Without an operation input, keep those honest localized words.
  if (!input.op && card.activity && !specialized) return card.activity.detail ?? null;
  const described = commandDetail(command);
  if (card.activity?.detail && ["activity.line.unchanged", "activity.line.waiting"].includes(card.activity.detail.key)) {
    return { ...described, key: card.activity.detail.key };
  }
  return described;
}

/** Backend labels identify the completed operation; wording uses the same UI mapping as cards. */
export function progressDetail(label: string): Detail {
  const [type, ...kind] = label.split(".");
  return commandDetail({ type, ...(kind.length ? { kind: kind.join(".") } : {}) });
}
