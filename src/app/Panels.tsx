/** The window's three columns: the layers panel and the properties palette on the left, the
 *  drawing in the middle (file tabs, the canvas with what floats over it, the split view, sheet
 *  tabs), and Mizan's column (the conversation, and the outputs in its place) on the right. */
import { useState } from "react";
import { uiError } from "../i18n/errors";
import { RecognitionPanel } from "../shell/RecognitionPanel";
import type { Drafting, DrawingInfo, Sheet, TabInfo } from "../api";
import { AssistantPanel } from "../ai/AssistantPanel";
import type { ConversationHistoryState } from "../ai/ConversationHistory";
import { BoqPanel } from "../boq/BoqPanel";
import { ConflictsPanel } from "../boq/ConflictsPanel";
import { ArtifactFrame } from "../shell/ArtifactFrame";
import { OUTPUT_IDS, outputList, type OutputId } from "../shell/outputs";
import { useT } from "../i18n";
import { ImageDisplayPanel } from "../shell/AttachImageDialog";
import { BlockEditBar } from "../shell/BlockDialogs";
import { ConstraintBar } from "../shell/ConstraintBar";
import { FileTabs } from "../shell/FileTabs";
import { AlongPrompt, InfoCard } from "../shell/InfoCard";
import { LayersPanel } from "../shell/LayersPanel";
import { PropertiesPanel } from "../shell/PropertiesPanel";
import type { Tool } from "../shell/Ribbon";
import { SheetTabs } from "../shell/SheetTabs";
import { TakeoffPanel } from "../shell/TakeoffPanel";
import { InPlaceText } from "../shell/TextEditor";
import type { Target } from "../viewport/batch";
import type { Point } from "../viewport/camera";
import type { CanvasColours } from "../viewport/colours";
import type { Theme } from "../viewport/gl";
import { CanvasMenu } from "../viewport/CanvasMenu";
import { SplitPane } from "../viewport/SplitPane";
import { Viewport } from "../viewport/Viewport";
import { type Measure, NO_VIEWPORTS } from "./shared";
import type { Annotate } from "./useAnnotate";
import type { AssistantWiring, Pointing } from "./useAssistantWiring";
import type { Attach } from "./useAttach";
import type { Blocks } from "./useBlocks";
import type { Drawings } from "./useDrawings";
import type { Layers } from "./useLayers";
import type { Parametric } from "./useParametric";
import type { Selection } from "./useSelection";
import type { Shell } from "./useShell";
import type { Sheets } from "./useSheets";
import type { Takeoff, TakeoffState } from "./useTakeoff";

type LeftProps = {
  info: DrawingInfo | null;
  /** The sheet on screen as the drawing lists it (a layout's viewports, its name). */
  sheetShown: Sheet | undefined;
  theme: Theme;
  drafting: Drafting | null;
  sheets: Pick<Sheets, "hidden" | "sheet">;
  layers: Layers;
  selection: Selection;
  /** Counts up when the canvas menu asks to see the properties palette. */
  revealProperties: number;
};

/** The layers panel and, under it, the properties palette. */
export function LeftPanels({ info, sheetShown, theme, drafting, sheets, layers, selection, revealProperties }: LeftProps) {
  const { hidden, sheet } = sheets;
  const {
    layersBusy,
    toggleLayer,
    isolateLayer,
    unisolateLayers,
    showAll,
    makeCurrent,
    setLayerProp,
    renameLayer,
    deleteLayer,
    newLayer,
    openLayerStates,
    openTranslator,
    setViewportFrozen,
    ownLayers,
    setCurrentState,
  } = layers;
  const { pickedInfo, pickedMany, setPickedProperty } = selection;
  const sheetName = sheetShown?.name ?? sheet;

  return (
    <div className="left">
      <LayersPanel
        layers={info?.layers ?? []}
        hidden={hidden}
        theme={theme}
        busy={layersBusy}
        onToggle={toggleLayer}
        onIsolate={isolateLayer}
        isolated={info?.isolated_layer ?? null}
        onUnisolate={unisolateLayers}
        onShowAll={showAll}
        current={info?.current.layer}
        onMakeCurrent={info ? makeCurrent : undefined}
        linetypes={info?.linetypes}
        onSetLayer={info ? setLayerProp : undefined}
        onRenameLayer={info ? renameLayer : undefined}
        onDeleteLayer={info ? deleteLayer : undefined}
        onNewLayer={info ? newLayer : undefined}
        onLayerStates={info ? openLayerStates : undefined}
        onTranslate={info ? openTranslator : undefined}
        viewports={sheetShown?.is_model ? NO_VIEWPORTS : info?.viewports}
        onViewportFrozen={info ? setViewportFrozen : undefined}
      />
      <PropertiesPanel
        entity={pickedInfo}
        many={pickedMany}
        units={info?.units ?? "metres"}
        precision={drafting?.precision ?? 3}
        sheet={sheetName}
        layers={ownLayers}
        linetypes={info?.linetypes}
        onSet={info && pickedInfo ? setPickedProperty : undefined}
        current={info?.current}
        onSetCurrent={info ? setCurrentState : undefined}
        reveal={revealProperties}
      />
    </div>
  );
}

