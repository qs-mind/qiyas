/** The window's dialogs: those over the drawing (xrefs, attach, calibrate, extract, find, text,
 *  layers, blocks, dimensions, tables, fields, drafting, settings, About) and the file ones after
 *  the status bar (recovery, RECOVER's report, the sheet set manager). Each renders only while
 *  its hook says it is open, in the same place among the window's children as before. */
import { open } from "../fileDialogs";
import {
  blocks as readBlocks,
  calibrate,
  runCommand,
  drawingBlocks,
  IMAGE_EXTENSIONS,
  imageSize,
  loadPlotStyle,
  paperCatalog,
  pdfPageCount,
  pdfPageSize,
  pdfThumbnails,
  reloadXrefs,
  savePlotStyle,
  setToolPalette,
  setXrefPath,
  type DrawingInfo,
} from "../api";
import type { Assistant } from "../ai/useAssistant";
import { useT } from "../i18n";
import { About } from "../shell/About";
import { AliasEditor } from "../shell/AliasEditor";
import { AuditDialog } from "../shell/AuditDialog";
import { NewDrawingDialog } from "../shell/NewDrawingDialog";
import { AttachImageDialog, defaultImageWidth } from "../shell/AttachImageDialog";
import { AttachPdfDialog, CalibrateDialog, metresPerUnit } from "../shell/AttachPdfDialog";
import { AttDefDialog, AttributeManager, AttributesDialog, CreateBlockDialog } from "../shell/BlockDialogs";
import { BlockPalette } from "../shell/BlockPalette";
import { CustomizeUi } from "../shell/CustomizeUi";
import { DesignCenter } from "../shell/DesignCenter";
import { DimStylePanel } from "../shell/DimStylePanel";
import { DotLoader } from "../shell/DotLoader";
import { DraftingSettings } from "../shell/DraftingSettings";
import { ExtractDialog } from "../shell/ExtractDialog";
import { FieldDialog } from "../shell/FieldDialog";
import { ParametersManager } from "../shell/ParametersManager";
import { FindReplaceDialog } from "../shell/FindReplaceDialog";
import { GroupNameDialog, LayerStatesDialog } from "../shell/LayerDialogs";
import { LayerTranslator } from "../shell/LayerTranslator";
import { OptionsDialog } from "../shell/OptionsDialog";
import { PlotDialog } from "../shell/PlotDialog";
import { PlotStyleEditor } from "../shell/PlotStyleEditor";
import { PurgeDialog } from "../shell/PurgeDialog";
import { RecoverReportDialog, RecoveryDialog } from "../shell/RecoveryDialog";
import { ScriptResultDialog } from "../shell/ScriptResultDialog";
import { SettingsDialog } from "../shell/SettingsDialog";
import { SheetSetManager } from "../shell/SheetSetManager";
import { NewTableDialog, TableEditor } from "../shell/TableEditor";
import { TextDialog } from "../shell/TextEditor";
import { ToleranceDialog } from "../shell/ToleranceDialog";
import { ToolPalettes } from "../shell/ToolPalettes";
import { XrefManager } from "../shell/XrefManager";
import { parseThumbnails } from "../viewport/batch";
import type { Annotate } from "./useAnnotate";
import type { Attach } from "./useAttach";
import type { Blocks } from "./useBlocks";
import type { DraftingState } from "./useDrafting";
import type { Drawings } from "./useDrawings";
import type { Layers } from "./useLayers";
import type { Options } from "./useOptions";
import type { Parametric } from "./useParametric";
import type { Plot } from "./usePlot";
import type { Shell } from "./useShell";
import type { Sheets } from "./useSheets";

