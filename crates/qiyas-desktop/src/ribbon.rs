//! The ribbon as data: tabs, their groups and the groups' buttons, each with the command it runs.
//! `default_ribbon` is the layout Qiyas ships, the one `src/shell/Ribbon.tsx` draws
//! (`src/shell/ribbon.default.json` pins the two to one JSON); the user's own layout lives in `settings.json` (`Settings::ribbon`, null for the
//! default), so the Customize dialog can change it and reset it. Data only: the window draws it.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// Tabs a layout holds at most.
pub const MAX_TABS: usize = 12;
/// Groups a tab holds at most.
pub const MAX_GROUPS: usize = 20;
/// Buttons a group holds at most.
pub const MAX_BUTTONS: usize = 60;
/// The longest id or label, in characters.
pub const MAX_LABEL: usize = 60;

/// How the window runs a button.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ButtonKind {
    /// Picks a tool (`Tool` in `src/viewport/Viewport.tsx`): line, trim, dimLinear...
    Tool,
    /// Runs an action: opens a dialog, saves, undoes...
    Action,
    /// A checkbox: group selection, Arabic text reversed.
    Toggle,
}

/// One button: what it runs and its label (an i18n key, or the user's own text).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RibbonButton {
    pub kind: ButtonKind,
    pub command: String,
    pub label: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RibbonGroup {
    pub id: String,
    pub label: String,
    pub buttons: Vec<RibbonButton>,
    /// How many of the buttons the group shows; the rest wait behind its "⋯" button (all when
    /// left out).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RibbonTab {
    pub id: String,
    pub label: String,
    pub groups: Vec<RibbonGroup>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RibbonLayout {
    pub tabs: Vec<RibbonTab>,
}

fn button(kind: ButtonKind, command: &str, label: &str) -> RibbonButton {
    RibbonButton {
        kind,
        command: command.into(),
        label: label.into(),
    }
}

fn tool(command: &str, label: &str) -> RibbonButton {
    button(ButtonKind::Tool, command, label)
}

fn action(command: &str, label: &str) -> RibbonButton {
    button(ButtonKind::Action, command, label)
}

/// Tools labelled `<prefix>.<name>`.
fn tools(prefix: &str, names: &[&str]) -> Vec<RibbonButton> {
    names
        .iter()
        .map(|n| tool(n, &format!("{prefix}.{n}")))
        .collect()
}

fn group(id: &str, label: &str, buttons: Vec<RibbonButton>) -> RibbonGroup {
    RibbonGroup {
        id: id.into(),
        label: label.into(),
        buttons,
        main: None,
    }
}

/// A group that shows its first `main` buttons and keeps the rest behind its "⋯" button.
fn group_main(id: &str, label: &str, buttons: Vec<RibbonButton>, main: usize) -> RibbonGroup {
    RibbonGroup {
        main: Some(main),
        ..group(id, label, buttons)
    }
}

fn tab(id: &str, groups: Vec<RibbonGroup>) -> RibbonTab {
    tab_labelled(id, id, groups)
}

fn tab_labelled(id: &str, label: &str, groups: Vec<RibbonGroup>) -> RibbonTab {
    RibbonTab {
        id: id.into(),
        label: label.into(),
        groups,
    }
}

/// Every dimension tool, then the dimension style: the Dimensions group of Measure (its first
/// five on show) and of Annotate (all of them).
fn dimensions() -> Vec<RibbonButton> {
    let mut buttons = tools(
        "dim",
        &[
            "dimLinear",
            "dimAligned",
            "dimAngular",
            "dimRadius",
            "dimDiameter",
            "dimArc",
            "dimOrdinate",
            "dimBaseline",
            "dimContinue",
            "qdim",
            "dimEdit",
            "dimBreak",
            "dimSpace",
            "dimJogLine",
        ],
    );
    buttons.push(action("dimStyle", "dim.styleButton"));
    buttons
}

/// The ribbon Qiyas ships: Measure (Measure, Inquiry, Take-off, Dimensions, Markup), Draw (the tab
/// with id "home": Draw, Modify, Edit, Annotation, Clipboard, Layers), Annotate (Dimensions,
/// Leaders, Markup, Tables), then, under "More" in the window, Parametric
/// (geometric and dimensional constraints, Parameters manager, Delete constraints, the constraint
/// bar), Insert, View, Manage (Run script; Audit, Purge and Recover; Options, Customize and
/// Aliases) and Output (Plot, Quick print, Print all layouts, Plot styles), as `Ribbon.tsx` draws
/// them. Labels are i18n keys. New, Open, Save, Save as, Undo and Redo are not here: they are the
/// window's quick-access bar beside the tabs, always shown.
pub fn default_ribbon() -> RibbonLayout {
    use ButtonKind::Toggle;
    let measure = tab(
        "measure",
        vec![
            group(
                "measure",
                "ribbon.measure",
                ["select", "distance", "area", "count"]
                    .iter()
                    .map(|n| tool(n, n))
                    .collect(),
            ),
            group("inquiry", "ribbon.inquiry", {
                let mut buttons: Vec<RibbonButton> =
                    ["geometry", "divide", "measureAlong", "idPoint"]
                        .iter()
                        .map(|n| tool(n, n))
                        .collect();
                buttons.push(action("extract", "extract"));
                buttons
            }),
            group(
                "takeoff",
                "ribbon.takeoff",
                vec![
                    action("exportTakeoff", "exportTakeoff"),
                    action("importBoq", "importBoq"),
                    action("compare", "compare"),
                    action("exportConflicts", "exportConflicts"),
                ],
            ),
            group_main("dims", "dim.group", dimensions(), 5),
            group(
                "markup",
                "ribbon.markup",
                vec![
                    tool("revcloud", "draw.revcloud"),
                    tool("commentCloud", "dim.commentCloud"),
                    tool("leader", "dim.leader"),
                    tool("mtext", "text.mtext"),
                ],
            ),
        ],
    );
    let home = tab_labelled(
        "home",
        "draw.group",
        vec![
            group(
                "draw",
                "draw.group",
                tools(
                    "draw",
                    &[
                        "line",
                        "polyline",
                        "rectangle",
                        "circle",
                        "arc",
                        "ellipse",
                        "spline",
                        "point",
                        "hatch",
                        "revcloud",
                        "wipeout",
                    ],
                ),
            ),
            group(
                "modify",
                "modify.group",
                tools(
                    "modify",
                    &[
                        "move", "copy", "rotate", "scale", "mirror", "stretch", "align", "array",
                        "explode", "erase",
                    ],
                ),
            ),
            group(
                "edit",
                "ribbon.edit",
                tools(
                    "edit",
                    &[
                        "trim", "extend", "break", "join", "lengthen", "reverse", "offset",
                        "fillet", "chamfer", "blend", "pedit", "overkill", "flatten",
                    ],
                ),
            ),
            group("text", "ribbon.annotation", {
                let mut buttons = tools("text", &["text", "mtext", "textEdit"]);
                buttons.push(action("find", "text.find"));
                buttons
            }),
            group(
                "clipboard",
                "clip.group",
                tools("clip", &["copyclip", "copybase", "pasteclip", "pasteblock"]),
            ),
            group("layers", "layers", {
                let mut buttons = vec![
                    action("layerStates", "layer.statesButton"),
                    action("translateLayers", "layer.translateButton"),
                ];
                buttons.extend(tools("layerTool", &["group", "ungroup", "matchprop"]));
                buttons.push(button(Toggle, "groupSelect", "group.select"));
                buttons
            }),
        ],
    );
    let annotate = tab(
        "annotate",
        vec![
            group("dims", "dim.group", dimensions()),
            group(
                "notes",
                "dim.notes",
                tools("dim", &["leader", "mleader", "tolerance"]),
            ),
            // The revision cloud, and the comment cloud (a cloud with its comment).
            group(
                "markup",
                "ribbon.markup",
                vec![
                    tool("revcloud", "draw.revcloud"),
                    tool("commentCloud", "dim.commentCloud"),
                ],
            ),
            group(
                "tables",
                "table.group",
                vec![
                    tool("table", "table.table"),
                    tool("tableEdit", "table.tableEdit"),
                    tool("field", "table.fieldTool"),
                    action("updateFields", "table.updateFields"),
                ],
            ),
        ],
    );
    let insert = tab(
        "insert",
        vec![group(
            "block",
            "block.group",
            vec![
                tool("block", "block.createTool"),
                action("blockPalette", "block.insertTool"),
                action("writeBlock", "block.writeTool"),
                tool("refedit", "block.refedit"),
                tool("attdef", "block.attdefTool"),
                tool("textEdit", "block.attedit"),
                action("attributeManager", "block.manager"),
                action("designCenter", "block.designCenter"),
                action("toolPalettes", "block.toolPalettes"),
            ],
        )],
    );
    let view = tab(
        "view",
        vec![
            group(
                "views",
                "ribbon.views",
                vec![
                    action("zoomExtents", "zoomExtents"),
                    tool("zoomWindow", "zoomWindow"),
                    action("views", "views"),
                    action("xrefs", "xrefs"),
                    action("attachPdf", "attachPdf"),
                    action("attachImage", "attachImage"),
                    action("calibrate", "calibrate"),
                ],
            ),
            group(
                "vports",
                "vport.group",
                vec![
                    tool("mview", "vport.mview"),
                    action("split", "vport.two"),
                    tool("ucs", "ucs.tool"),
                    tool("ucsObject", "ucs.object"),
                    action("ucsWorld", "ucs.world"),
                    action("sheetSet", "sheetSet.button"),
                ],
            ),
            group(
                "display",
                "ribbon.display",
                vec![
                    button(Toggle, "arabicReversed", "arabicReversed"),
                    button(Toggle, "ucsIcon", "ucsIcon"),
                ],
            ),
        ],
    );
    let manage = tab(
        "manage",
        vec![
            group(
                "scripts",
                "ribbon.scripts",
                vec![action("runScript", "script.run")],
            ),
            group(
                "drawing",
                "ribbon.drawing",
                vec![
                    action("audit", "audit.button"),
                    action("purge", "purge.button"),
                    action("recover", "recover.button"),
                ],
            ),
            group(
                "setup",
                "ribbon.setup",
                vec![
                    action("options", "options.button"),
                    action("customize", "cui.button"),
                    action("aliases", "aliasEdit.button"),
                ],
            ),
        ],
    );
    let output = tab(
        "output",
        vec![
            group(
                "plot",
                "ribbon.plot",
                vec![
                    action("plot", "plot.button"),
                    action("quickPrint", "plot.quickPrint"),
                    action("printAll", "plot.printAll"),
                ],
            ),
            group(
                "plotStyles",
                "ribbon.plotStyles",
                vec![action("plotStyles", "plot.styles")],
            ),
        ],
    );
    let parametric = tab(
        "parametric",
        vec![
            group(
                "geometric",
                "param.geometric",
                tools(
                    "param",
                    &[
                        "gcCoincident",
                        "gcCollinear",
                        "gcConcentric",
                        "gcFixed",
                        "gcParallel",
                        "gcPerpendicular",
                        "gcHorizontal",
                        "gcVertical",
                        "gcTangent",
                        "gcSmooth",
                        "gcSymmetric",
                        "gcEqual",
                        "autoConstrain",
                    ],
                ),
            ),
            group(
                "dimensional",
                "param.dimensional",
                tools(
                    "param",
                    &[
                        "dcLinear",
                        "dcAligned",
                        "dcAngular",
                        "dcRadius",
                        "dcDiameter",
                    ],
                ),
            ),
            group(
                "constraints",
                "param.manage",
                vec![
                    action("parameters", "param.parameters"),
                    tool("delConstraint", "param.delConstraint"),
                    action("constraintBar", "param.showBar"),
                ],
            ),
        ],
    );
    RibbonLayout {
        tabs: vec![
            measure, home, annotate, parametric, insert, view, manage, output,
        ],
    }
}

/// The default ribbon, for the Customize dialog's "Reset to default".
#[tauri::command]
pub async fn get_default_ribbon() -> RibbonLayout {
    default_ribbon()
}

fn check_name(what: &str, text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().count() > MAX_LABEL {
        return Err(format!(
            "A ribbon {what} needs a name of 1 to {MAX_LABEL} characters"
        ));
    }
    Ok(())
}

/// Checks a user's ribbon before it is kept: at most `MAX_TABS` tabs of `MAX_GROUPS` groups of
/// `MAX_BUTTONS` buttons, ids unique (tabs, and groups within a tab), every id and label 1 to
/// `MAX_LABEL` characters, and every button one the default ribbon has (its kind and command),
/// so the window always knows how to run it. A button may appear more than once.
pub fn validate(layout: &RibbonLayout) -> Result<(), String> {
    let known: HashSet<(ButtonKind, String)> = default_ribbon()
        .tabs
        .into_iter()
        .flat_map(|t| t.groups)
        .flat_map(|g| g.buttons)
        .map(|b| (b.kind, b.command))
        .collect();
    if layout.tabs.is_empty() || layout.tabs.len() > MAX_TABS {
        return Err(format!("The ribbon holds 1 to {MAX_TABS} tabs"));
    }
    let mut tab_ids = HashSet::new();
    for tab in &layout.tabs {
        check_name("tab", &tab.id)?;
        check_name("tab", &tab.label)?;
        if !tab_ids.insert(tab.id.as_str()) {
            return Err(format!("Two ribbon tabs have the id \"{}\"", tab.id));
        }
        if tab.groups.len() > MAX_GROUPS {
            return Err(format!(
                "The ribbon tab \"{}\" holds more than {MAX_GROUPS} groups",
                tab.label
            ));
        }
        let mut group_ids = HashSet::new();
        for group in &tab.groups {
            check_name("group", &group.id)?;
            check_name("group", &group.label)?;
            if !group_ids.insert(group.id.as_str()) {
                return Err(format!(
                    "Two groups of the ribbon tab \"{}\" have the id \"{}\"",
                    tab.label, group.id
                ));
            }
            if group.buttons.len() > MAX_BUTTONS {
                return Err(format!(
                    "The ribbon group \"{}\" holds more than {MAX_BUTTONS} buttons",
                    group.label
                ));
            }
            for b in &group.buttons {
                check_name("button", &b.label)?;
                if !known.contains(&(b.kind, b.command.clone())) {
                    return Err(format!("The ribbon has no button for \"{}\"", b.command));
                }
            }
        }
    }
    Ok(())
}
