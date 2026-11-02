/** The window's top bar: the ribbon, and the language, theme, settings and About buttons. */
import { clearRecents, recents as readRecents, redo, undo, views as readViews, type DrawingInfo, type NamedView } from "../api";
import { useLanguage, useT } from "../i18n";
import { Glyph } from "../shell/glyphs";
import { Ribbon } from "../shell/Ribbon";
import { isRibbonTool, type RibbonHandlers } from "../shell/ribbonLayout";
import { RibbonIcon } from "../shell/ribbonIcons";
import type { Measure } from "./shared";
import type { Annotate } from "./useAnnotate";
import type { Attach } from "./useAttach";
import type { Blocks } from "./useBlocks";
import type { Commands, ToolChoice } from "./useCommands";
import type { Drawings } from "./useDrawings";
import type { Layers } from "./useLayers";
import type { Options } from "./useOptions";
import type { Parametric } from "./useParametric";
import type { Plot } from "./usePlot";
import type { Shell } from "./useShell";
import type { Sheets } from "./useSheets";
import type { Takeoff } from "./useTakeoff";

type Props = {
  info: DrawingInfo | null;
  /** The sheet on screen is model space: named views are of model space. */
  onModel: boolean;
  measure: Pick<Measure, "cancel">;
  shell: Shell;
  toolChoice: ToolChoice;
  commands: Commands;
  sheets: Sheets;
  layers: Layers;
  attach: Attach;
  blocks: Blocks;
  annotate: Annotate;
  drawings: Drawings;
  takeoff: Takeoff;
  plot: Plot;
  options: Pick<Options, "ribbon" | "setOptionsOpen" | "setCustomizeOpen" | "setAliasesOpen" | "openPurge" | "openAudit">;
  /** Opens "Connect an AI app". */
  onConnect(): void;
  parametric: Pick<Parametric, "openParameters" | "setBarHidden">;
  onCommandLine?(line: string): void;
};