type Props = {
  info: DrawingInfo | null;
  shell: Shell;
  assistant: Pick<Assistant, "refreshStatus">;
  drafting: DraftingState;
  sheets: Pick<Sheets, "batch" | "renderProgress" | "redraw">;
  layers: Layers;
  attach: Attach;
  blocks: Blocks;
  annotate: Annotate;
  drawings: Drawings;
  plot: Plot;
  options: Options;
  parametric: Pick<Parametric, "parametersOpen" | "parameters" | "setParameter" | "closeParameters" | "deleteConstraint">;
  /** Takes the last step back (the app's undo): the toast after a purge offers it. */
  onUndo(): void;
};

/** The dialogs over the drawing (between the top bar and the panels). */
export function Dialogs({
  info,
  shell,
  assistant,
  drafting: draftingState,
  sheets,
  layers,
  attach,
  blocks,
  annotate,
  drawings,
  plot,
  options,
  parametric,
  onUndo,
}: Props) {
  const t = useT();
  const { theme, aboutOpen, setAboutOpen, settingsOpen, setSettingsOpen, loadAliases, run, setNotice, focusDrawing } =
    shell;
  const { drafting, draftingOpen, setDraftingOpen, saveDrafting } = draftingState;
  const { batch, renderProgress, redraw } = sheets;
  const {
    groupAsk,
    setGroupAsk,
    layerStatesOpen,
    setLayerStatesOpen,
    translatorOpen,
    setTranslatorOpen,
    runDrawingCommand,
    loadLayerStandard,
  } = layers;
  const {
    attachOpen,
    setAttachOpen,
    attachImageOpen,
    setAttachImageOpen,
    pending,
    setPending,
    pickedInsert,
    calibration,
    setCalibration,
    attachPdfPage,
    attachImageFile,
  } = attach;
  const {
    createBlockAsk,
    setCreateBlockAsk,
    attributesAsk,
    setAttributesAsk,
    attDefAt,
    setAttDefAt,
    blockPaletteOpen,
    blockPicture,
    closeBlockPalette,
    startInsert,
    writeBlockFile,
    addToPalette,
    designCenter,
    setDesignCenter,
    palette,
    setPalette,
    runPaletteItem,
    managerBlocks,
    setManagerBlocks,
    runBlock,
  } = blocks;
  const {
    findOpen,
    setFindOpen,
    findTexts,
    replaceTexts,
    showMatch,
    textDialog,
    setTextDialog,
    currentTextStyle,
    currentTextHeight,
    setTextHeight,
    setTextStyle,
    runText,
    dimStyleOpen,
    setDimStyleOpen,
    runDimStyle,
    dimText,
    setDimText,
    newTableAt,
    setNewTableAt,
    tableEditing,
    setTableEditing,
    runTable,
    fieldAsk,
    setFieldAsk,
    fieldTarget,
    setFieldTarget,
    insertField,
  } = annotate;
  const { xrefsOpen, setXrefsOpen, xrefsChanged, locateXref, extractOpen, setExtractOpen, extractRows, exportExtract } =
    drawings;

  return (
    <>
      {aboutOpen && <About theme={theme} onClose={() => setAboutOpen(false)} />}
      {xrefsOpen && info && (
        <XrefManager
          xrefs={info.xrefs}
          onReload={async () => xrefsChanged(await reloadXrefs())}
          onLocate={locateXref}
          onClear={async (name) => xrefsChanged(await setXrefPath(name, null))}
          onClose={() => setXrefsOpen(false)}
        />
      )}
      {attachOpen && info && (
        <AttachPdfDialog
          units={info.units}
          picked={pickedInsert}
          picking={pending?.kind === "insert" && pending.of === "pdf"}
          progress={renderProgress}
          chooseFile={async () => {
            const path = await open({ filters: [{ name: "PDF", extensions: ["pdf"] }] });
            return typeof path === "string" ? path : null;
          }}
          pageCount={pdfPageCount}
          pageSize={pdfPageSize}
          thumbnails={async (path, first, count) =>
            parseThumbnails(await pdfThumbnails(path, first, count, 96))
          }
          onPickPoint={() => setPending({ kind: "insert", of: "pdf" })}
          onAttach={attachPdfPage}
          onClose={() => {
            setAttachOpen(false);
            setPending(null);
          }}
        />
      )}
      {attachImageOpen && info && (
        <AttachImageDialog
          units={info.units}
          defaultWidth={defaultImageWidth(batch?.bounds ?? null, metresPerUnit(info.units))}
          picked={pickedInsert}
          picking={pending?.kind === "insert" && pending.of === "image"}
          chooseFile={async () => {
            const path = await open({
              filters: [{ name: t("attachImage.file"), extensions: IMAGE_EXTENSIONS }],
            });
            return typeof path === "string" ? path : null;
          }}
          imageSize={imageSize}
          onPickPoint={() => setPending({ kind: "insert", of: "image" })}
          onAttach={attachImageFile}
          onClose={() => {
            setAttachImageOpen(false);
            setPending(null);
          }}
        />
      )}
      {calibration && info && (
        <CalibrateDialog
          points={calibration}
          units={info.units}
          onCalibrate={async (distance) => {
            await calibrate(calibration[0], calibration[1], distance);
            await redraw();
          }}
          onClose={() => setCalibration(null)}
        />
      )}
      {plot.plotAsk && info && (
        <PlotDialog
          sheet={plot.plotAsk.sheet}
          drawingPath={info.path}
          units={info.units}
          batch={batch}
          setup={plot.plotAsk.setup}
          stylePath={plot.plotAsk.stylePath}
          picked={plot.pickedWindow}
          picking={plot.windowPick !== null}
          papers={paperCatalog}
          printers={plot.printers}
          chooseStyle={plot.chooseStyle}
          loadStyle={loadPlotStyle}
          choosePdf={plot.choosePdf}
          onPickWindow={plot.startWindowPick}
          onKeep={plot.keep}
          onPlotPdf={plot.plotToPdf}
          onPrint={plot.print}
          onClose={plot.closePlot}
        />
      )}
      {plot.stylesAsk && (
        <PlotStyleEditor
          initialPath={plot.stylesAsk.initialPath}
          chooseOpen={plot.chooseStyle}
          chooseSave={plot.chooseStyleSave}
          load={loadPlotStyle}
          save={savePlotStyle}
          onClose={plot.closeStyles}
        />
      )}
      {extractOpen && info && (
        <ExtractDialog
          layers={info.layers.map((l) => l.name)}
          onRun={extractRows}
          onExport={exportExtract}
          onClose={() => setExtractOpen(false)}
        />
      )}
      {findOpen && info && (
        <FindReplaceDialog
          onFind={findTexts}
          onReplaceAll={replaceTexts}
          onShow={(m) => run(() => showMatch(m))}
          onClose={() => setFindOpen(false)}
        />
      )}
      {textDialog?.kind === "mtext" && info && (
        <TextDialog
          heading="text.newMtext"
          value=""
          multiline
          styles={info.text_styles}
          style={currentTextStyle}
          height={textDialog.request.height ?? currentTextHeight}
          onSubmit={async (d) => {
            const r = textDialog.request;
            const { height, value, style } = d;
            await runText({ type: "mtext", at: r.at, width: r.width, height, value, style });
            setTextHeight(d.height);
            setTextStyle(d.style);
          }}
          onClose={() => {
            setTextDialog(null);
            focusDrawing();
          }}
        />
      )}
      {textDialog?.kind === "edit" && (
        <TextDialog
          heading="text.editTitle"
          value={textDialog.value}
          multiline={textDialog.multiline}
          onSubmit={async (d) => {
            // Unchanged: nothing to do (an MText keeps its formatting).
            if (d.value === textDialog.value) return;
            await runText({
              type: "edit_text",
              target: { handle: textDialog.handle, chain: [] },
              value: d.value,
              tag: null,
            });
          }}
          onClose={() => {
            setTextDialog(null);
            focusDrawing();
          }}
        />
      )}
      {groupAsk && (
        <GroupNameDialog
          names={info?.groups?.map((g) => g.name) ?? []}
          onSubmit={async (name) => {
            await runDrawingCommand({ ...groupAsk, name });
          }}
          onClose={() => {
            setGroupAsk(null);
            focusDrawing();
          }}
        />
      )}
      {layerStatesOpen && info && (
        <LayerStatesDialog
          states={info.layer_states ?? []}
          onSave={async (name) => {
            await runDrawingCommand({ type: "save_layer_state", name });
          }}
          onRestore={async (name) => {
            await runDrawingCommand({ type: "restore_layer_state", name });
          }}
          onRename={async (from, to) => {
            await runDrawingCommand({ type: "rename_layer_state", from, to });
          }}
          onDelete={async (name) => {
            await runDrawingCommand({ type: "delete_layer_state", name });
          }}
          onClose={() => {
            setLayerStatesOpen(false);
            focusDrawing();
          }}
        />
      )}
      {translatorOpen && info && (
        <LayerTranslator
          layers={info.layers}
          loadStandard={loadLayerStandard}
          onTranslate={async (from_path, mappings) => {
            await runDrawingCommand({ type: "translate_layers", from_path, mappings });
          }}
          onClose={() => {
            setTranslatorOpen(false);
            focusDrawing();
          }}
        />
      )}
      {createBlockAsk && (
        <CreateBlockDialog
          names={createBlockAsk.names}
          onSubmit={async (name, convert) => {
            const outcome = await runBlock({ ...createBlockAsk.cmd, name, convert_to_insert: convert });
            if (outcome.type === "edited" && outcome.unlinked > 0) {
              setNotice(t("block.unlinked", { n: outcome.unlinked }));
            }
          }}
          onClose={() => {
            setCreateBlockAsk(null);
            focusDrawing();
          }}
        />
      )}
      {attributesAsk && (
        <AttributesDialog
          heading={attributesAsk.heading}
          fields={attributesAsk.fields}
          onSubmit={async (values) => {
            const cmd = attributesAsk.command(values);
            if (cmd) await runBlock(cmd);
          }}
          onClose={() => {
            setAttributesAsk(null);
            focusDrawing();
          }}
        />
      )}
      {attDefAt && (
        <AttDefDialog
          height={attDefAt.height ?? currentTextHeight}
          onSubmit={async (d) => {
            setTextHeight(d.height);
            const { tag, prompt, height } = d;
            await runBlock({ type: "att_def", tag, prompt, default: d.default, at: attDefAt.at, height });
          }}
          onClose={() => {
            setAttDefAt(null);
            focusDrawing();
          }}
        />
      )}
      {blockPaletteOpen && info && (
        <BlockPalette
          load={readBlocks}
          thumbnail={blockPicture}
          onInsert={(block) => startInsert(block)}
          onWrite={writeBlockFile}
          onAddToPalette={addToPalette}
          onClose={closeBlockPalette}
        />
      )}
      {designCenter && info && (
        <DesignCenter
          choose={async () => {
            const path = await open({ filters: [{ name: "Drawings", extensions: ["dwg", "dxf"] }] });
            return typeof path === "string" ? path : null;
          }}
          list={drawingBlocks}
          has={(name) => designCenter.includes(name.toUpperCase())}
          onInsert={(block, path) => {
            // A block this drawing has is inserted as this drawing defines it.
            const want = block.name.toUpperCase();
            if (!designCenter.includes(want)) {
              startInsert(block, path);
              return;
            }
            run(async () => {
              const own = (await readBlocks()).find((b) => b.name.toUpperCase() === want);
              startInsert(own ?? block);
            });
          }}
          onRedefine={async (name, path) => {
            await runBlock({ type: "redefine_block", name, from_path: path });
            setNotice(t("block.redefined", { name }));
          }}
          onClose={() => {
            setDesignCenter(null);
            focusDrawing();
          }}
        />
      )}
      {palette && info && (
        <ToolPalettes
          items={palette}
          onRun={runPaletteItem}
          onSave={setToolPalette}
          onClose={() => {
            setPalette(null);
            focusDrawing();
          }}
        />
      )}
      {managerBlocks && (
        <AttributeManager
          blocks={managerBlocks}
          onSave={async (block, tag, prompt, value) => {
            await runBlock({ type: "edit_att_def", block, tag, prompt, default: value });
          }}
          onSync={async (block) => {
            const outcome = await runBlock({ type: "sync_attributes", block });
            const locked = outcome.type === "updated" ? outcome.skipped : 0;
            const notes = [t("block.synced")];
            if (locked > 0) notes.push(t("locked.skipped", { n: locked }));
            setNotice(notes.join(" · "));
          }}
          onClose={() => {
            setManagerBlocks(null);
            focusDrawing();
          }}
        />
      )}
      {dimStyleOpen && info && (
        <DimStylePanel
          styles={info.dim_styles}
          current={info.dim_style}
          annotationScale={info.annotation_scale}
          onSave={(name, fields, current) => runDimStyle({ type: "set_dim_style", name, fields, current })}
          onScale={(name) => runDimStyle({ type: "annotation_scale", name })}
          onClose={() => {
            setDimStyleOpen(false);
            focusDrawing();
          }}
        />
      )}
      {(dimText?.kind === "leader" || dimText?.kind === "mleader" || dimText?.kind === "override") && (
        <TextDialog
          heading={dimText.kind === "override" ? "dim.text.override" : "dim.text.leader"}
          value={dimText.kind === "override" ? "<>" : ""}
          multiline={dimText.kind !== "override"}
          onSubmit={async (d) => {
            if (dimText.kind === "override") {
              const target = { handle: dimText.handle, chain: [] };
              await runText({ type: "dim_edit", target, op: { text_override: d.value } });
            } else {
              await runText({ type: dimText.kind, points: dimText.points, text: d.value });
            }
          }}
          onClose={() => {
            setDimText(null);
            focusDrawing();
          }}
        />
      )}
      {dimText?.kind === "comment" && (
        <TextDialog
          heading="dim.text.comment"
          value=""
          multiline
          onSubmit={(d) => runText({ type: "comment", points: dimText.points, text: d.value })}
          onClose={() => {
            setDimText(null);
            focusDrawing();
          }}
        />
      )}
      {newTableAt && (
        <NewTableDialog
          textHeight={currentTextHeight}
          onSubmit={(table) => runText({ type: "table", at: newTableAt, ...table, data: [] })}
          onClose={() => {
            setNewTableAt(null);
            focusDrawing();
          }}
        />
      )}
      {tableEditing && (
        <TableEditor
          key={tableEditing.handle}
          handle={tableEditing.handle}
          table={tableEditing.table}
          cell={tableEditing.cell}
          picking={pending?.kind === "fieldTarget"}
          onRun={runTable}
          onField={(cell) => {
            setFieldTarget(null);
            setFieldAsk({ mode: "cell", cell });
          }}
          onClose={() => {
            setTableEditing(null);
            focusDrawing();
          }}
        />
      )}
      {parametric.parametersOpen && (
        <ParametersManager
          parameters={parametric.parameters}
          precision={draftingState.drafting?.precision ?? 3}
          onSet={parametric.setParameter}
          onDelete={parametric.deleteConstraint}
          onClose={() => {
            parametric.closeParameters();
            focusDrawing();
          }}
        />
      )}
      {fieldAsk && (
        <FieldDialog
          mode={fieldAsk.mode}
          target={fieldTarget}
          picking={pending?.kind === "fieldTarget"}
          onPick={() => setPending({ kind: "fieldTarget" })}
          onSubmit={insertField}
          onClose={() => {
            setFieldAsk(null);
            setPending(null);
            if (!tableEditing) focusDrawing();
          }}
        />
      )}
      {dimText?.kind === "tolerance" && (
        <ToleranceDialog
          onSubmit={(frame) => runText({ type: "tolerance", at: dimText.at, frame })}
          onClose={() => {
            setDimText(null);
            focusDrawing();
          }}
        />
      )}
      {draftingOpen && drafting && (
        <DraftingSettings
          drafting={drafting}
          onSave={saveDrafting}
          onClose={() => setDraftingOpen(false)}
        />
      )}
      {settingsOpen && (
        <SettingsDialog
          onSaved={options.takeSettings}
          onClose={() => {
            setSettingsOpen(false);
            assistant.refreshStatus();
            loadAliases();
          }}
        />
      )}
      {options.optionsOpen && (
        <OptionsDialog
          onSaved={options.takeSettings}
          onSettings={() => setSettingsOpen(true)}
          onClose={() => options.setOptionsOpen(false)}
        />
      )}
      {options.customizeOpen && (
        <CustomizeUi onSaved={options.setRibbon} onClose={() => options.setCustomizeOpen(false)} />
      )}
      {options.aliasesOpen && <AliasEditor onSaved={loadAliases} onClose={() => options.setAliasesOpen(false)} />}
      {options.purgeOpen && info && (
        <PurgeDialog
          look={runCommand}
          apply={runDrawingCommand}
          onUndo={onUndo}
          onClose={() => {
            options.setPurgeOpen(false);
            focusDrawing();
          }}
        />
      )}
      {options.auditOpen && info && (
        <AuditDialog
          look={runCommand}
          apply={runDrawingCommand}
          onClose={() => {
            options.setAuditOpen(false);
            focusDrawing();
          }}
        />
      )}
    </>
  );
}

