// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import { About } from "./About";

const api = vi.hoisted(() => ({ checkUpdate: vi.fn(), installUpdate: vi.fn(), cancelUpdate: vi.fn() }));
const events = vi.hoisted(() => new Map<string, (e: { payload: unknown }) => void>());

vi.mock("../api", async (original) => ({ ...(await original<object>()), ...api }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (e: { payload: unknown }) => void) => {
    events.set(name, handler);
    return () => events.delete(name);
  }),
}));

/** The core sends `name` to the window. */
const send = (name: string, payload: unknown = null) =>
  act(async () => events.get(name)?.({ payload }));

beforeAll(() => {
  // jsdom may not implement modal dialogs.
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
  vi.clearAllMocks();
  events.clear();
  localStorage.clear();
});

function show(lang: "en" | "ar" = "en") {
  localStorage.setItem("qiyas.lang", lang);
  const onClose = vi.fn();
  render(
    <LanguageProvider>
      <About theme="light" onClose={onClose} />
    </LanguageProvider>,
  );
  return onClose;
}

async function click(name: string) {
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name }));
  });
}

test("check for updates: up to date", async () => {
  api.checkUpdate.mockResolvedValue({ status: "up_to_date" });
  show();
  await click("Check for updates");
  expect(api.checkUpdate).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("status").textContent).toBe("Up to date");
  expect(api.installUpdate).not.toHaveBeenCalled();
});

test("an update found is installed only on a click; the download shows its progress", async () => {
  api.checkUpdate.mockResolvedValue({ status: "available", version: "0.2.0", notes: "Plotting to PDF." });
  let fail: (e: string) => void = () => {};
  api.installUpdate.mockReturnValue(new Promise((_, reject) => (fail = reject)));
  const onClose = show();
  await click("Check for updates");
  expect(screen.getByRole("status").textContent).toBe("Update available: 0.2.0");
  expect(screen.getByText("Plotting to PDF.")).toBeTruthy();
  expect(api.installUpdate).not.toHaveBeenCalled();

  expect(screen.getByRole("button", { name: "Install and restart" }).querySelector(".dot-loader")).toBeNull();
  await click("Install and restart");
  expect(api.installUpdate).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "Downloading…" })).toBeTruthy();
  // The dots wait beside the text (hidden from assistive technology: the name stays the text).
  expect(screen.getByRole("button", { name: "Downloading…" }).querySelector(".dot-loader")).toBeTruthy();
  // No size yet: no percentage, so no bar.
  expect(screen.queryByRole("progressbar")).toBeNull();
  await send("update:progress", { downloaded: 45, total: 100 });
  expect(screen.getByRole("button", { name: "Downloading… 45%" })).toBeTruthy();
  // The size is known: how far it is, as a labelled bar too.
  expect(screen.getByRole("progressbar", { name: "Downloading the update…" }).getAttribute("aria-valuenow")).toBe("45");
  await send("update:progress", { downloaded: 3_500_000, total: null });
  expect(screen.getByRole("button", { name: "Downloading… 3.5 MB" })).toBeTruthy();
  expect(screen.queryByRole("progressbar")).toBeNull();
  // While it downloads, Close still works (it stops the download: below).
  expect((screen.getByRole("button", { name: "Close" }) as HTMLButtonElement).disabled).toBe(false);

  // Installing: neither the button nor Close can be pressed, and Escape is ignored.
  await send("update:installing");
  expect((screen.getByRole("button", { name: "Installing…" }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByRole("button", { name: "Installing…" }).querySelector(".dot-spiral")).toBeTruthy();
  expect((screen.getByRole("button", { name: "Close" }) as HTMLButtonElement).disabled).toBe(true);
  const escape = new Event("cancel", { cancelable: true });
  screen.getByRole("dialog", { hidden: true }).dispatchEvent(escape);
  expect(escape.defaultPrevented).toBe(true);
  expect(onClose).not.toHaveBeenCalled();

  // A failed install says why and offers Install again.
  await act(async () => fail("signature does not match"));
  expect(screen.getByRole("alert").textContent).toBe(
    "The update could not be installed: The update could not be verified. Download it again.",
  );
  expect((screen.getByRole("button", { name: "Install and restart" }) as HTMLButtonElement).disabled).toBe(false);
});

test("a build without a real key says updates are not set up, never nothing", async () => {
  api.checkUpdate.mockRejectedValue("updates_not_configured");
  show();
  await click("Check for updates");
  expect(screen.getByRole("alert").textContent).toBe("Updates are not set up in this build yet.");
  // The check can be asked again.
  expect(screen.getByRole("button", { name: "Check for updates" })).toBeTruthy();
});

test("unsaved drawings stop the install; a failed check says why", async () => {
  api.checkUpdate.mockResolvedValueOnce({ status: "available", version: "0.2.0", notes: null });
  api.installUpdate.mockRejectedValue("unsaved_drawings");
  show();
  await click("Check for updates");
  await click("Install and restart");
  expect(screen.getByRole("alert").textContent).toBe("Save your drawings first: the update restarts Qiyas.");

  api.installUpdate.mockRejectedValue("no_update");
  await click("Install and restart");
  expect(screen.getByRole("alert").textContent).toBe("Check for updates again first.");
  expect(screen.getByRole("button", { name: "Check for updates" })).toBeTruthy();

  api.checkUpdate.mockRejectedValue("error sending request");
  await click("Check for updates");
  expect(screen.getByRole("alert").textContent).toBe("The update check failed: That didn’t work. Technical details are available in Settings.");
});

test("closing the box during the download stops it", async () => {
  api.checkUpdate.mockResolvedValue({ status: "available", version: "0.2.0", notes: null });
  api.installUpdate.mockReturnValue(new Promise(() => {}));
  api.cancelUpdate.mockResolvedValue(undefined);
  const onClose = show();
  await click("Check for updates");
  await click("Install and restart");
  await click("Close");
  expect(api.cancelUpdate).toHaveBeenCalledTimes(1);
  expect(onClose).toHaveBeenCalled();
});

test("installed while a drawing got unsaved changes: Qiyas waits for the user to restart", async () => {
  api.checkUpdate.mockResolvedValue({ status: "available", version: "0.2.0", notes: null });
  api.installUpdate.mockRejectedValue("installed_restart_later");
  show();
  await click("Check for updates");
  await click("Install and restart");
  expect(screen.getByRole("status").textContent).toBe(
    "Installed. Restart Qiyas when you are ready: save your drawings first.",
  );
  expect(screen.queryByRole("button", { name: "Install and restart" })).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
});

test("an install already running (the launch dialog's) is said so", async () => {
  api.checkUpdate.mockResolvedValue({ status: "available", version: "0.2.0", notes: null });
  api.installUpdate.mockRejectedValue("update_busy");
  show();
  await click("Check for updates");
  await click("Install and restart");
  expect(screen.getByRole("alert").textContent).toBe("An update is being installed already.");
});

test("in Arabic", async () => {
  api.checkUpdate.mockResolvedValue({ status: "available", version: "0.2.0", notes: null });
  show("ar");
  await click("البحث عن تحديثات");
  expect(screen.getByRole("status").textContent).toBe("تحديث متاح: \u20680.2.0\u2069");
  expect(screen.getByRole("button", { name: "التثبيت وإعادة التشغيل" })).toBeTruthy();
});
