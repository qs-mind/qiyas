import { invoke } from "@tauri-apps/api/core";

/** These commands accept existing PDF and workbook paths, not URLs or executables. */
export const openOutputFile = (path: string) => invoke<void>("open_output_file", { path });
export const revealOutputFile = (path: string) => invoke<void>("reveal_output_file", { path });