type WorkProps = {
  info: DrawingInfo | null;
  fileTabs: TabInfo[];
  activeTab: number | null;
  tool: Tool;
  measure: Measure;
  /** A click in the drawing (the command line's `clickDrawing`). */
  clickDrawing(world: Point, screen: Point, worldPerPixel: number, shift: boolean): void;
  highlight: Target[];
  overlay: Float64Array | null;
  drafting: Drafting | null;
  shell: Shell;
  sheets: Sheets;
  attach: Pick<Attach, "pending">;
  blocks: Pick<Blocks, "closeBlockEdit">;
  annotate: Annotate;
  drawings: Pick<Drawings, "switchTab" | "closeFileTab" | "switching">;
  selection: Selection;
  /** The Options dialog's background and highlight colours (null: the theme's). */
  colours: CanvasColours;
  parametric: Pick<Parametric, "constraints" | "deleteConstraint" | "hoverConstraint" | "barHidden" | "setBarHidden">;
  /** The right-click menu: a tool chosen, the properties palette shown, Mizan asked about the
   *  selection. */
  chooseTool(tool: Tool): void;
  onProperties(): void;
  askMizan?: Pick<Pointing, "selection" | "ask">;
  /** A tap of Space in the drawing: repeat the last tool. */
  onSpaceTap?(): void;
};

/** The drawing: the edit in place's bar, the file tabs, the canvas (with the along prompt, the
 *  in-place text editor and the image sliders over it), the split view, the sheet tabs. */
