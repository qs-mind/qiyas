/** The Manage tab's dialogs (Options, Customize, Aliases, Purge, Audit) and the window-wide
 *  options they keep in `settings.json`: decimals with no drawing open, the snap aperture and pick
 *  box, the keyboard shortcuts and the ribbon layout. Read at start and after a dialog saves. */
import { useEffect, useMemo, useState } from "react";
import { getSettings, type DrawingInfo, type RibbonLayout, type Settings } from "../api";
import { SHORTCUT_COMMANDS } from "../shell/OptionsDialog";
import type { CanvasColours } from "../viewport/colours";
import type { T } from "./shared";
import type { Shell } from "./useShell";

const KNOWN = new Set(SHORTCUT_COMMANDS.map(([c]) => c));

type Deps = Pick<Shell, "setError"> & { t: T; info: DrawingInfo | null };

export function useOptions({ t, info, setError }: Deps) {
  const [showTechnicalDetails, setShowTechnicalDetails] = useState(false);
  const [optionsOpen, setOptionsOpen] = useState(false);
  const [customizeOpen, setCustomizeOpen] = useState(false);
  const [aliasesOpen, setAliasesOpen] = useState(false);
  const [purgeOpen, setPurgeOpen] = useState(false);
  const [auditOpen, setAuditOpen] = useState(false);
  /** Decimals where no drawing is open; a drawing's drafting precision wins while it is. */
  const [displayPrecision, setDisplayPrecision] = useState(3);
  /** CSS pixels around the cursor snaps and picks reach. */
  const [px, setPx] = useState({ snap: 10, pick: 6 });
  const [userShortcuts, setUserShortcuts] = useState<Record<string, string>>({});
  /** The canvas's background and highlight colours; null for the theme's. */
  const [colours, setColours] = useState<CanvasColours>({ background: null, highlight: null });
  /** The user's ribbon; null draws the default. */
  const [ribbon, setRibbon] = useState<RibbonLayout | null>(null);
  /** The shortcuts the keyboard handler can run (a hand-edited file may name others). */
  const shortcuts = useMemo(
    () => Object.fromEntries(Object.entries(userShortcuts).filter(([c]) => KNOWN.has(c))),
    [userShortcuts],
  );

  /** Takes the options from the settings (as read, or as a dialog saved them). */
  function takeSettings(s: Settings) {
    setShowTechnicalDetails(s.show_technical_details ?? false);
    setDisplayPrecision(s.display_precision ?? 3);
    setPx({ snap: s.snap_aperture_px ?? 10, pick: s.pick_box_px ?? 6 });
    setUserShortcuts(s.shortcuts ?? {});
    setRibbon(s.ribbon ?? null);
    setColours({ background: s.background_color ?? null, highlight: s.highlight_color ?? null });
  }

  /** Purge and Audit wait while a block is edited in place (the core would purge or repair the
   *  edit's working copies): the window says so instead of opening them. */
  function openForDrawing(open: (on: boolean) => void) {
    if (info?.block_edit) {
      setError(t("manage.blockEdit"));
      return;
    }
    open(true);
  }
  const openPurge = () => openForDrawing(setPurgeOpen);
  const openAudit = () => openForDrawing(setAuditOpen);

  useEffect(() => {
    getSettings()
      .then(takeSettings)
      // No settings to read: the defaults stay.
      .catch(() => {});
  }, []);

  return {
    showTechnicalDetails,
    optionsOpen,
    setOptionsOpen,
    customizeOpen,
    setCustomizeOpen,
    aliasesOpen,
    setAliasesOpen,
    purgeOpen,
    setPurgeOpen,
    openPurge,
    auditOpen,
    setAuditOpen,
    openAudit,
    displayPrecision,
    px,
    shortcuts,
    colours,
    ribbon,
    setRibbon,
    takeSettings,
  };
}

export type Options = ReturnType<typeof useOptions>;
