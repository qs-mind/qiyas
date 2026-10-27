import { Popover } from "@base-ui/react/popover";
import { useRef } from "react";
import type { Drafting, Ucs } from "../api";
import { useT } from "../i18n";
import type { DraftingToggle } from "../tools/shortcuts";
import { usePointer, type PointerStore } from "../tools/pointer";
import { isWorld, toUcs } from "../tools/ucs";
import { Progress } from "../ui/Progress";
import { SegmentedToggles } from "../ui/Segmented";
import { DotLoader, type DotPattern } from "./DotLoader";

type Props = {
  cursor: [number, number] | null;
  /** The pointer, followed live: only this bar renders on a move. When given, it says the cursor
   *  and the snap, and `cursor` and `snapLabel` are not used. */
  pointer?: PointerStore;
  units: string;
  unitsWarning: boolean;
  /** The drawing's reading problems, one line each; a button with their count opens the list. */
  problems: string[];
  message: string;
  /** The motion to show beside the message while something waits (opening, comparing, a picture
   *  drawing); null or absent for none. The message keeps saying what it is. */
  waiting?: DotPattern | null;
  /** How far something that reports it is (a PDF page being drawn), 0 to 100: a bar beside the
   *  message, which says what it is. */
  progress?: number | null;
  /** Decimals of the cursor position. */
  precision?: number;
  /** Metres per drawing unit: the cursor is shown in metres. */
  metresPerUnit?: number;
  /** The name of the object snap under the cursor, or null. */
  snapLabel?: string | null;
  /** The drawing's drafting settings; null (no drawing) leaves the switches off. */
  drafting?: Drafting | null;
  onToggle?(toggle: DraftingToggle): void;
  /** Opens the drafting settings. */
  onDraftingSettings?(): void;
  /** The user coordinate system: the cursor is shown in it, marked "UCS". */
  ucs?: Ucs | null;
};

const TOGGLES: { toggle: DraftingToggle; key: string; on: (d: Drafting) => boolean }[] = [
  { toggle: "osnap", key: "F3", on: (d) => d.osnap },
  { toggle: "ortho", key: "F8", on: (d) => d.ortho },
  { toggle: "polar", key: "F10", on: (d) => d.polar },
  { toggle: "grid", key: "F7", on: (d) => d.grid },
];

/** The name of the object snap under the cursor. Alone in following the pointer, so a move renders
 *  this and not the whole bar. */
function SnapKind({ pointer }: { pointer: PointerStore }) {
  const t = useT();
  const live = usePointer(pointer);
  return live?.snap ? <span className="snap-kind">{t(`snap.${live.snap.kind}`)}</span> : null;
}

/** The cursor position; see `SnapKind`. */
function CursorReadout({ pointer, shown }: { pointer: PointerStore; shown(p: [number, number]): string }) {
  const live = usePointer(pointer);
  return (
    <span className="cursor" dir="ltr">
      {live?.cursor ? shown(live.cursor) : ""}
    </span>
  );
}

/** Bottom line: what is happening, the snap under the cursor, the drafting switches, cursor
 *  position in metres, units and file warnings. */
export function StatusBar({
  cursor,
  pointer,
  units,
  unitsWarning,
  problems,
  message,
  waiting = null,
  progress = null,
  precision = 3,
  metresPerUnit = 1,
  snapLabel = null,
  drafting = null,
  onToggle,
  onDraftingSettings,
  ucs = null,
}: Props) {
  const t = useT();
  const metres = (v: number) => (v * metresPerUnit).toFixed(precision);
  const shown = (p: [number, number]) => {
    const [x, y] = ucs && !isWorld(ucs) ? toUcs(ucs, p) : p;
    const xy = `${metres(x)}, ${metres(y)} ${t("qty.m")}`;
    return ucs && !isWorld(ucs) ? `${t("status.ucs")} ${xy}` : xy;
  };
  const list = useRef<HTMLUListElement>(null);
  return (
    <footer className="status">
      <span className="message" role="status" aria-live="polite">
        {progress !== null ? (
          <Progress compact value={progress} label={message} />
        ) : (
          <>
            {waiting && <DotLoader pattern={waiting} />}
            {message}
          </>
        )}
      </span>
      {pointer ? <SnapKind pointer={pointer} /> : snapLabel && <span className="snap-kind">{snapLabel}</span>}
      {problems.length > 0 && (
        // A popover: it takes the focus (so the keyboard can scroll the list), Escape closes it and
        // gives the focus back to the button, and so does a click outside.
        <Popover.Root modal={false}>
          <Popover.Trigger className="warn problems-trigger">{t("recovered", { n: problems.length })}</Popover.Trigger>
          <Popover.Portal>
            <Popover.Positioner className="problems-pos" side="top" align="end" sideOffset={6} collisionPadding={8}>
              <Popover.Popup className="problems-card" initialFocus={list}>
                <ul className="problem-list" ref={list} tabIndex={-1} aria-label={t("problems")}>
                  {problems.map((p, i) => (
                    <li key={i}>{p}</li>
                  ))}
                </ul>
              </Popover.Popup>
            </Popover.Positioner>
          </Popover.Portal>
        </Popover.Root>
      )}
      {unitsWarning && <span className="warn">{t("unitsUnknown")}</span>}
      {units && <span>{units}</span>}
      <span className="drafting">
        <SegmentedToggles
          label={t("drafting")}
          className="drafting-toggles"
          segments={TOGGLES.map(({ toggle, key }) => ({
            value: toggle,
            label: t(`status.${toggle}`),
            title: `${t(`drafting.${toggle}Title`)} (${key})`,
            disabled: !drafting || !onToggle,
          }))}
          on={drafting ? TOGGLES.filter(({ on }) => on(drafting)).map(({ toggle }) => toggle) : []}
          onToggle={(toggle) => onToggle?.(toggle)}
        />
        <button
          type="button"
          className="gear"
          aria-haspopup="dialog"
          aria-label={t("drafting")}
          title={t("drafting")}
          disabled={!drafting || !onDraftingSettings}
          onClick={onDraftingSettings}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <circle cx="8" cy="8" r="2.2" />
            <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" />
            <circle cx="8" cy="8" r="4.6" />
          </svg>
        </button>
      </span>
      {/* Coordinates read left to right in both languages. */}
      {pointer ? (
        <CursorReadout pointer={pointer} shown={shown} />
      ) : (
        <span className="cursor" dir="ltr">
          {cursor ? shown(cursor) : ""}
        </span>
      )}
    </footer>
  );
}
