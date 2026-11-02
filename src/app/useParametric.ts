/** Parametric constraints in the window: the constraint bar's list (read again whenever the
 *  drawing on screen is drawn again: every change, undo, redo and sheet), the parameters manager
 *  (PARAMETERS) and its edits, and deleting a constraint from the bar. The tools themselves are
 *  `src/tools/constraintTools.ts`, run through `useTool`. */
import { useDialogOpen } from "./useDialogs";
import { useEffect, useState, type RefObject } from "react";
import {
  listConstraints,
  listParameters,
  runCommand,
  type ConstraintInfo,
  type DrawingInfo,
  type ParameterInfo,
} from "../api";
import type { Tool } from "../shell/Ribbon";
import { isConstraintTool } from "../tools/constraintTools";
import type { Batch } from "../viewport/batch";
import type { Commands } from "./useCommands";
import type { Shell } from "./useShell";
import type { Sheets } from "./useSheets";
import type { TakeoffState } from "./useTakeoff";

type Deps = Pick<Shell, "run"> &
  Pick<Sheets, "redraw"> &
  Pick<TakeoffState, "refreshTakeoffAndBoq" | "setAiHighlight"> & {
    info: DrawingInfo | null;
    batch: Batch | null;
    tool: Tool;
    autosave(): Promise<void>;
    /** The root's `apply`, filled after this hook runs (the bar's deletes go through it). */
    apply: RefObject<Commands["apply"] | null>;
  };

export function useParametric({
  info,
  batch,
  tool,
  run,
  redraw,
  refreshTakeoffAndBoq,
  setAiHighlight,
  autosave,
  apply,
}: Deps) {
  const [constraints, setConstraints] = useState<ConstraintInfo[]>([]);
  const [parameters, setParameters] = useState<ParameterInfo[]>([]);
  const [parametersOpen, setParametersOpen] = useDialogOpen("parameters");
  /** The bar hidden until it is asked for again (its × , or the ribbon's button). */
  const [barHidden, setBarHidden] = useState(false);
  /** The drawing on screen may hold constraints: it had some when read, or a parametric tool
   *  or the manager was used since. Only then is the list read after each redraw. */
  const [used, setUsed] = useState(false);
  useEffect(() => {
    if (isConstraintTool(tool)) setUsed(true);
  }, [tool]);
  const wanted = !!info && (info.has_constraints === true || used || parametersOpen);

  // Every redraw follows a change of the drawing on screen: read the lists again.
  useEffect(() => {
    if (!info || !wanted) {
      setConstraints([]);
      setParameters([]);
      return;
    }
    let gone = false;
    listConstraints().then(
      (c) => !gone && setConstraints(c),
      () => !gone && setConstraints([]),
    );
    if (parametersOpen) {
      listParameters().then(
        (p) => !gone && setParameters(p),
        () => !gone && setParameters([]),
      );
    }
    return () => {
      gone = true;
    };
  }, [batch, info, parametersOpen, wanted]);

  function deleteConstraint(id: number) {
    setAiHighlight([]);
    void apply.current?.({ type: "delete_constraint", id });
  }

  /** The parameter `name` takes `expression`; throws the core's refusal for the dialog to show. */
  async function setParameter(name: string, expression: string) {
    await runCommand({ type: "set_parameter", name, expression });
    await run(async () => {
      await refreshTakeoffAndBoq();
      await redraw();
    });
    await autosave();
  }

  return {
    constraints,
    parameters,
    parametersOpen,
    openParameters: () => setParametersOpen(true),
    closeParameters: () => setParametersOpen(false),
    setParameter,
    deleteConstraint,
    /** Highlights what a glyph holds while the pointer is on it. */
    hoverConstraint: (handles: number[] | null) => setAiHighlight(handles ?? []),
    barHidden,
    setBarHidden,
  };
}

export type Parametric = ReturnType<typeof useParametric>;
