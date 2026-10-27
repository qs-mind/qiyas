import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { atlasPages, batchVersion, exportEvidence, isNotModified, renderBatch } from "./api";
import { setWorkspace } from "./workspaceIpc";

const buffer = (bytes: number[]) => new Uint8Array(bytes).buffer;
// What qiyas-desktop sends for a batch that is still current: "QINM" and four zero bytes.
const NOT_MODIFIED = buffer([0x51, 0x49, 0x4e, 0x4d, 0, 0, 0, 0]);

describe("exportEvidence", () => {
  beforeEach(() => invoke.mockReset());
  afterEach(() => setWorkspace(null));

  it("uses the displayed drawing and the backend's default path and language", async () => {
    setWorkspace(7);
    const output = { path: "/d/plan evidence.pdf", skipped: [] };
    invoke.mockResolvedValue(output);
    expect(await exportEvidence()).toEqual(output);
    expect(invoke).toHaveBeenCalledWith("export_evidence", { path: null, language: null, workspace: 7 });
  });

  it("passes an explicit path, BOQ language and workspace unchanged", async () => {
    setWorkspace(7);
    const output = { path: "/deliverables/evidence.pdf", skipped: [] };
    invoke.mockResolvedValue(output);
    expect(await exportEvidence(output.path, "ar", 3)).toEqual(output);
    expect(invoke).toHaveBeenCalledWith("export_evidence", { path: output.path, language: "ar", workspace: 3 });
  });
});

describe("renderBatch", () => {
  beforeEach(() => invoke.mockReset());

  it("gives null for the not-modified marker and asks with the version", async () => {
    invoke.mockResolvedValue(NOT_MODIFIED);
    expect(await renderBatch(42)).toBeNull();
    expect(invoke).toHaveBeenCalledWith("render_batch", { ifVersion: 42, pxPerUnit: undefined });
  });

  it("gives the bytes of a batch", async () => {
    // A batch starts with "BYIQ", and is longer than the marker.
    const batch = buffer([0x42, 0x59, 0x49, 0x51, 2, 0, 0, 0, 0, 0, 0, 0]);
    invoke.mockResolvedValue(batch);
    expect(await renderBatch()).toBe(batch);
    expect(invoke).toHaveBeenCalledWith("render_batch", {
      ifVersion: undefined,
      pxPerUnit: undefined,
    });
    // With the zoom the glyphs should be drawn for.
    await renderBatch(undefined, 12.5);
    expect(invoke).toHaveBeenCalledWith("render_batch", { ifVersion: undefined, pxPerUnit: 12.5 });
  });

  it("does not take an 8-byte buffer that is not the marker for it", () => {
    expect(isNotModified(buffer([0x51, 0x49, 0x4e, 0x4d, 0, 0, 0, 1]))).toBe(false);
    expect(isNotModified(buffer([0x51, 0x49, 0x4e, 0x4d]))).toBe(false);
  });
});

describe("batchVersion", () => {
  it("asks the core", async () => {
    invoke.mockResolvedValue(7);
    expect(await batchVersion()).toBe(7);
    expect(invoke).toHaveBeenCalledWith("batch_version");
  });
});

describe("atlasPages", () => {
  it("asks for the pages changed since a version", async () => {
    const bytes = buffer([1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    invoke.mockResolvedValue(bytes);
    expect(await atlasPages(3)).toBe(bytes);
    expect(invoke).toHaveBeenCalledWith("atlas_pages", { sinceVersion: 3 });
  });
});