export function WorkArea({
  info,
  fileTabs,
  activeTab,
  tool,
  measure,
  clickDrawing,
  highlight,
  overlay,
  drafting,
  shell,
  sheets,
  attach,
  blocks,
  annotate,
  drawings,
  selection,
  colours,
  parametric,
  chooseTool,
  onProperties,
  askMizan,
  onSpaceTap,
}: WorkProps) {
  const { theme, setError, run, focusDrawing, viewport, viewportBox } = shell;
  const {
    batch,
    batchHeld,
    atlas,
    pictures,
    inset,
    split,
    setSplit,
    sheet,
    redrawGlyphs,
    syncPictures,
    syncAtlas,
    chooseSheet,
    ucsIcon,
  } = sheets;
  const { pending } = attach;
  const { closeBlockEdit } = blocks;
  const { textEditor, setTextEditor, currentTextStyle, currentTextHeight, placeInPlace } = annotate;
  const { switchTab, closeFileTab, switching } = drawings;
  const { imageChosen, setImageChosen, setChosenDisplay } = selection;

  return (
    <main className="work">
      {info?.block_edit && (
        <BlockEditBar
          name={info.block_edit}
          onSave={() => closeBlockEdit(true)}
          onDiscard={() => closeBlockEdit(false)}
        />
      )}
      <FileTabs
        tabs={fileTabs}
        active={activeTab}
        onSelect={(id) => id !== activeTab && switchTab(id)}
        onClose={(id) => closeFileTab(id)}
      />
      <div className="views">
        <CanvasMenu
          box={<div className="viewport" ref={viewportBox} tabIndex={0} />}
          hasDrawing={info !== null}
          onTool={chooseTool}
          onProperties={onProperties}
          onAskMizan={askMizan?.selection ? askMizan.ask : undefined}
          onZoomExtents={() => {
            viewport.current?.fitExtents();
            focusDrawing();
          }}
          aliases={shell.commandAliases}
        >
          <Viewport
            ref={viewport}
            batch={batch}
            atlas={atlas}
            onZoomHint={redrawGlyphs}
            pictures={pictures}
            onPictureHint={(px, visible) => run(() => syncPictures(batchHeld.current, px, visible))}
            highlight={highlight}
            overlay={overlay}
            tool={tool}
            theme={theme}
            colours={colours}
            onClick={clickDrawing}
            boxes={measure.boxes && !pending}
            onBox={(bounds, crossing, shift) => {
              if (switching.current === 0) measure.onBox(bounds, crossing, shift);
            }}
            grid={drafting?.grid ? drafting.grid_spacing : null}
            ucs={info?.ucs}
            ucsIcon={ucsIcon}
            inset={inset}
            onMove={measure.onMove}
            onWindowZoomed={measure.cancel}
            onSpaceTap={onSpaceTap}
          />
          {info && !parametric.barHidden && (
            <ConstraintBar
              constraints={parametric.constraints}
              onDelete={parametric.deleteConstraint}
              onHover={parametric.hoverConstraint}
              onHide={() => {
                parametric.hoverConstraint(null);
                parametric.setBarHidden(true);
              }}
            />
          )}
          {measure.asking && info && (
            <AlongPrompt
              key={measure.asking}
              kind={measure.asking}
              units={info.units}
              onAnswer={(value) => {
                measure.answer(value);
                focusDrawing();
              }}
              onCancel={() => {
                measure.cancel();
                focusDrawing();
              }}
            />
          )}
          {textEditor && info && (
            <InPlaceText
              key={textEditor.key}
              at={textEditor.at}
              styles={info.text_styles}
              style={currentTextStyle}
              height={textEditor.request.height ?? currentTextHeight}
              onSubmit={(d) => placeInPlace(textEditor.request, d)}
              onCancel={() => {
                setTextEditor(null);
                focusDrawing();
              }}
            />
          )}
          {imageChosen && (
            <ImageDisplayPanel
              value={imageChosen.display}
              onCommit={(display) => setChosenDisplay(imageChosen.handle, display)}
              onClose={() => setImageChosen(null)}
            />
          )}
        </CanvasMenu>
        {split && info && (
          <SplitPane
            sheets={info.sheets}
            mainSheet={sheet}
            mainBatch={batch}
            mainInset={inset}
            atlas={atlas}
            theme={theme}
            colours={colours}
            ucs={info.ucs}
            ucsIcon={ucsIcon}
            syncAtlas={syncAtlas}
            onError={(e) => setError(uiError(e))}
            onClose={() => setSplit(false)}
          />
        )}
      </div>
      {info && <SheetTabs sheets={info.sheets} active={sheet} onSelect={chooseSheet} />}
    </main>
  );
}

type SideProps = {
  showTechnicalDetails?: boolean;
  info: DrawingInfo | null;
  drafting: Drafting | null;
  shell: Pick<Shell, "sideTab" | "setSideTab">;
  takeoffState: TakeoffState;
  takeoff: Takeoff;
  ai: AssistantWiring;
  conversationHistory: ConversationHistoryState;
  recognition: { revision: unknown; run: Layers["runDrawingCommand"] };
  /** Writes the take-off to Excel (the take-off output's action). */
  onExportTakeoff?(): void;
  /** The /compare command: ask for a BOQ and find its conflicts. */
  onSlashCompare?(): void;
  /** Opens "Connect an AI app". */
  onConnect?(): void;
  /** A key asked for Mizan's box. */
  ask?: { n: number; text: string };
};

/** The right column (design B): Mizan's conversation, and, when Mizan's work is opened, the take-off,
 *  BOQ, conflicts or take-off report in the same column in place of the conversation, with a way
 *  back. The conversation and every output stay mounted, one shown, so a draft, a search or a
 *  running answer survives the switch. */
