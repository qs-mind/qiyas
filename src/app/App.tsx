import { uiError } from "../i18n/errors";
/** The window: the composition root. It calls the hooks in an order that keeps their effects
 *  where they were (shell, sheets' fit, the tool, the assistant, the drawings, the command line,
 *  the selection) and hands their results to the top bar, the dialogs and the panels. Hooks
 *  that call back into one made later (`apply`, `onCommand`, the tool's `cancel`) reach it
 *  through a ref filled right after it exists. */
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { boundaryAt, entityInfo, pick, pickWindow, snap, undo, type AiRegion, type DrawingInfo } from "../api";
import { EXPLAIN_EVIDENCE, type Evidence } from "../boq/Explain";
import { useLanguage, useT } from "../i18n";
import { metresPerUnit } from "../shell/AttachPdfDialog";
import { CommandLine } from "../shell/CommandLine";
import { StatusBar } from "../shell/StatusBar";
import type { Action } from "../tools/commandParser";
import { useTool } from "../tools/useTool";
import { Notices } from "../ui/notices";
import { Dialogs, FileDialogs } from "./Dialogs";
import { Header } from "./Header";
import { PanelHandle, usePanelRails } from "./panelRails";
import { LeftPanels, SidePanel, WorkArea } from "./Panels";
import { readProblems } from "./shared";
import { useAnnotate } from "./useAnnotate";
import { useAssistantWiring } from "./useAssistantWiring";
import { useConversationHistory } from "../ai/ConversationHistory";
import { useAttach } from "./useAttach";
import { useBlocks } from "./useBlocks";
import { useCommandLine } from "./useCommandLine";
import { type Commands, useCommands, useToolChoice } from "./useCommands";
import { useComments } from "./useComments";
import { useDrafting } from "./useDrafting";
import { useAutosave, useDrawings, useFileTabs } from "./useDrawings";
import { useLayers } from "./useLayers";
import { useOptions } from "./useOptions";
import { useOverlay } from "./useOverlay";
import { useParametric } from "./useParametric";
import { usePlot } from "./usePlot";
import { useSelection } from "./useSelection";
import { useSheets } from "./useSheets";
import { useShell } from "./useShell";
import { useTakeoff, useTakeoffState } from "./useTakeoff";

