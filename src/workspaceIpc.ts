/** Capture the displayed workspace when IPC starts. Busy picks wait without blocking tabs. */
import { invoke as nativeInvoke } from "@tauri-apps/api/core";
let visible: number | null = null;
const routed = new Set(["ai_cancel", "ai_clear", "ai_resume", "ai_send", "ai_set_language", "ai_status", "ai_undo", "ai_window_reply", "atlas_pages", "attach_image", "attach_pdf", "batch_version", "block_thumbnail", "blocks", "boq", "boundary_at", "calibrate", "cmdline_step", "compare", "conflicts", "drawing_info", "entity_info", "explain_item", "export_conflicts", "export_evidence", "export_takeoff", "get_drafting", "image_display", "import_boq", "is_dirty", "measurement_report", "paper_views", "pick", "pick_polygon", "pick_window", "project_command", "plot_and_print", "plot_pdf", "plot_pdf_to", "redo", "reload_xrefs", "render_batch", "run_command", "run_script", "save_drawing", "save_drawing_as", "save_project", "set_drafting", "set_layer_hidden", "set_sheet", "set_xref_path", "sheet_batch", "sheet_batch_version", "snap", "table_cell_at", "takeoff", "totals", "underlay_image", "undo", "views", "workspace_attach", "workspace_events", "workspace_provider", "write_block"]);
const reads = new Set(["drawing_info", "drafting", "get_drafting", "takeoff", "totals", "boq", "conflicts", "paper_views", "views", "blocks"]);
const picks = new Set(["pick", "pick_window", "pick_polygon", "snap", "boundary_at"]);
const cached = new Map<string, { args: string; value: unknown }>();
export function retainWorkspaces(ids: number[]) {
  const open = new Set(ids.map(String));
  for (const key of cached.keys()) if (!open.has(key.split(":")[0])) cached.delete(key);
}
export function setWorkspace(id: number | null) { visible = id; }
export function displayedWorkspace() { return visible; }
export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const workspace = args?.workspace ?? visible;
  const input = workspace == null || !routed.has(command) ? args : { ...args, workspace };
  const signature = JSON.stringify(input ?? null);
  for (;;) {
    try {
      const value = await (input === undefined ? nativeInvoke<T>(command) : nativeInvoke<T>(command, input));
      if (reads.has(command)) {
        // One cached response per command and workspace; zoom queries do not grow this store.
        cached.set(`${workspace}:${command}`, { args: signature, value });
      }
      return value;
    } catch (error) {
      if (String(error) !== "workspace_busy") throw error;
      const last = cached.get(`${workspace}:${command}`);
      if (reads.has(command) && last?.args === signature) return last.value as T;
      if (!picks.has(command)) throw error;
      await new Promise(resolve => setTimeout(resolve, 50));
      if (workspace !== visible) throw new Error("Drawing changed");
    }
  }
}
