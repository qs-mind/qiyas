import { uiError } from "../i18n/errors";
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  cancelUpdate,
  checkUpdate,
  installUpdate,
  NO_UPDATE,
  RESTART_LATER,
  UNSAVED_DRAWINGS,
  UPDATE_BUSY,
  UPDATE_CANCELLED,
  UPDATE_INSTALLING,
  UPDATE_PROGRESS,
  UPDATES_NOT_CONFIGURED,
  type UpdateProgress,
} from "../api";
import { useLanguage, useT } from "../i18n";
import { Progress } from "../ui/Progress";
import { DotLoader } from "./DotLoader";
import type { Theme } from "../viewport/gl";
import { version } from "../../package.json";

/** Where "Check for updates" has got to. Nothing installs until Install and restart is clicked:
 *  then it downloads (Close stops it), installs (nothing stops it) and restarts, or, when a
 *  drawing got unsaved changes meanwhile, is `installed` and waits for the user to restart. */
type Step =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate" }
  | {
      kind: "available";
      version: string;
      notes: string | null;
      phase: "ready" | "downloading" | "installing";
      progress: UpdateProgress | null;
    }
  | { kind: "installed" };

/** How much has come: a percentage when the size is known, else megabytes. */
function amount(p: UpdateProgress | null): string {
  if (!p) return "";
  if (p.total) return `${Math.floor((p.downloaded / p.total) * 100)}%`;
  return `${(p.downloaded / 1_000_000).toFixed(1)} MB`;
}

/** The About box: the Qiyas lockup (English or Arabic, ink or white to suit the theme), who makes
 *  it, the version, and Check for updates. While an update installs, the box can't be closed;
 *  closing it during the download stops the download. */
export function About({ theme, onClose }: { theme: Theme; onClose: () => void }) {
  const t = useT();
  const { lang } = useLanguage();
  const dialog = useRef<HTMLDialogElement>(null);
  const [step, setStep] = useState<Step>({ kind: "idle" });
  const [error, setError] = useState<string | null>(null);
  const phase = step.kind === "available" ? step.phase : null;
  const downloading = useRef(false);
  downloading.current = phase === "downloading";

  useEffect(() => {
    // A modal dialog moves focus inside; give it back to the button that opened it on close.
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal();
    return () => opener?.focus();
  }, []);

  // The download's progress, and the moment it turns into the install.
  useEffect(() => {
    let gone = false;
    const offs: (() => void)[] = [];
    const keep = (off: () => void) => (gone ? off() : offs.push(off));
    const update = (f: (s: Step & { kind: "available" }) => Step) =>
      setStep((s) => (s.kind === "available" && s.phase !== "ready" ? f(s) : s));
    listen<UpdateProgress>(UPDATE_PROGRESS, (e) => update((s) => ({ ...s, progress: e.payload })))
      .then(keep)
      .catch(() => {});
    listen(UPDATE_INSTALLING, () => update((s) => ({ ...s, phase: "installing" })))
      .then(keep)
      .catch(() => {});
    return () => {
      gone = true;
      offs.forEach((off) => off());
    };
  }, []);

  /** The words for a refusal or failure of the updater. */
  function say(e: unknown, key: string): string {
    const text = String(e);
    if (text === UPDATES_NOT_CONFIGURED) return t("about.update.notConfigured");
    if (text === UNSAVED_DRAWINGS) return t("about.update.unsaved");
    if (text === NO_UPDATE) return t("about.update.checkAgain");
    if (text === UPDATE_BUSY) return t("about.update.busy");
    return t(key, { error: uiError(text) });
  }

  async function check() {
    setError(null);
    setStep({ kind: "checking" });
    try {
      const found = await checkUpdate();
      setStep(
        found.status === "available"
          ? { kind: "available", version: found.version, notes: found.notes, phase: "ready", progress: null }
          : { kind: "upToDate" },
      );
    } catch (e) {
      setStep({ kind: "idle" });
      setError(say(e, "about.update.checkFailed"));
    }
  }

  async function install() {
    if (step.kind !== "available") return;
    setError(null);
    setStep({ ...step, phase: "downloading", progress: null });
    try {
      // Qiyas restarts when it worked, so this only comes back otherwise.
      await installUpdate();
    } catch (e) {
      const text = String(e);
      // Stopped because the box closed: nothing to say.
      if (text === UPDATE_CANCELLED) return;
      if (text === RESTART_LATER) {
        setStep({ kind: "installed" });
        return;
      }
      setStep(text === NO_UPDATE ? { kind: "idle" } : { ...step, phase: "ready", progress: null });
      setError(say(e, "about.update.installFailed"));
    }
  }

  const lockup = `/brand/qiyas-lockup${lang === "ar" ? "-ar" : ""}${theme === "dark" ? "-white" : ""}.svg`;

  return (
    <dialog
      ref={dialog}
      className="about"
      aria-labelledby="about-line"
      onClose={() => {
        if (downloading.current) cancelUpdate().catch(() => {});
        onClose();
      }}
      onCancel={(e) => {
        if (phase === "installing") e.preventDefault();
      }}
      // Keys typed here are not shortcuts for the drawing behind (Escape still closes the dialog).
      onKeyDown={(e) => e.stopPropagation()}
    >
      <img src={lockup} alt="" width={280} />
      <h2 id="about-line" className="heading">
        {t("about.line")}
      </h2>
      <p dir="ltr" className="about-version">
        v{version}
      </p>
      <div className="about-update">
        {step.kind === "upToDate" && <p role="status">{t("about.upToDate")}</p>}
        {step.kind === "installed" && <p role="status">{t("about.update.restartLater")}</p>}
        {step.kind === "available" && (
          <>
            <p role="status">{t("about.updateAvailable", { version: step.version })}</p>
            {step.notes && (
              <p className="about-notes" dir="auto">
                {step.notes}
              </p>
            )}
          </>
        )}
        {error && <p role="alert">{error}</p>}
        {/* The size is known: how far the download is, as a bar (the button says it too). */}
        {step.kind === "available" && step.phase === "downloading" && step.progress?.total ? (
          <Progress value={(step.progress.downloaded / step.progress.total) * 100} label={t("about.downloadingLabel")} />
        ) : null}
        {step.kind === "available" ? (
          <button type="button" disabled={step.phase !== "ready"} onClick={install}>
            {step.phase !== "ready" && <DotLoader pattern={step.phase === "installing" ? "spiral" : "scan"} />}
            {step.phase === "ready"
              ? t("about.install")
              : step.phase === "installing"
                ? t("about.installing")
                : t("about.downloading", { amount: amount(step.progress) }).trim()}
          </button>
        ) : (
          step.kind !== "installed" && (
            <button type="button" disabled={step.kind === "checking"} onClick={check}>
              {step.kind === "checking" && <DotLoader pattern="scan" />}
              {t(step.kind === "checking" ? "about.checking" : "about.checkUpdates")}
            </button>
          )
        )}
      </div>
      <button type="button" autoFocus disabled={phase === "installing"} onClick={() => dialog.current?.close()}>
        {t("about.close")}
      </button>
    </dialog>
  );
}