/** The file dialogs after the status bar: an autosave to recover, RECOVER's report, the sheet set
 *  manager. */
export function FileDialogs({ drawings }: { drawings: Drawings }) {
  const t = useT();
  const {
    recovery,
    recoverFrom,
    discardFrom,
    openAnyway,
    dismissRecovery,
    recoverReport,
    setRecoverReport,
    scriptRun,
    setScriptRun,
    scriptLine,
    stopRunningScript,
    sheetSetShown,
    setSheetSetShown,
    sheetSetApi,
    openAll,
    newOpen,
    setNewOpen,
    newDrawing,
  } = drawings;

  return (
    <>
      {newOpen && <NewDrawingDialog onCreate={newDrawing} onClose={() => setNewOpen(false)} />}
      {recovery && (
        <RecoveryDialog
          key={recovery.id}
          items={recovery.items}
          onRecover={(drawing, copy) => recoverFrom(recovery, drawing, copy)}
          onDiscard={(drawing, copy) => discardFrom(recovery, drawing, copy)}
          onOpen={(drawing) => openAnyway(recovery, drawing)}
          onClose={() => {
            dismissRecovery(recovery);
          }}
        />
      )}
      {recoverReport && (
        <RecoverReportDialog
          path={recoverReport.path}
          report={recoverReport.report}
          onClose={() => setRecoverReport(null)}
        />
      )}
      {scriptLine !== null && (
        <div className="script-running" aria-live="polite">
          <DotLoader pattern="chase" />
          {t("script.running", { line: Math.max(scriptLine, 1) })}
          <button type="button" onClick={stopRunningScript}>
            {t("script.stop")}
          </button>
        </div>
      )}
      {scriptRun && (
        <ScriptResultDialog path={scriptRun.path} result={scriptRun.result} onClose={() => setScriptRun(null)} />
      )}
      {sheetSetShown && (
        <SheetSetManager api={sheetSetApi} onOpenAll={openAll} onClose={() => setSheetSetShown(false)} />
      )}
    </>
  );
}
