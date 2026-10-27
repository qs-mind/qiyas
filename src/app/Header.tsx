/** The window's top bar: the ribbon, and the language, theme, settings and About buttons. */
import { clearRecents, recents as readRecents, redo, undo, views as readViews, type DrawingInfo, type NamedView } from "../api";
import { useLanguage, useT } from "../i18n";
import { Glyph } from "../shell/glyphs";
import { OutputsMenu } from "../shell/OutputsMenu";
import type { OutputId, OutputInfo } from "../shell/outputs";
import { Ribbon } from "../shell/Ribbon";
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
  /** What Mizan produced, and the one the right column shows (the "Outputs" menu). */
  outputs: OutputInfo[];
  shownOutput: OutputId | null;
  onOpenOutput(id: OutputId): void;
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
  outputs,
  shownOutput,
  onOpenOutput,
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
  const { importBoqFile, compareNow, exportConflictsFile, exportEvidenceFile } = takeoff;

  return (
    // A running script: the ribbon waits (its Stop is beside the status bar).
    <header className="top" inert={scriptLine !== null}>
      <Ribbon
        active={tool}
        onTool={chooseTool}
        onCommandLine={onCommandLine}
        onNew={() => setNewOpen(true)}
        onOpen={openFile}
        loadRecents={readRecents}
        onOpenRecent={openPath}
        onClearRecents={clearRecents}
        onSave={info ? saveFile : undefined}
        onSaveAs={info ? saveFileAs : undefined}
        onExport={info ? exportFile : undefined}
        onImportBoq={info ? importBoqFile : undefined}
        onCompare={info ? compareNow : undefined}
        onExportConflicts={info ? exportConflictsFile : undefined}
        onPlot={info ? plot.openPlot : undefined}
        onQuickPrint={info ? plot.quickPrint : undefined}
        onPrintAll={info?.sheets.some((s) => !s.is_model) ? plot.printAll : undefined}
        onPlotStyles={plot.openStyles}
        onUndo={() => history(undo)}
        onRedo={() => history(redo)}
        onZoomExtents={() => {
          viewport.current?.fitExtents();
          focusDrawing();
        }}
        onXrefs={info ? () => setXrefsOpen(true) : undefined}
        onAttachPdf={
          info
            ? () => {
                setPickedInsert(null);
                setAttachOpen(true);
              }
            : undefined
        }
        onAttachImage={
          info
            ? () => {
                setPickedInsert(null);
                setAttachImageOpen(true);
              }
            : undefined
        }
        onCalibrate={
          batch && batch.imageTable.length > 0
            ? () => {
                measure.cancel();
                setPending({ kind: "calibrate", first: null });
                focusDrawing();
              }
            : undefined
        }
        onExtract={info ? () => setExtractOpen(true) : undefined}
        onFind={info ? () => setFindOpen(true) : undefined}
        onDimStyle={info ? openDimStyle : undefined}
        onUpdateFields={info ? updateFields : undefined}
        onParameters={info ? parametric.openParameters : undefined}
        onConstraintBar={info ? () => parametric.setBarHidden(false) : undefined}
        onInsertBlock={info ? () => setBlockPaletteOpen(true) : undefined}
        onWriteBlock={info ? () => setBlockPaletteOpen(true) : undefined}
        onAttributeManager={info ? openAttributeManager : undefined}
        onDesignCenter={info ? openDesignCenter : undefined}
        onToolPalettes={info ? openToolPalettes : undefined}
        onLayerStates={info ? () => setLayerStatesOpen(true) : undefined}
        onTranslateLayers={info ? () => setTranslatorOpen(true) : undefined}
        groupSelect={groupSelect}
        onGroupSelect={setGroupSelect}
        split={split}
        onSplit={info ? setSplit : undefined}
        onUcsWorld={info ? () => apply({ type: "ucs", origin: [0, 0], x_axis: [1, 0] }) : undefined}
        onSheetSet={() => setSheetSetShown(true)}
        onRecover={recoverFile}
        onRunScript={runScriptFile}
        onAudit={info ? options.openAudit : undefined}
        onPurge={info ? options.openPurge : undefined}
        onOptions={() => options.setOptionsOpen(true)}
        onCustomize={() => options.setCustomizeOpen(true)}
        onAliases={() => options.setAliasesOpen(true)}
        layout={options.ribbon}
        aliases={shell.commandAliases}
        arabicReversed={arabicReversed}
        onArabicReversed={info ? chooseArabicReversed : undefined}
        ucsIcon={ucsIcon}
        onUcsIcon={chooseUcsIcon}
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
        <OutputsMenu outputs={outputs} shown={shownOutput} onOpen={onOpenOutput} onExportEvidence={info ? exportEvidenceFile : undefined} />
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
