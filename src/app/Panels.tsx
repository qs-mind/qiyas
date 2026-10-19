/** The window's three columns: the layers panel and the properties palette on the left, the
 *  drawing in the middle (file tabs, the canvas with what floats over it, the split view, sheet
 *  tabs), and the side panel (take-off, BOQ, conflicts, Mizan) on the right. */
import { MizanMark } from "../ai/MizanMark";
import { Tabs } from "@base-ui/react/tabs";
import type { Drafting, DrawingInfo, Sheet, TabInfo } from "../api";
import { AssistantPanel } from "../ai/AssistantPanel";
import { BoqPanel } from "../boq/BoqPanel";
import { ConflictsPanel } from "../boq/ConflictsPanel";
import { openCount } from "../boq/conflictsMath";
import { useT } from "../i18n";
import { ImageDisplayPanel } from "../shell/AttachImageDialog";
import { BlockEditBar } from "../shell/BlockDialogs";
import { CommentsPanel } from "../shell/CommentsPanel";
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
import type { Comments } from "./useComments";
import type { Drawings } from "./useDrawings";
import type { Layers } from "./useLayers";
import { SideTabs, type SideTabItem } from "./SideTabs";
import type { Parametric } from "./useParametric";
import type { Selection } from "./useSelection";
import type { Shell, SideTab } from "./useShell";
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
            onError={(e) => setError(String(e))}
            onClose={() => setSplit(false)}
          />
        )}
      </div>
      {info && <SheetTabs sheets={info.sheets} active={sheet} onSelect={chooseSheet} />}
    </main>
  );
}

type SideProps = {
  info: DrawingInfo | null;
  drafting: Drafting | null;
  shell: Pick<Shell, "sideTab" | "setSideTab">;
  takeoffState: TakeoffState;
  takeoff: Takeoff;
  ai: AssistantWiring;
  comments: Comments;
};

/** The side panel: its tabs, the inquiry card, and the take-off, BOQ, conflicts, comments and
 *  Mizan panels (all mounted, one shown). */
export function SidePanel({ info, drafting, shell, takeoffState, takeoff, ai, comments }: SideProps) {
  const t = useT();
  const { sideTab, setSideTab } = shell;
  const { items, itemTotals, selectedId, bill, boqCode, conflicts, compared, infoCard, setInfoCard } = takeoffState;
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
  const openConflicts = openCount(conflicts);

  const tabs: SideTabItem<SideTab>[] = [
    { id: "takeoff", label: t("takeoff") },
    { id: "boq", label: t("boq") },
    {
      id: "conflicts",
      label: t("conflicts"),
      extra:
        openConflicts > 0 ? (
          <span className="tab-count" title={t("conflicts.openCount", { n: openConflicts })}>
            {openConflicts}
          </span>
        ) : undefined,
    },
    { id: "comments", label: t("comments") },
    { id: "assistant", label: t("assistant"), extra: assistant.busy ? <MizanMark /> : undefined },
  ];

  // The BOQ table and the conflicts need more room than the take-off. All the panels stay mounted,
  // so a draft, a search or a running answer survives a tab switch.
  return (
    <SideTabs<SideTab>
      className={sideTab === "boq" || sideTab === "conflicts" ? "side wide" : "side"}
      tabs={tabs}
      value={sideTab}
      onChange={setSideTab}
    >
      {infoCard && <InfoCard info={infoCard} onClose={() => setInfoCard(null)} />}
      <Tabs.Panel value="takeoff" keepMounted tabIndex={-1} className="side-panel">
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
      </Tabs.Panel>
      <Tabs.Panel value="boq" keepMounted tabIndex={-1} className="side-panel">
        <BoqPanel
          boq={bill}
          takeoff={items}
          selectedCode={boqCode}
          onSelect={selectBoq}
          onImport={info ? onImportBoq : undefined}
          onTolerance={setTolerance}
        />
      </Tabs.Panel>
      <Tabs.Panel value="conflicts" keepMounted tabIndex={-1} className="side-panel">
        <ConflictsPanel
          conflicts={conflicts}
          compared={compared}
          hasBoq={bill !== null}
          onCompare={info ? onCompare : undefined}
          onExport={info ? onExportConflicts : undefined}
          onShow={showConflict}
          onResolve={resolveConflict}
          onOpenBoq={openBoqTab}
        />
      </Tabs.Panel>
      <Tabs.Panel value="comments" keepMounted tabIndex={-1} className="side-panel">
        <CommentsPanel
          comments={comments.comments}
          sheets={info?.sheets ?? []}
          onRefresh={comments.refresh}
          onShow={comments.show}
        />
      </Tabs.Panel>
      <Tabs.Panel value="assistant" keepMounted tabIndex={-1} className="side-panel">
        <AssistantPanel
          assistant={assistant}
          onShow={showCard}
          onUndo={undoCard}
          onSettings={openSettings}
          pointing={pointing}
        />
      </Tabs.Panel>
    </SideTabs>
  );
}
