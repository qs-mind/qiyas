// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { ReadReport, Recoverable } from "../api";
import { LanguageProvider, useLanguage } from "../i18n";
import ar from "../i18n/ar.json";
import en from "../i18n/en.json";
import { RecoverReportDialog, RecoveryDialog } from "./RecoveryDialog";
import { localTime } from "./recoveryQueue";

beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
});

afterEach(() => {
  cleanup();
  // The Arabic test's choice is remembered: the next starts in English again.
  localStorage.clear();
});

const newer = (drawing: string, saved_at: number, more: Partial<Recoverable> = {}): Recoverable => ({
  drawing,
  autosave: `${drawing}.sv$`,
  saved_at,
  drawing_at: saved_at - 600,
  crashed: false,
  locked_elsewhere: null,
  older_copies: [],
  ...more,
});

const items: Recoverable[] = [newer("/jobs/villa.dwg", 1_760_000_000), newer("/jobs/tower.dxf", 1_760_000_100)];

const noop = async () => {};

function show(list: Recoverable[]) {
  render(
    <LanguageProvider>
      <RecoveryDialog items={list} onRecover={noop} onDiscard={noop} onOpen={noop} onClose={() => {}} />
    </LanguageProvider>,
  );
}

test("not crashed: a newer autosave exists, with the autosave's and the drawing's times", () => {
  show(items.slice(0, 1));
  expect(screen.getByRole("heading").textContent).toBe(en["recovery.title"]);
  expect(en["recovery.title"]).toBe("A newer autosave exists");
  const row = screen.getByRole("listitem").textContent ?? "";
  expect(row).toContain(localTime(1_760_000_000, "en"));
  expect(row).toContain(localTime(1_760_000_000 - 600, "en"));
  expect(row).not.toContain(en["recovery.older"]);
  expect(screen.queryByRole("note")).toBeNull();
  expect(screen.getByRole("button", { name: "Later" })).toBeTruthy();
});

test("crashed: Qiyas closed unexpectedly last time, Recover, Discard and Later as before", async () => {
  const onRecover = vi.fn(async () => {});
  const onDiscard = vi.fn(async () => {});
  // The autosave may be older than the drawing after a crash: the row says so.
  const crashed = newer("/jobs/villa.dwg", 1_760_000_000, { drawing_at: 1_760_000_900, crashed: true });
  render(
    <LanguageProvider>
      <RecoveryDialog items={[crashed]} onRecover={onRecover} onDiscard={onDiscard} onOpen={noop} onClose={() => {}} />
    </LanguageProvider>,
  );
  expect(screen.getByRole("heading").textContent).toBe("Qiyas closed unexpectedly last time");
  const row = screen.getByRole("listitem").textContent ?? "";
  expect(row).toContain(localTime(1_760_000_000, "en"));
  expect(row).toContain(localTime(1_760_000_900, "en"));
  expect(row).toContain(en["recovery.older"]);
  // One drawing, or all crashed: the heading says it, the row doesn't repeat it.
  expect(row).not.toContain(en["recovery.crashedItem"]);

  fireEvent.click(screen.getByRole("button", { name: "Recover" }));
  await waitFor(() => expect(onRecover).toHaveBeenCalledWith("/jobs/villa.dwg"));
  fireEvent.click(screen.getByRole("button", { name: "Discard" }));
  await waitFor(() => expect(onDiscard).toHaveBeenCalledWith("/jobs/villa.dwg"));
  expect(screen.getByRole("button", { name: "Later" })).toBeTruthy();
});

test("crashed and newer together: the crash heads the dialog and marks its own rows", () => {
  show([
    newer("/jobs/villa.dwg", 1_760_000_000, { crashed: true }),
    newer("/jobs/tower.dxf", 1_760_000_100),
  ]);
  expect(screen.getByRole("heading").textContent).toBe(en["recovery.crashedTitle"]);
  const [crashedRow, newerRow] = screen.getAllByRole("listitem").map((r) => r.textContent ?? "");
  expect(crashedRow).toContain(en["recovery.crashedItem"]);
  expect(newerRow).not.toContain(en["recovery.crashedItem"]);
});

test("open in another Qiyas: a warning with its PID and computer; only Open anyway and Later", async () => {
  const onOpen = vi.fn(async () => {});
  const onDiscard = vi.fn(async () => {});
  const locked = newer("/jobs/villa.dwg", 1_760_000_000, {
    locked_elsewhere: { pid: 4242, host: "site-office-pc", started_at: 1_759_990_000 },
  });
  render(
    <LanguageProvider>
      <RecoveryDialog
        items={[locked, newer("/jobs/tower.dxf", 1_760_000_100)]}
        onRecover={noop}
        onDiscard={onDiscard}
        onOpen={onOpen}
        onClose={() => {}}
      />
    </LanguageProvider>,
  );
  expect(screen.getByRole("note").textContent).toBe(
    "This drawing is open in another Qiyas window (PID 4242 on site-office-pc)",
  );
  const [lockedRow, otherRow] = screen.getAllByRole("listitem");
  // Discard would delete the other Qiyas's autosave, and Recover open its work twice.
  expect(within(lockedRow).getAllByRole("button").map((b) => b.textContent)).toEqual(["Open anyway"]);
  expect(within(otherRow).getAllByRole("button").map((b) => b.textContent)).toEqual(["Recover", "Discard"]);
  expect(screen.getByRole("button", { name: "Later" })).toBeTruthy();

  fireEvent.click(within(lockedRow).getByRole("button", { name: "Open anyway" }));
  await waitFor(() => expect(onOpen).toHaveBeenCalledWith("/jobs/villa.dwg"));
  expect(onDiscard).not.toHaveBeenCalled();
});