export function Header({
  info,
  onModel,
  measure,
  shell,
  toolChoice,
  commands,
  sheets,
  layers,
  attach,
  blocks,
  annotate,
  drawings,
  takeoff,
  plot,
  options,
  parametric,
  onConnect,
  onCommandLine,
}: Props) {
  const t = useT();
  const { lang, setLang } = useLanguage();
  const { theme, chooseTheme, setAboutOpen, setSettingsOpen, viewport, focusDrawing } = shell;
  const { tool, chooseTool } = toolChoice;
  const { apply, history } = commands;
  const { batch, split, setSplit, arabicReversed, chooseArabicReversed, ucsIcon, chooseUcsIcon, saveView, deleteView } =
    sheets;
  const { setLayerStatesOpen, setTranslatorOpen, groupSelect, setGroupSelect } = layers;
  const { setAttachOpen, setAttachImageOpen, setPending, setPickedInsert } = attach;
  const { setBlockPaletteOpen, openAttributeManager, openDesignCenter, openToolPalettes } = blocks;
  const { setFindOpen, openDimStyle, updateFields } = annotate;
  const {
    openFile,
    setNewOpen,
    openPath,
    saveFile,
    saveFileAs,
    exportFile,
    setXrefsOpen,
    setExtractOpen,
    setSheetSetShown,
    recoverFile,
    runScriptFile,
    scriptLine,
  } = drawings;
  const { importBoqFile, compareNow, exportConflictsFile } = takeoff;

  const actions: RibbonHandlers = {
    new: { run: () => setNewOpen(true) },
    open: { run: openFile },
    save: { run: saveFile, enabled: !!info },
    saveAs: { run: saveFileAs, enabled: !!info },
    exportTakeoff: { run: exportFile, enabled: !!info },
    importBoq: { run: importBoqFile, enabled: !!info },
    compare: { run: compareNow, enabled: !!info },
    exportConflicts: { run: exportConflictsFile, enabled: !!info },
    plot: { run: plot.openPlot, enabled: !!info },
    quickPrint: { run: plot.quickPrint, enabled: !!info },
    printAll: { run: plot.printAll, enabled: !!info?.sheets.some((s) => !s.is_model) },
    plotStyles: { run: plot.openStyles },
    undo: { run: () => history(undo) },
    redo: { run: () => history(redo) },
    zoomExtents: { run: () => { viewport.current?.fitExtents(); focusDrawing(); } },
    xrefs: { run: () => setXrefsOpen(true), enabled: !!info },
    attachPdf: { run: () => { setPickedInsert(null); setAttachOpen(true); }, enabled: !!info },
    attachImage: { run: () => { setPickedInsert(null); setAttachImageOpen(true); }, enabled: !!info },
    calibrate: {
      run: () => { measure.cancel(); setPending({ kind: "calibrate", first: null }); focusDrawing(); },
      enabled: !!batch && batch.imageTable.length > 0,
    },
    extract: { run: () => setExtractOpen(true), enabled: !!info },
    find: { run: () => setFindOpen(true), enabled: !!info },
    dimStyle: { run: openDimStyle, enabled: !!info },
    updateFields: { run: updateFields, enabled: !!info },
    parameters: { run: parametric.openParameters, enabled: !!info },
    constraintBar: { run: () => parametric.setBarHidden(false), enabled: !!info },
    blockPalette: { run: () => setBlockPaletteOpen(true), enabled: !!info },
    writeBlock: { run: () => setBlockPaletteOpen(true), enabled: !!info },
    attributeManager: { run: openAttributeManager, enabled: !!info },
    designCenter: { run: openDesignCenter, enabled: !!info },
    toolPalettes: { run: openToolPalettes, enabled: !!info },
    layerStates: { run: () => setLayerStatesOpen(true), enabled: !!info },
    translateLayers: { run: () => setTranslatorOpen(true), enabled: !!info },
    groupSelect: { run: () => setGroupSelect(!groupSelect), pressed: groupSelect },
    split: { run: () => setSplit(!split), enabled: !!info, pressed: split },
    ucsWorld: { run: () => apply({ type: "ucs", origin: [0, 0], x_axis: [1, 0] }), enabled: !!info },
    sheetSet: { run: () => setSheetSetShown(true) },
    recover: { run: recoverFile },
    runScript: { run: runScriptFile },
    audit: { run: options.openAudit, enabled: !!info },
    purge: { run: options.openPurge, enabled: !!info },
    options: { run: () => options.setOptionsOpen(true) },
    customize: { run: () => options.setCustomizeOpen(true) },
    aliases: { run: () => options.setAliasesOpen(true) },
    arabicReversed: { run: () => chooseArabicReversed(!arabicReversed), enabled: !!info, pressed: arabicReversed },
    ucsIcon: { run: () => chooseUcsIcon(!ucsIcon), pressed: ucsIcon },
  };

  return (
    // A running script: the ribbon waits (its Stop is beside the status bar).
    <header className="top" inert={scriptLine !== null}>
      <Ribbon
        onAction={(command) => {
          if (isRibbonTool(command)) chooseTool(command);
          else actions[command].run();
        }}
        isEnabled={(command) => isRibbonTool(command) || actions[command].enabled !== false}
        isPressed={(command) => isRibbonTool(command) ? tool === command : actions[command].pressed === true}
        onCommandLine={onCommandLine}
        loadRecents={readRecents}
        onOpenRecent={openPath}
        onClearRecents={clearRecents}
        layout={options.ribbon}
        aliases={shell.commandAliases}
        {...(onModel && {
          loadViews: readViews,
          onGoToView: (v: NamedView) => {
            viewport.current?.fitTo(v.bounds);
            focusDrawing();
          },
          onSaveView: saveView,
          onDeleteView: deleteView,
        })}
      />
      <div className="prefs">
        {/* One switch each: the language button names the other language in its own script; the
            theme button shows where it goes (a moon in light, a sun in dark). */}
        <button
          type="button"
          className="rb pref-lang"
          lang={lang === "en" ? "ar" : "en"}
          aria-label={t(lang === "en" ? "langAr" : "langEn")}
          title={t(lang === "en" ? "langAr" : "langEn")}
          onClick={() => setLang(lang === "en" ? "ar" : "en")}
        >
          {lang === "en" ? "ع" : "EN"}
        </button>
        <button
          type="button"
          className="rb"
          aria-label={t(theme === "dark" ? "light" : "dark")}
          title={t(theme === "dark" ? "light" : "dark")}
          onClick={() => chooseTheme(theme === "dark" ? "light" : "dark")}
        >
          <RibbonIcon name={theme === "dark" ? "sun" : "moon"} />
        </button>
        <button
          type="button"
          className="rb"
          aria-haspopup="dialog"
          aria-label={t("connect.tip")}
          title={t("connect.tip")}
          onClick={onConnect}
        >
          <Glyph name="plug" size={18} />
        </button>
        <button
          type="button"
          className="rb"
          aria-haspopup="dialog"
          aria-label={t("settings")}
          title={t("settings")}
          onClick={() => setSettingsOpen(true)}
        >
          <RibbonIcon name="gear" />
        </button>
        <button type="button" className="rb" aria-haspopup="dialog" aria-label={t("about")} title={t("about")} onClick={() => setAboutOpen(true)}>
          <RibbonIcon name="info" />
        </button>
      </div>
    </header>
  );
}