export function SidePanel({ showTechnicalDetails, info, drafting, shell, takeoffState, takeoff, ai, conversationHistory, recognition, onExportTakeoff, onSlashCompare, onConnect, ask }: SideProps) {
  const t = useT();
  const { sideTab, setSideTab } = shell;
  const { items, itemTotals, selectedId, bill, generated, boqCode, conflicts, compared, infoCard, setInfoCard } = takeoffState;
  const {
    selectRow,
    removeRow,
    renameRow,
    linkRow,
    selectBoq,
    onImportBoq,
    setTolerance,
    onCompare,
    onExportConflicts,
    showConflict,
    resolveConflict,
    openBoqTab,
  } = takeoff;
  const { assistant, showCard, undoCard, openSettings, pointing } = ai;
  const shown: OutputId | null = sideTab === "assistant" ? null : sideTab;
  const drawing = info?.path.split(/[\\/]/).pop() ?? "";
  const outputs = outputList(takeoffState);
  /** The take-off report reads again when its head's Refresh is pressed. */
  const [reviewRefresh, setReviewRefresh] = useState(0);
  const meta = (id: OutputId) => {
    const count = outputs.find((o) => o.id === id)?.count;
    // The BOQ is a workbook: its name says where it came from.
    const name = id === "boq" && bill ? (bill.source.split(/[\\/]/).pop() ?? drawing) : drawing;
    return [name, count ? t(count === 1 ? "outputs.oneItem" : "outputs.items", { n: count }) : ""].filter(Boolean).join(" · ");
  };
  const body = (id: OutputId) => {
    switch (id) {
      case "takeoff":
        return (
          <TakeoffPanel
            items={items}
            totals={itemTotals}
            selectedId={selectedId}
            onSelect={selectRow}
            onRemove={removeRow}
            onRename={renameRow}
            boq={bill}
            onLink={linkRow}
            precision={drafting?.precision ?? 3}
          />
        );
      case "boq":
        return (
          <BoqPanel
            boq={bill}
            generated={generated}
            onShowGenerated={takeoff.showGenerated}
            takeoff={items}
            selectedCode={boqCode}
            onSelect={selectBoq}
            onTolerance={setTolerance}
          />
        );
      case "conflicts":
        return (
          <ConflictsPanel
            conflicts={conflicts}
            compared={compared}
            hasBoq={bill !== null}
            onShow={showConflict}
            onResolve={resolveConflict}
            onOpenBoq={openBoqTab}
          />
        );
      case "review":
        return <RecognitionPanel active={sideTab === "review"} drawing={info?.path ?? null} revision={recognition.revision} run={recognition.run} refresh={reviewRefresh} />;
    }
  };
  /** The buttons of an output's head: what its panel used to carry, now in one place. */
  const actionsOf = (id: OutputId) => {
    switch (id) {
      case "takeoff":
        return [{ id: "export", label: t("exportTakeoff"), onClick: () => onExportTakeoff?.(), disabled: !onExportTakeoff || items.length === 0 }];
      case "boq":
        return [{ id: "import", label: t("importBoq"), onClick: onImportBoq, disabled: !info }];
      case "conflicts":
        return [
          { id: "compare", label: t("compare"), onClick: onCompare, disabled: !info, primary: true },
          { id: "export", label: t("exportConflicts"), onClick: onExportConflicts, disabled: !info || conflicts.length === 0 },
        ];
      case "review":
        return [{ id: "refresh", label: t("recognition.refresh"), onClick: () => setReviewRefresh((n) => n + 1), disabled: !info }];
    }
  };
  const frames = OUTPUT_IDS.map((id) => (
    <div key={id} className="art-item" hidden={sideTab !== id}>
      <ArtifactFrame
        id={id}
        outputs={outputs}
        meta={meta(id)}
        actions={actionsOf(id)}
        onSelect={setSideTab}
        onBack={() => setSideTab("assistant")}
      >
        <div className="side-panel">{body(id)}</div>
      </ArtifactFrame>
    </div>
  ));

  return (
    <aside className={shown ? "side col art-on" : "side col"} aria-label={t("assistant")}>
      {infoCard && <InfoCard info={infoCard} onClose={() => setInfoCard(null)} />}
      <AssistantPanel
        showTechnicalDetails={showTechnicalDetails}
        assistant={assistant}
        drawing={info ? { path: info.path, revision: recognition.revision } : undefined}
        history={conversationHistory}
        onShow={showCard}
        onUndo={undoCard}
        onSettings={openSettings}
        pointing={pointing}
        artifact={{ shown, node: frames, onBack: () => setSideTab("assistant") }}
        onCompare={onSlashCompare}
        onConnect={onConnect}
        ask={ask}
      />
    </aside>
  );
}
