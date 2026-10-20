/** Every native file picker uses the drawing's folder, the last chosen folder, or Documents.
 *  Untitled drawings have a placeholder path; it must never choose a working folder for them. */
import { documentDir } from "@tauri-apps/api/path";
import {
  open as nativeOpen,
  save as nativeSave,
  type OpenDialogOptions,
  type SaveDialogOptions,
} from "@tauri-apps/plugin-dialog";
import { drawingInfo, type DrawingInfo } from "./api";

export { ask, message } from "@tauri-apps/plugin-dialog";

const LAST_FOLDER = "qiyas.files.lastFolder";

function absolute(path: string) {
  return path.startsWith("/") || /^[A-Za-z]:[\\/]/.test(path) || path.startsWith("\\\\");
}

function folderOf(path: string): string | null {
  const end = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (end < 0 || !absolute(path)) return null;
  return path.slice(0, end + 1);
}

async function startPath(offered?: string, shown?: Pick<DrawingInfo, "path" | "untitled">) {
  const drawing = shown ?? await drawingInfo().catch(() => null);
  let folder = drawing && !drawing.untitled ? folderOf(drawing.path) : null;
  if (!folder) {
    try {
      const last = localStorage.getItem(LAST_FOLDER);
      if (last && absolute(last)) folder = last;
    } catch {
      // A blocked store still leaves Documents as the default.
    }
  }
  folder ??= await documentDir();
  const name = offered?.split(/[\\/]/).pop();
  const separator = folder.includes("\\") ? "\\" : "/";
  return name ? `${folder.replace(/[\\/]$/, "")}${separator}${name}` : folder;
}

function remember(path: string | string[] | null, directory = false) {
  const first = Array.isArray(path) ? path[0] : path;
  const folder = first ? (directory && absolute(first) ? first : folderOf(first)) : null;
  if (folder) {
    try {
      localStorage.setItem(LAST_FOLDER, folder);
    } catch {
      // Picking a file still works when storage is blocked.
    }
  }
}

export async function open<T extends OpenDialogOptions>(options?: T) {
  const result = await nativeOpen<T>({ ...options, defaultPath: await startPath(options?.defaultPath) } as T);
  remember(result, options?.directory);
  return result;
}

export async function save(options?: SaveDialogOptions, shown?: Pick<DrawingInfo, "path" | "untitled">) {
  const result = await nativeSave({ ...options, defaultPath: await startPath(options?.defaultPath, shown) });
  remember(result);
  return result;
}