test("open elsewhere, nothing to recover: the warning, Open anyway and Later, no autosave time", async () => {
  const onOpen = vi.fn(async () => {});
  const lockOnly = newer("/jobs/villa.dwg", 0, {
    autosave: null,
    saved_at: null,
    drawing_at: 1_760_000_000,
    locked_elsewhere: { pid: 4242, host: "site-office-pc", started_at: 1 },
  });
  render(
    <LanguageProvider>
      <RecoveryDialog items={[lockOnly]} onRecover={noop} onDiscard={noop} onOpen={onOpen} onClose={() => {}} />
    </LanguageProvider>,
  );
  expect(screen.getByRole("heading").textContent).toBe(en["recovery.lockedTitle"]);
  // Nothing to recover: no Recover/Discard explanation, no autosave time.
  expect(screen.getByRole("dialog").textContent).not.toContain(en["recovery.intro"]);
  const row = screen.getByRole("listitem");
  expect(row.textContent).not.toContain("Autosaved");
  expect(row.textContent).toContain(localTime(1_760_000_000, "en"));
  expect(screen.getByRole("note").textContent).toContain("PID 4242 on site-office-pc");
  expect(within(row).getAllByRole("button").map((b) => b.textContent)).toEqual(["Open anyway"]);
  expect(screen.getByRole("button", { name: "Later" })).toBeTruthy();
  fireEvent.click(within(row).getByRole("button", { name: "Open anyway" }));
  await waitFor(() => expect(onOpen).toHaveBeenCalledWith("/jobs/villa.dwg"));
});

test("older copies from a crash are listed under the row, each with its own Recover and Discard", async () => {
  const onRecover = vi.fn(async () => {});
  const onDiscard = vi.fn(async () => {});
  const copies = [
    { path: "/jobs/villa.dwg.sv$.4242", saved_at: 1_760_170_000, kind: "instance" as const },
    { path: "/jobs/villa.dwg.sv$.crash-20261010-090000", saved_at: 1_760_080_000, kind: "crash" as const },
  ];
  render(
    <LanguageProvider>
      <RecoveryDialog
        items={[newer("/jobs/villa.dwg", 0, { autosave: null, saved_at: null, older_copies: copies })]}
        onRecover={onRecover}
        onDiscard={onDiscard}
        onOpen={noop}
        onClose={() => {}}
      />
    </LanguageProvider>,
  );
  expect(screen.getByRole("heading").textContent).toBe(en["recovery.copiesTitle"]);
  const list = screen.getByRole("list", { name: en["recovery.olderCopies"] });
  const rows = within(list).getAllByRole("listitem");
  expect(rows.map((r) => r.textContent)).toEqual(copies.map((c) => expect.stringContaining(localTime(c.saved_at, "en"))));
  // Each says where it came from.
  expect(rows[0].textContent).toContain("from an earlier window");
  expect(rows[1].textContent).toContain("from a crash");
  // Nothing autosaved beside the copies: no Recover or Discard of the drawing itself.
  const [drawingRow] = screen.getAllByRole("listitem");
  expect([...drawingRow.querySelectorAll(":scope > .actions button")]).toHaveLength(0);

  fireEvent.click(within(rows[1]).getByRole("button", { name: "Recover" }));
  await waitFor(() => expect(onRecover).toHaveBeenCalledWith("/jobs/villa.dwg", copies[1].path));
  fireEvent.click(within(rows[0]).getByRole("button", { name: "Discard" }));
  await waitFor(() => expect(onDiscard).toHaveBeenCalledWith("/jobs/villa.dwg", copies[0].path));
});