export default function App() {
  const t = useT();
  const { lang } = useLanguage();
  const [info, setInfo] = useState<DrawingInfo | null>(null);
  const panels = usePanelRails();
  /** Counts up when the canvas menu's Properties asks for the palette (it may be folded away). */
  const [revealProperties, setRevealProperties] = useState(0);
  const shell = useShell();
  // The BOQ and conflicts tabs want a wider right panel: their width is kept apart.
  const rightSlot = shell.sideTab === "boq" || shell.sideTab === "conflicts" ? "rightWide" : "right";
  const options = useOptions({ t, info, setError: shell.setError });
  const { theme, settingsOpen, setSettingsOpen, commandAliases, setError, setNotice, setSideTab, viewport, focusDrawing } =
    shell;
  /** User commands end Mizan's animation before running. Background refreshes use shell.run. */
  const cancelRevealRef = useRef<(() => void) | null>(null);
  const run = async (step: () => Promise<void>) => {
    cancelRevealRef.current?.();
    await shell.run(step);
  };
  const { fileTabs, setFileTabs, activeTab, setActiveTab, refreshTabs } = useFileTabs();
  const { autosaveOff, autosave } = useAutosave(t, setError);
  /** Bumped whenever another drawing comes on screen (open, tab switch, close): work started
   *  before is stale (`isStale`) and drops its results. */
  const switchGen = useRef(0);
  const draftingState = useDrafting({ run, autosave });
  const { drafting, setDraftingState, setDraftingOpen, toggleDrafting } = draftingState;
  const takeoffState = useTakeoffState(switchGen);
  const { items, setSelectedId, setCompared, aiHighlight, setAiHighlight, setInfoCard, refreshTakeoffAndBoq } = takeoffState;
  /** The tool hook comes after the sheets (it takes their state); the sheets cancel it through this. */
  const measureRef = useRef<{ cancel(): void } | null>(null);
  const sheets = useSheets({ switchGen, viewport, run: shell.run, autosave, setInfo, refreshTabs, items, measure: measureRef });
  cancelRevealRef.current = sheets.cancelReveal;
  const {
    batch,
    batchHeld,
    setRenderProgress,
    setArabicReversed,
    sheet,
    setSheetBlock,
    hidden,
    setHidden,
    fitMatch,
    redraw: redrawSheet,
    syncPictures,
    chooseSheet,
  } = sheets;
  const redraw = async (fit = false) => {
    sheets.cancelReveal(false);
    await redrawSheet(fit);
  };
  const layers = useLayers({
    switchGen,
    info,
    setInfo,
    setError,
    run,
    autosave,
    redraw,
    cancelReveal: sheets.cancelReveal,
    refreshTakeoffAndBoq,
    hidden,
    setHidden,
  });
  const { setGroupAsk, runDrawingCommand, runLayerCommand, groupOf, syncInfo } = layers;
  const toolChoice = useToolChoice(() => {
    sheets.cancelReveal();
    focusDrawing();
  });
  const { tool, lastPoint, preset, chooseTool } = toolChoice;
  const attach = useAttach({ t, setInfo, setNotice, viewport, batchHeld, redraw, syncPictures });
  /** `apply` and `onCommand` come after the hooks that call them back (a block placed, a
   *  palette's command line); those reach them through these, filled once they exist. */
  const applyRef = useRef<Commands["apply"] | null>(null);
  const onCommandRef = useRef<((a: Action) => void) | null>(null);
  /** The area tool hands the area it marked to Mizan's wiring, made after the tool. */
  const markRegionRef = useRef<((region: AiRegion) => void) | null>(null);
  const blocks = useBlocks({
    t,
    switchGen,
    info,
    setInfo,
    setError,
    setNotice,
    run,
    autosave,
    redraw,
    refreshTakeoffAndBoq,
    focusDrawing,
    chooseTool,
    commandAliases,
    apply: applyRef,
    onCommand: onCommandRef,
  });
  const { setCreateBlockAsk, setAttributesAsk, setAttDefAt, placeBlock } = blocks;
  const annotate = useAnnotate({
    t,
    switchGen,
    info,
    setInfo,
    setNotice,
    run,
    autosave,
    focusDrawing,
    viewport,
    redraw,
    sheet,
    fitMatch,
    chooseSheet,
    batchHeld,
    setAiHighlight,
    placeBlock,
    setAttDefAt,
    setAttributesAsk,
    apply: applyRef,
  });
  const { setDimText, currentTextHeight, placeText, openTextEdit } = annotate;
  const parametric = useParametric({
    info,
    batch,
    tool,
    run,
    redraw,
    refreshTakeoffAndBoq,
    setAiHighlight,
    autosave,
    apply: applyRef,
  });
  const commands = useCommands({
    t,
    setInfo,
    info,
    setNotice,
    run,
    autosave,
    redraw,
    refreshTakeoffAndBoq,
    setInfoCard,
    setSelectedId,
    chooseTool,
    syncInfo,
    runDrawingCommand,
    setGroupAsk,
    setCreateBlockAsk,
    currentTextHeight,
  });
  const { apply, history } = commands;
  applyRef.current = apply;
  const measure = useTool(
    tool,
    {
      pick,
      pickWindow,
      snap,
      entityInfo,
      run: apply,
      zoomTo: (bounds) => viewport.current?.fitTo(bounds),
      boundary: (p) => {
        // About 50 screen pixels across first; the core looks further until the area fits.
        const pxPerUnit = viewport.current?.pxPerUnit() ?? 1;
        return boundaryAt(p[0], p[1], 25 / pxPerUnit);
      },
      hatchScale: () => 0.025 / metresPerUnit(info?.units ?? "unknown"),
      undo: () => history(undo),
      onError: (e) => setError(uiError(e)),
      placeText,
      editText: (handle, at) => run(() => openTextEdit(handle, at)),
      dimText: setDimText,
      groupOf,
      markRegion: (region) => markRegionRef.current?.(region),
    },
    drafting,
    info?.ucs,
    options.px,
  );
  measureRef.current = measure;
  const ai = useAssistantWiring({
    t,
    lang,
    switchGen,
    info,
    setInfo,
    run: shell.run,
    autosave,
    setSettingsOpen,
    viewport,
    batch,
    sheet,
    chooseSheet,
    queueReveal: sheets.queueReveal,
    cancelReveal: sheets.cancelReveal,
    setSheetBlock,
    setHidden,
    setRenderProgress,
    redraw: redrawSheet,
    refreshTakeoffAndBoq,
    setCompared,
    setAiHighlight,
    syncInfo,
    measure,
    tool,
    chooseTool,
    setSideTab,
  });
  markRegionRef.current = ai.markRegion;
  const { assistant } = ai;
  const drawings = useDrawings({
    t,
    switchGen,
    info,
    setInfo,
    setError,
    setNotice,
    run,
    settingsOpen,
    fileTabs,
    setFileTabs,
    setActiveTab,
    refreshTabs,
    autosaveOff,
    autosave,
    setDraftingState,
    sheets,
    setArabicReversed,
    setSheetBlock,
    setHidden,
    redraw,
    takeoffState,
    refreshTakeoffAndBoq,
    attach,
    preset,
    lastPoint,
    measure,
  });
  const conversationHistory = useConversationHistory({
    drawing: info?.path ?? null,
    busy: assistant.busy,
    empty: assistant.transcript.items.length === 0,
    savedVersion: assistant.savedVersion,
    prepare: drawings.prepareConversation,
    resume: assistant.resume,
  });
  const takeoff = useTakeoff({
    t,
    info,
    setNotice,
    run,
    autosave,
    setSideTab,
    viewport,
    sheet,
    batch,
    chooseSheet,
    redraw,
    takeoffState,
    apply,
  });
  const { selected, boqLinked, conflictLinked } = takeoff;
  const plot = usePlot({ t, switchGen, info, sheet, viewport, run, setError, setNotice, focusDrawing, measure });
  const { commandLineOn, commandInput, onCommand, typeCommand, startCommand, coreActive, clickDrawing, commandState, prompt, message, waiting, progress } = useCommandLine({
    t,
    switchGen,
    info,
    measure,
    assistant,
    shell: { ...shell, run },
    drafting: { toggleDrafting },
    toolChoice,
    commands,
    sheets,
    layers,
    attach,
    blocks,
    annotate,
    drawings,
    takeoffState,
    takeoff,
    plot,
    options,
    parametric,
  });
  onCommandRef.current = onCommand;
  useEffect(() => {
    const show = (event: Event) => {
      const evidence = (event as CustomEvent<Evidence[]>).detail;
      if (!evidence.length) return;
      void run(async () => {
        const first = evidence[0];
        if (first.drawing !== info?.path) await drawings.openPath(first.drawing);
        // A cancelled open must never highlight unrelated handles in the previous drawing.
        const current = await import("../api").then(api => api.drawingInfo());
        if (current.path !== first.drawing) return;
        await chooseSheet(first.sheet);
        setAiHighlight(evidence.map(e => e.handle));
      });
    };
    window.addEventListener(EXPLAIN_EVIDENCE, show);
    return () => window.removeEventListener(EXPLAIN_EVIDENCE, show);
  }, [info?.path, drawings.openPath, chooseSheet, run, setAiHighlight]);
  const selection = useSelection({ info, tool, measure, batch, redraw, runLayerCommand });
  const { highlight, overlay } = useOverlay({
    tool, measure, batch, selected, boqLinked, conflictLinked, aiHighlight, revealOverlay: sheets.revealOverlay,
  });
  const comments = useComments({ sideTab: shell.sideTab, viewport, sheet, chooseSheet, switchGen, info, batch });

  // An empty model is said next to the file name, not counted as a reading problem.
  const problems = info ? readProblems(info) : [];
  const sheetShown = info?.sheets.find((s) => s.block.toLowerCase() === sheet.toLowerCase());
  // Named views are of model space.
  const onModel = sheetShown?.is_model ?? false;
  // Unknown units are measured as millimetres; the status bar says so.
  const units = info ? t(`unit.${info.units === "unknown" ? "millimetres" : info.units}`) : "";

  return (
    <div className="app">
      <Header
        info={info}
        onModel={onModel}
        measure={measure}
        shell={shell}
        toolChoice={toolChoice}
        commands={commands}
        sheets={sheets}
        layers={layers}
        attach={attach}
        blocks={blocks}
        annotate={annotate}
        drawings={drawings}
        takeoff={takeoff}
        plot={plot}
        options={options}
        parametric={parametric}
        onCommandLine={(line) => {
          void startCommand(line).catch((error) => shell.setError(uiError(error)));
        }}
      />
      <Dialogs
        info={info}
        shell={shell}
        assistant={assistant}
        drafting={draftingState}
        sheets={sheets}
        layers={layers}
        attach={attach}
        blocks={blocks}
        annotate={annotate}
        drawings={drawings}
        plot={plot}
        options={options}
        parametric={parametric}
        onUndo={() => history(undo)}
      />
      <div
        data-open-panel={shell.sideTab}
        className={`middle${panels.hidden.left ? " left-off" : ""}${panels.hidden.right ? " right-off" : ""}`}
        style={{ "--left-w": `${panels.width.left}px`, "--side-w": `${panels.width[rightSlot]}px` } as CSSProperties}
      >
        <LeftPanels
          info={info}
          sheetShown={sheetShown}
          theme={theme}
          drafting={drafting}
          sheets={sheets}
          layers={layers}
          selection={selection}
          revealProperties={revealProperties}
        />
        <PanelHandle
          side="left"
          hidden={panels.hidden.left}
          width={panels.width.left}
          onToggle={() => panels.toggle("left")}
          onFold={(folded) => panels.setFolded("left", folded)}
          onResize={(w) => panels.resize("left", w)}
        />
        <WorkArea
          info={info}
          fileTabs={fileTabs}
          activeTab={activeTab}
          tool={tool}
          measure={measure}
          clickDrawing={clickDrawing}
          highlight={highlight}
          overlay={overlay}
          drafting={drafting}
          shell={shell}
          sheets={sheets}
          attach={attach}
          blocks={blocks}
          annotate={annotate}
          drawings={drawings}
          selection={selection}
          colours={options.colours}
          parametric={parametric}
          chooseTool={chooseTool}
          askMizan={ai.pointing}
          onProperties={() => {
            panels.setFolded("left", false);
            setRevealProperties((n) => n + 1);
          }}
        />
        <PanelHandle
          side="right"
          slot={rightSlot}
          hidden={panels.hidden.right}
          width={panels.width[rightSlot]}
          onToggle={() => panels.toggle("right")}
          onFold={(folded) => panels.setFolded("right", folded)}
          onResize={(w) => panels.resize(rightSlot, w)}
        />
        <SidePanel
          showTechnicalDetails={options.showTechnicalDetails}
          info={info}
          drafting={drafting}
          shell={shell}
          takeoffState={takeoffState}
          takeoff={takeoff}
          ai={ai}
          conversationHistory={conversationHistory}
          comments={comments}
          recognition={{ revision: batch, run: runDrawingCommand }}
        />
      </div>
      {commandLineOn && (
        <CommandLine state={commandState} prompt={prompt} onAction={onCommand} inputRef={commandInput}
          words={shell.words} onTyped={typeCommand} coreActive={coreActive} />
      )}
      <StatusBar
        cursor={measure.cursor}
        // With no drawing open, the Options dialog's decimals.
        precision={drafting?.precision ?? options.displayPrecision}
        metresPerUnit={info ? metresPerUnit(info.units) : 1}
        snapLabel={measure.snap ? t(`snap.${measure.snap.kind}`) : null}
        drafting={info ? drafting : null}
        onToggle={toggleDrafting}
        onDraftingSettings={() => setDraftingOpen(true)}
        units={units}
        unitsWarning={info?.units === "unknown"}
        problems={problems}
        message={message}
        waiting={waiting}
        progress={progress}
        ucs={info?.ucs}
      />
      <FileDialogs drawings={drawings} />
      <Notices theme={theme} />
    </div>
  );
}