test("Later waits while Recover or Discard is working (a click or Escape)", async () => {
  let finish = () => {};
  const onClose = vi.fn();
  render(
    <LanguageProvider>
      <RecoveryDialog
        items={items.slice(0, 1)}
        onRecover={() => new Promise<void>((done) => (finish = done))}
        onDiscard={noop}
        onOpen={noop}
        onClose={onClose}
      />
    </LanguageProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Recover" }));
  const later = screen.getByRole("button", { name: "Later" }) as HTMLButtonElement;
  await waitFor(() => expect(later.disabled).toBe(true));
  // Escape does nothing while it works.
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(onClose).not.toHaveBeenCalled();
  expect(screen.getByRole("dialog")).toBeTruthy();
  act(() => finish());
  await waitFor(() => expect(later.disabled).toBe(false));
  expect(onClose).not.toHaveBeenCalled();
});

test("in Arabic the times, the PID and the computer are isolated, so they read in their own order", () => {
  function Arabic() {
    const { setLang } = useLanguage();
    return (
      <>
        <button type="button" onClick={() => setLang("ar")}>
          ar
        </button>
        <RecoveryDialog
          items={[
            newer("/jobs/villa.dwg", 1_760_000_000, {
              drawing_at: null,
              crashed: true,
              locked_elsewhere: { pid: 77, host: "qs-laptop", started_at: 1 },
            }),
          ]}
          onRecover={noop}
          onDiscard={noop}
          onOpen={noop}
          onClose={() => {}}
        />
      </>
    );
  }
  render(
    <LanguageProvider>
      <Arabic />
    </LanguageProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "ar", hidden: true }));
  expect(screen.getByRole("heading").textContent).toBe(ar["recovery.crashedTitle"]);
  const note = screen.getByRole("note");
  expect([...note.querySelectorAll("bdi")].map((b) => b.textContent)).toEqual(["77", "qs-laptop"]);
  const row = screen.getByRole("listitem");
  expect([...row.querySelectorAll("bdi")].map((b) => b.textContent)).toContain(localTime(1_760_000_000, "ar"));
  // The drawing is gone: no drawing time, it says so.
  expect(row.textContent).toContain(ar["recovery.drawingGone"]);
});

test("each autosave newer than its drawing is offered: recover or discard it", async () => {
  const onRecover = vi.fn(async () => {});
  const onDiscard = vi.fn(async () => {});
  const onClose = vi.fn();
  render(
    <LanguageProvider>
      <RecoveryDialog items={items} onRecover={onRecover} onDiscard={onDiscard} onOpen={noop} onClose={onClose} />
    </LanguageProvider>,
  );
  expect(screen.getByRole("heading").textContent).toBe("A newer autosave exists");
  const rows = screen.getAllByRole("listitem");
  expect(rows).toHaveLength(2);
  expect(rows[0].textContent).toContain("villa.dwg");
  expect(rows[0].textContent).toContain("/jobs");

  fireEvent.click(screen.getAllByRole("button", { name: "Recover" })[1]);
  await waitFor(() => expect(onRecover).toHaveBeenCalledWith("/jobs/tower.dxf"));
  fireEvent.click(screen.getAllByRole("button", { name: "Discard" })[0]);
  await waitFor(() => expect(onDiscard).toHaveBeenCalledWith("/jobs/villa.dwg"));
  expect(onClose).not.toHaveBeenCalled();
});

test("a failed recovery says why and keeps the dialog", async () => {
  render(
    <LanguageProvider>
      <RecoveryDialog
        items={items.slice(0, 1)}
        onRecover={async () => {
          throw "The autosave could not be read";
        }}
        onDiscard={async () => {}}
        onOpen={noop}
        onClose={() => {}}
      />
    </LanguageProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Recover" }));
  await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("The file could not be read. Choose another file or a backup."));
});

test("a refused Discard (the drawing was opened elsewhere meanwhile) says why and keeps the row", async () => {
  render(
    <LanguageProvider>
      <RecoveryDialog
        items={items.slice(0, 1)}
        onRecover={noop}
        onDiscard={async () => {
          throw "/jobs/villa.dwg is open in another Qiyas (process 4242 on site-office-pc): its autosave stays";
        }}
        onOpen={noop}
        onClose={() => {}}
      />
    </LanguageProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Discard" }));
  // Rust's English is said in the window's own words.
  await waitFor(() =>
    expect(screen.getByRole("alert").textContent).toBe(
      "This drawing is open in another Qiyas window (PID 4242 on site-office-pc): its autosave stays",
    ),
  );
  expect(screen.getAllByRole("listitem")).toHaveLength(1);
  expect((screen.getByRole("button", { name: "Discard" }) as HTMLButtonElement).disabled).toBe(false);
});

test("Recover's report lists what the reader dropped, by kind", () => {
  const report: ReadReport = {
    records: 456,
    decoded: 419,
    skipped: 28,
    entities: 402,
    recovered_errors: 28,
    dropped: [
      { kind: "LINE", count: 3 },
      { kind: "record-catalog-failed", count: 14 },
    ],
  };
  render(
    <LanguageProvider>
      <RecoverReportDialog path="/jobs/damaged.dwg" report={report} onClose={() => {}} />
    </LanguageProvider>,
  );
  const text = screen.getByRole("dialog").textContent ?? "";
  expect(text).toContain("damaged.dwg");
  expect(text).toContain("402");
  expect(text).toContain("28");
  const rows = screen.getAllByRole("row");
  expect(rows.map((r) => r.textContent)).toContain("Line3");
});

test("Escape is Later when nothing is working", () => {
  const onClose = vi.fn();
  render(
    <LanguageProvider>
      <RecoveryDialog items={items.slice(0, 1)} onRecover={noop} onDiscard={noop} onOpen={noop} onClose={onClose} />
    </LanguageProvider>,
  );
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(onClose).toHaveBeenCalledTimes(1);
});
