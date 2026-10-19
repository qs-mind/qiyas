//! The tool catalogue: names, descriptions written for the model, and strict input schemas.
//!
//! Every schema is an object with `additionalProperties: false` and a `required` list, and uses
//! only what the API's strict tool mode accepts (no numeric, length or item-count limits); the
//! executor checks those limits itself.

use serde_json::{json, Value};

/// One tool the model can call.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema of the input: an object, `additionalProperties: false`, with `required`.
    pub input_schema: Value,
    /// Whether the tool changes the take-off, the BOQ work or the drawing (an undo step, and the
    /// take-off panel must refresh). Showing another sheet or hiding layers is not counted.
    pub mutating: bool,
}

/// The tools, in the order the model sees them.
pub fn catalogue() -> Vec<ToolSpec> {
    vec![
        tool(
            "list_drawings",
            "List the drawings open in Qiyas (tab id, title, path, unsaved changes, untitled, on \
             screen) and the recent drawings. Works with no drawing open: use it to offer the \
             user a recent drawing or a new one.",
            json!({}),
            &[],
            false,
        ),
        tool(
            "new_drawing",
            "Make a new, empty drawing in a new tab and put it on screen, untitled until the user \
             saves it. units: \"m\", \"cm\" or \"mm\"; when the user has not said, ask which \
             before calling. The drawing summary in the system prompt is then of the earlier \
             drawing (or none): trust this tool's answer and the [state] block.",
            json!({ "units": { "type": "string", "enum": ["m", "cm", "mm"], "description": "The drawing's units." } }),
            &["units"],
            false,
        ),
        tool(
            "open_drawing",
            "Open a DWG or DXF drawing in a tab (or show its tab when it is open already) and put \
             it on screen. Use a path from list_drawings (open or recent) or one the user wrote; \
             any other path is asked about in Qiyas first, and a drawing with an autosave to \
             decide about is refused (the user opens it with Open). The answer summarises it.",
            json!({ "path": { "type": "string", "description": "The drawing file's full path." } }),
            &["path"],
            false,
        ),
        tool(
            "switch_drawing",
            "Put another open drawing on screen, by its tab id from list_drawings. Every other \
             tool works on the drawing on screen.",
            json!({ "id": { "type": "integer", "description": "The tab id." } }),
            &["id"],
            false,
        ),
        tool(
            "save_drawing",
            "Save the drawing on screen over its own file (Qiyas keeps a .bak of the old one). An \
             untitled drawing has no file yet: this opens the Save dialog for the user to choose \
             where, and does not wait for it. Save only when the user asked.",
            json!({}),
            &[],
            false,
        ),
        tool(
            "close_drawing",
            "Close a drawing's tab (the one on screen when id is left out). One with unsaved \
             changes is not closed by you: Qiyas asks the user Save, Don't Save or Cancel. Close \
             only when the user asked.",
            json!({ "id": { "type": "integer", "description": "The tab id; the drawing on screen when left out." } }),
            &[],
            false,
        ),
        tool(
            "list_sheets",
            "List the drawing's sheets (model space and paper layouts), the sheet shown now and \
             the drawing units. Coordinates, lengths and areas read from the drawing are in \
             drawing units; measurements also give their quantity in metres.",
            json!({}),
            &[],
            false,
        ),
        tool(
            "set_sheet",
            "Show another sheet. Queries and measurements that take no sheet work on the sheet \
             shown.",
            json!({ "sheet": sheet("The sheet name or block name, from list_sheets.") }),
            &["sheet"],
            false,
        ),
        tool(
            "list_layers",
            "List the drawing's layers with their colour and whether they are off, frozen, \
             locked, or hidden by the user in Qiyas.",
            json!({ "offset": offset() }),
            &[],
            false,
        ),
        tool(
            "show_layers",
            "Show or hide layers in Qiyas; the drawing file is not changed. Entities on hidden \
             layers are not drawn and not counted by count_blocks or block_summary.",
            json!({
                "names": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Layer names, from list_layers."
                },
                "visible": { "type": "boolean", "description": "true shows them, false hides them." }
            }),
            &["names", "visible"],
            false,
        ),
        tool(
            "find_entities",
            "Find entities on a sheet by layer, type, block name, text or box; every filter given \
             must match, names match in any case. Each row gives the entity's handle (an integer \
             id for get_entity, measure_length, measure_area, add_measurement and zoom_to), type, \
             layer, block name, text, length and area in drawing units, and bounds \
             [[min_x, min_y], [max_x, max_y]]. Includes entities on hidden layers, and those of \
             xrefs (other drawings shown in this one; layers named XREF|LAYER): such a row has \
             xref and chain, and its handle needs that chain. Paged: total counts every match; \
             pass next_offset as offset for the next page.",
            json!({
                "sheet": sheet("Sheet name or block name; the sheet shown when left out."),
                "layer": { "type": "string", "description": "Only entities on this layer." },
                "kind": {
                    "type": "string",
                    "description": "Only this entity type, e.g. Line, LwPolyline, Polyline, Arc, \
                                    Circle, Hatch, Insert (a block reference), Text, MText."
                },
                "block_name": { "type": "string", "description": "Only references of this block." },
                "text_contains": {
                    "type": "string",
                    "description": "Only Text, MText or block references whose text or attribute \
                                    values contain this, in any case."
                },
                "within": bbox("Only entities with at least one point inside this box."),
                "offset": offset(),
                "limit": { "type": "integer", "description": "Rows per page, at most 500. Default 100." }
            }),
            &[],
            false,
        ),
        tool(
            "get_entity",
            "One entity by handle: its type, layer, block name, text, and length and area in \
             drawing units. For an entity inside blocks, give chain too.",
            json!({
                "handle": handle("The entity's handle, from find_entities."),
                "chain": chain()
            }),
            &["handle"],
            false,
        ),
        tool(
            "read_text",
            "All text on a sheet, including text inside blocks and block attributes, with its \
             position and height in drawing units. Use it to find room names, labels, notes and \
             titles.",
            json!({
                "sheet": sheet("Sheet name or block name; the sheet shown when left out."),
                "within": bbox("Only text placed inside this box."),
                "offset": offset()
            }),
            &[],
            false,
        ),
        tool(
            "read_tables",
            "Schedules on a sheet: table entities, and grids of loose text laid out as a table \
             (door, window and finish schedules). Each has a name, columns and rows of text.",
            json!({
                "sheet": sheet("Sheet name or block name; the sheet shown when left out."),
                "offset": offset()
            }),
            &[],
            false,
        ),
        tool(
            "read_dimensions",
            "Dimensions on a sheet with the value each one measures (drawing units, or degrees \
             for angles) and any text written over it. Use them to check measured lengths.",
            json!({
                "sheet": sheet("Sheet name or block name; the sheet shown when left out."),
                "offset": offset()
            }),
            &[],
            false,
        ),
        tool(
            "block_summary",
            "How many references of each block sit on each layer of a sheet, visible layers \
             only, most first. Counts top-level INSERT records, including loaded xref tops; not nested occurrences or array cells. Not a complete visible count or project total. See result scope and limitations before count_blocks.",
            json!({
                "sheet": sheet("Sheet name or block name; the sheet shown when left out."),
                "offset": offset()
            }),
            &[],
            false,
        ),
        tool(
            "measure_length",
            "Measure a length and add it to the take-off. To get the length or perimeter of \
             something drawn (a wall, a curved wall, an arc, a polyline), find that entity \
             first (find_entities near the place or on the layer, or get_entity) and give its \
             handle: that measures the drawn geometry exactly, arcs and bulges included (a closed \
             shape gives its perimeter). Give points only to measure between points the user \
             gave you; never approximate a curve with points you chose yourself. If several \
             entities could be meant, ask the user which. Give handle or points, not both. \
             Returns the measurement with its id; shown is in metres.",
            json!({
                "handle": handle("The entity to measure, from find_entities."),
                "chain": chain(),
                "points": points("A polyline between points the user gave, in drawing \
                                  coordinates, 2 points or more.")
            }),
            &[],
            true,
        ),
        tool(
            "measure_area",
            "Measure an area and add it to the take-off. To get the area of something drawn \
             (a room, a slab, a circle, a hatch), find that entity first (find_entities near \
             the place or on the layer, or get_entity) and give its handle: that measures the \
             drawn geometry exactly, arcs and bulges included. Give points only for corners the \
             user gave you; never approximate a curve with points you chose yourself. If several \
             entities could be meant, ask the user which. To get a candidate area of an enclosed \
             space bounded by several entities, use room_areas. Give handle or points, not \
             both. Returns the measurement with its id; shown is in square metres.",
            json!({
                "handle": handle("The closed entity to measure, from find_entities."),
                "chain": chain(),
                "points": points("The corners of the ring the user gave, in drawing \
                                  coordinates, 3 points or more; it is closed for you.")
            }),
            &[],
            true,
        ),
        tool(
            "count_blocks",
            "Count the references of a block on the sheet shown, on visible layers only, and add \
             the count to the take-off in pieces. Block names come from block_summary or \
             find_entities. Counts top-level INSERT records, including loaded xref tops; not nested occurrences or array cells. Not a complete visible count or project total. Complete parent visibility, cycles, unavailable xrefs, dynamic visibility and clipping are not resolved.",
            json!({
                "block_name": { "type": "string", "description": "The block to count, in any case." },
                "layer": { "type": "string", "description": "Only count references on this layer." }
            }),
            &["block_name"],
            true,
        ),
        tool(
            "add_measurement",
            "Add a take-off line you describe. Geometry is points (drawing coordinates) or \
             handles (entities from find_entities, summed), not both. A length, area or count is \
             worked out from the geometry: a count is one piece per point or per handle, and \
             count handles must be block references. A volume is the area times height. An item \
             is a quantity in its own unit (e.g. 5 bags) and may have no geometry. For one shape \
             prefer measure_length, measure_area or count_blocks; use this to sum several \
             entities, for volumes and items, or to set the description at once. A take-off \
             row needs a real measured quantity: a row with no geometry and a quantity of 0 is \
             refused. Notes and analysis (room sizes, overall widths, comments) are not rows: \
             write them in your answer, or in the drawing with add_text or add_comment.",
            json!({
                "kind": {
                    "type": "string",
                    "enum": ["length", "area", "volume", "count", "item"],
                    "description": "What is measured."
                },
                "description": {
                    "type": "string",
                    "description": "What was measured and where, e.g. 'Block wall 200 mm, ground floor'."
                },
                "points": points("Geometry in drawing coordinates."),
                "handles": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "description": "Entities from find_entities; for a count, block references only."
                },
                "height": { "type": "number", "description": "A volume's height in metres, above 0." },
                "value": { "type": "number", "description": "An item's quantity, in its unit." },
                "unit": { "type": "string", "description": "An item's unit, e.g. bags, t, nr." },
                "note": { "type": "string", "description": "A note for the user." },
                "empty_item": {
                    "type": "boolean",
                    "description": "true only when the user explicitly asked for an empty \
                                    item with no quantity yet; never for notes or analysis."
                }
            }),
            &["kind", "description"],
            true,
        ),
        tool(
            "set_height",
            "Turn an area into a volume of that area times height, or change a volume's height.",
            json!({
                "id": id(),
                "height": { "type": "number", "description": "Height in metres, above 0." }
            }),
            &["id", "height"],
            true,
        ),
        tool(
            "list_takeoff",
            "The take-off: every measurement with its id, kind, value (drawing units), shown in \
             unit (m, m2, m3, pcs, or an item's own unit), description, layer, sheet, handles, \
             source (user, ai or mcp), note and BOQ code; then totals per kind and unit.",
            json!({ "offset": offset() }),
            &[],
            false,
        ),
        tool(
            "remove_measurement",
            "Remove a measurement from the take-off. The user can undo it.",
            json!({ "id": id() }),
            &["id"],
            true,
        ),
        tool(
            "set_description",
            "Change a measurement's description. Say what was measured and where.",
            json!({
                "id": id(),
                "text": { "type": "string", "description": "The new description." }
            }),
            &["id", "text"],
            true,
        ),
        tool(
            "import_boq",
            "Import a bill of quantities from an Excel file beside the open drawing; it replaces \
             the BOQ imported before (the user can undo it). The path is relative to the \
             drawing's folder (the [state] block gives it); files outside that folder and its \
             subfolders can't be opened. Headers may be English or Arabic. Returns the number \
             of items, the sections, and how many item rows were left out because their \
             quantity is not a number; list_boq gives the items.",
            json!({ "path": {
                "type": "string",
                "description": "The .xlsx file, relative to the drawing's folder, e.g. boq.xlsx."
            } }),
            &["path"],
            true,
        ),
        tool(
            "list_boq",
            "The imported BOQ items: code, description, unit (m, m2, m3, pcs, kg, t, or as \
             written), quantity, rate, section, tolerance in percent (null: the user's setting) \
             and the row in the workbook. Paged: total counts every item; pass next_offset as \
             offset for the next page.",
            json!({ "offset": offset() }),
            &[],
            false,
        ),
        tool(
            "link_boq",
            "Link a measurement to a BOQ item, so its quantity counts towards that item. Before \
             comparing, link each measurement this way, or name it the way the BOQ does.",
            json!({
                "id": id(),
                "code": { "type": "string", "description": "The BOQ item code; empty unlinks it." }
            }),
            &["id", "code"],
            true,
        ),
        tool(
            "compare_boq",
            "Compare the take-off with the BOQ, the schedules and the block counts of every sheet, \
             and list the conflicts: quantity mismatches, work missing from the BOQ or from the \
             drawings, schedules that disagree with the plans, blocks counted differently on two \
             sheets of one floor, doubtful units. Conflicts found before keep their status and \
             note. When the result is cut, list_conflicts pages through the rest. Before \
             comparing, link each measurement to its BOQ item with link_boq, or name it the way \
             the BOQ does.",
            json!({
                "tolerance": {
                    "type": "number",
                    "description": "Allowed difference in percent for BOQ items with no tolerance of their own; 5 when left out."
                }
            }),
            &[],
            true,
        ),
        tool(
            "list_conflicts",
            "The conflicts from the last comparison, with their status and notes, and their \
             counts per kind and per status. Paged: total counts every conflict; pass \
             next_offset as offset for the next page.",
            json!({ "offset": offset() }),
            &[],
            false,
        ),
        tool(
            "resolve_conflict",
            "Set a conflict's status, with a note saying why.",
            json!({
                "id": { "type": "string", "description": "The conflict's id, from list_conflicts." },
                "status": { "type": "string", "enum": ["open", "resolved", "ignored"] },
                "note": { "type": "string", "description": "Why." }
            }),
            &["id", "status", "note"],
            true,
        ),
        tool(
            "add_markup",
            "Put a markup on the drawing for the user: a cloud around something (a ring of 3 \
             points or more) or a note at a point (1 point or more), with text. Markups are kept \
             with the take-off, not in the drawing file, and are not quantities; to draw a \
             comment into the drawing itself use add_comment.",
            json!({
                "points": points("Where, in drawing coordinates."),
                "text": { "type": "string", "description": "What the user should look at, and why." },
                "kind": { "type": "string", "enum": ["cloud", "note"] }
            }),
            &["points", "text", "kind"],
            true,
        ),
        tool(
            "add_dimension",
            "Draw a dimension into the drawing itself, as AutoCAD would: a real dimension entity \
             (lines, arrowheads and the measured text) saved in the DWG/DXF, on the current layer \
             in the current dimension style, sized by the annotation scale \
             (set_annotation_scale). The text shows the drawing's unit after the number \
             (\"12.00 m\", cm, mm, inches or feet) unless the user turned that off in the \
             dimension style. Without an annotation scale the text is the style's own size \
             (0.18 drawing units by default), small on a plan in metres: set the plan's scale \
             (e.g. 1:100) first. Points are [x, y] in drawing coordinates. Kinds and \
             what each needs: linear p1, p2, line_point (horizontal or vertical by where \
             line_point is; rotation in degrees for another direction); aligned p1, p2, offset \
             (the line that far to the left of p1->p2, negative right); angular vertex, p1, p2, \
             arc_point; radius, diameter and arc_length handle (a circle or arc from \
             find_entities) and point (where the text goes); ordinate point (the feature) and \
             leader_end; baseline and continue handle (an existing linear or aligned dimension) \
             and points (the next points); quick handles (lines, arcs, circles) and line_point. \
             One undo step. Then read_dimensions gives the values measured.",
            json!({
                "kind": {
                    "type": "string",
                    "enum": ["linear", "aligned", "angular", "radius", "diameter",
                             "arc_length", "ordinate", "baseline", "continue", "quick"]
                },
                "p1": point(),
                "p2": point(),
                "line_point": point_described("Where the dimension line passes."),
                "rotation": { "type": "number", "description": "linear: the direction in degrees." },
                "offset": { "type": "number", "description": "aligned: distance of the line from the points, drawing units." },
                "vertex": point(),
                "arc_point": point_described("angular: where the dimension arc passes."),
                "point": point_described("radius, diameter, arc_length: where the text goes; ordinate: the feature."),
                "leader_end": point_described("ordinate: the leader's end; mostly up or down gives X, sideways Y."),
                "handle": handle("The circle, arc or base dimension, from find_entities."),
                "handles": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "description": "quick: the entities, from find_entities."
                },
                "points": points("baseline, continue: the next points, in order.")
            }),
            &["kind"],
            true,
        ),
        tool(
            "set_annotation_scale",
            "Set the drawing's annotation scale, e.g. \"1:100\": new dimensions are drawn at \
             their dimension style's sizes times it. Use it when dimensions you drew come out \
             far too small or too large for the plan (look with view_image). One undo step.",
            json!({ "scale": { "type": "string", "description": "paper:drawing, e.g. \"1:50\" or \"1:100\"." } }),
            &["scale"],
            true,
        ),
        tool(
            "add_comment",
            "Draw a revision cloud with a written comment beside it into the drawing itself, on \
             the layer COMMENTS (red), saved in the DWG/DXF: for remarks to the designer or the \
             team. The cloud goes round the polygon points (3 or more, drawing coordinates; leave \
             a margin round what it marks). One undo step. Unlike add_markup, it is part of the \
             drawing, not the take-off.",
            json!({
                "points": points("The polygon the cloud goes round."),
                "text": { "type": "string", "description": "The comment, in the user's language." },
                "arc_length": { "type": "number", "description": "Length of each arc in drawing units; worked out from the outline when left out." }
            }),
            &["points", "text"],
            true,
        ),
        tool(
            "list_comments",
            "The comment clouds on every sheet: id, the cloud's handle, its label's handle, the \
             comment text, the sheet and the cloud's bounds.",
            json!({}),
            &[],
            false,
        ),
        tool(
            "measure_geometry",
            "The geometry of one entity as drawn, in metres and degrees: an arc's or circle's \
             radius, diameter, centre, angles and length (a circle's area); an ellipse's centre \
             and radii; a line's length, angle from the X axis and deltas; for a polyline, the \
             segment nearest at (its first when at is left out) and the whole length; a block \
             reference's block, insertion point, scale, rotation and attributes; a text's \
             content, height and style. Changes nothing.",
            json!({
                "handle": handle("The entity, from find_entities."),
                "chain": chain(),
                "at": point_described("For a polyline: a point near the segment wanted, in \
                                       drawing coordinates.")
            }),
            &["handle"],
            false,
        ),
        tool(
            "divide_along",
            "Divide an entity's path (line, arc, circle, polyline, ellipse, spline) into count \
             equal parts by true length: places point entities on layer QIYAS-POINT at the \
             divisions (count - 1 points on an open path, count on a closed one) and adds their \
             count to the take-off in pieces. The user can undo it.",
            json!({
                "handle": handle("The entity to divide, from find_entities."),
                "chain": chain(),
                "count": { "type": "integer", "description": "Equal parts, 2 to 1000." }
            }),
            &["handle", "count"],
            true,
        ),
        tool(
            "measure_along",
            "Place a point entity every step along an entity's path from its start, by true \
             length, on layer QIYAS-POINT (none at the very end), and add their count to the \
             take-off in pieces, e.g. posts every 2.5 m along a fence. At most 1000 points. The \
             user can undo it.",
            json!({
                "handle": handle("The entity, from find_entities."),
                "chain": chain(),
                "step": { "type": "number", "description": "Spacing in metres, above 0." }
            }),
            &["handle", "step"],
            true,
        ),
        tool(
            "id_point",
            "The coordinates of a point in metres (X and Y), as the user would read them.",
            json!({ "point": point_described("[x, y] in drawing coordinates.") }),
            &["point"],
            false,
        ),
        tool(
            "extract_data",
            "A table of entity data on a sheet: one row per entity the filters find (as \
             find_entities), one column per field, values as text (length and area in metres \
             and square metres, X and Y where it sits in metres; empty when it has none; block \
             references have no length). attributes adds one column per block attribute tag, \
             e.g. MARK for door marks. Paged: total counts every match; pass next_offset as \
             offset for the next page.",
            json!({
                "sheet": sheet("Sheet name or block name; the sheet shown when left out."),
                "layer": { "type": "string", "description": "Only entities on this layer." },
                "kind": { "type": "string", "description": "Only this entity type, e.g. Insert, Line, LwPolyline, Text." },
                "block_name": { "type": "string", "description": "Only references of this block." },
                "text_contains": {
                    "type": "string",
                    "description": "Only Text, MText or block references whose text or attribute values contain this."
                },
                "within": bbox("Only entities with at least one point inside this box."),
                "fields": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["handle", "kind", "layer", "block_name", "text", "length",
                                 "area", "x", "y", "sheet", "xref"]
                    },
                    "description": "The columns, in order."
                },
                "attributes": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Attribute tags to add as columns after fields, e.g. [\"MARK\"]."
                },
                "offset": offset(),
                "limit": { "type": "integer", "description": "Rows per page, at most 5000. Default 200." }
            }),
            &["fields"],
            false,
        ),
        tool(
            "zoom_to",
            "Zoom the user's view to entities or to a box on the sheet shown. Give handles or \
             bounds, not both.",
            json!({
                "handles": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "description": "Entities from find_entities, drawn on the sheet shown."
                },
                "bounds": bbox("The box to show.")
            }),
            &[],
            false,
        ),
        tool(
            "add_text",
            "Write text into the drawing itself (real TEXT, or MTEXT for several lines or a \
             wrapping width), saved in the DWG/DXF, one undo step for all the texts of the \
             call. Each text has a point (where it starts, [x, y] in drawing coordinates; for \
             multi-line text the top left corner) and text (any language; Arabic is kept in \
             reading order). height is the text's size in the drawing, in metres (2.5 mm printed \
             at 1:100 is 0.25 m); left out, it is 2.5 mm times the annotation scale \
             (set_annotation_scale), the size the Text tool uses. rotation in degrees turns single-line text. layer names the layer \
             (it is created, white, when the drawing has none); left out, the current layer. \
             Look with view_image afterwards and say what you wrote and where.",
            json!({
                "texts": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "text": { "type": "string", "description": "What to write; \\n starts a new line." },
                            "point": point_described("Where it starts: [x, y] in drawing coordinates."),
                            "height": { "type": "number", "description": "Text height in metres, above 0. Default 2.5 mm times the annotation scale." },
                            "rotation": { "type": "number", "description": "Degrees counter-clockwise; single-line text only." },
                            "width": { "type": "number", "description": "MTEXT wrapping width in metres; 0 or left out: no wrapping." }
                        },
                        "required": ["text", "point"],
                        "additionalProperties": false
                    },
                    "description": "The texts to write, at most 200."
                },
                "layer": { "type": "string", "description": "The layer to write on; the current layer when left out." },
                "style": { "type": "string", "description": "A text style of the drawing; the current one when left out." }
            }),
            &["texts"],
            true,
        ),
        tool(
            "room_areas",
            "Find the enclosed spaces (rooms) of the drawing and their areas in square metres, \
             without changing anything. Each space is the smallest closed region the drawn line \
             work makes round a seed point, the same finder the Hatch tool uses; its candidate area is \
             the region's area less the closed shapes inside it (columns, shafts, a fountain) \
             unless subtract_islands is false. Seeds are the room-name texts already in the \
             drawing (all of them, or those on text_layer, or those matching names) or points you \
             give in seeds. Returns per space: name, net_area_m2, enclosed_area_m2 (what the line \
             work encloses, columns that overlap the walls already cut out), the islands taken \
             off (area and bounds) and the point; and the texts that fell in no closed region. \
             Check the result against the drawing (view_image) before you write it anywhere; \
             label_spaces writes it into the drawing. These are unapproved geometric candidates, not certified net rooms. Text, layer, hatch or closed boundary alone does not establish room type or net basis. Exclude them from net-room accuracy.",
            json!({
                "seeds": seeds_schema(),
                "text_layer": { "type": "string", "description": "Take the room names from the Text and MText on this layer." },
                "names": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Only the texts equal to one of these (any case) are rooms."
                },
                "within": bbox("Only room names placed inside this box."),
                "subtract_islands": { "type": "boolean", "description": "Take the closed shapes inside a space off its area. Default true." }
            }),
            &[],
            false,
        ),
        tool(
            "label_spaces",
            "Do room_areas and write each unapproved candidate area into the drawing as text, like \"Unapproved candidate 58.29 m²\", \
             just under the room name (or at the seed point when it has no name), one undo step \
             for all the labels. Same inputs as room_areas, plus: layer (the labels' layer, \
             created if missing; the current layer when left out), height in metres (the room \
             name's own height by default), decimals (default 2). It adds texts; it does not \
             remove labels that are already there, so look with read_text first and ask the user \
             when some exist. Returns the same list, with the label's handle and point.",
            json!({
                "seeds": seeds_schema(),
                "text_layer": { "type": "string", "description": "Take the room names from the Text and MText on this layer." },
                "names": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Only the texts equal to one of these (any case) are rooms."
                },
                "within": bbox("Only room names placed inside this box."),
                "subtract_islands": { "type": "boolean", "description": "Take the closed shapes inside a space off its area. Default true." },
                "layer": { "type": "string", "description": "The labels' layer; the current layer when left out." },
                "height": { "type": "number", "description": "Label height in metres; the room name's own height when left out." },
                "decimals": { "type": "integer", "description": "Decimal places, 0 to 6. Default 2." },
                "language": { "type": "string", "enum": ["ar", "en"], "description": "Label language. Set from the user request, regardless of UI or seed language. When omitted, Arabic seed names use Arabic; other names use English. This never approves net area." }
            }),
            &[],
            true,
        ),
        tool(
            "add_table",
            "Draw a table (a schedule: rooms and areas, doors, windows, a BOQ extract) into the \
             drawing itself, as a real table entity, one undo step. point is its top left corner \
             in drawing coordinates; columns are the heading cells and rows the lines below, \
             every cell a string (numbers as text, already formatted). Sizes are metres: \
             text_height (default 2.5 mm times the annotation scale), row_height (twice the \
             text by default) and col_width (every column the same; wide enough for the longest \
             cell by default). layer as add_text. Put the schedule where it hides nothing \
             (find_entities / view_image first), and edit one later with edit_table_cell.",
            json!({
                "point": point_described("Top left corner: [x, y] in drawing coordinates."),
                "columns": { "type": "array", "items": { "type": "string" }, "description": "The heading cells." },
                "rows": {
                    "type": "array",
                    "items": { "type": "array", "items": { "type": "string" } },
                    "description": "The lines below the headings; each at most as long as columns."
                },
                "text_height": { "type": "number", "description": "Metres, above 0." },
                "row_height": { "type": "number", "description": "Metres, above 0." },
                "col_width": { "type": "number", "description": "Metres, above 0." },
                "layer": { "type": "string", "description": "The layer; the current layer when left out." }
            }),
            &["point", "columns", "rows"],
            true,
        ),
        tool(
            "edit_table_cell",
            "Change cells of a table in the drawing (from add_table, or a handle from \
             find_entities with kind Table); rows and columns count from 0, row 0 being the \
             headings. One undo step for all the cells of the call.",
            json!({
                "handle": handle("The table, from add_table or find_entities."),
                "cells": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "row": { "type": "integer" },
                            "col": { "type": "integer" },
                            "text": { "type": "string" }
                        },
                        "required": ["row", "col", "text"],
                        "additionalProperties": false
                    },
                    "description": "The cells to change."
                }
            }),
            &["handle", "cells"],
            true,
        ),
        tool(
            "draw",
            "Draw shapes into the drawing itself (real entities, saved in the DWG/DXF), one undo \
             step for all the shapes of the call: lines, polylines (bulges make arcs), \
             rectangles, circles, arcs, hatches and revision clouds. All numbers are in \
             drawing units (list_sheets gives them), unlike heights in the text tools. Use it \
             to build a legend or a key: a framed box (rectangle), a swatch or a sample line per \
             layer in that layer's colour, then add_text for the labels beside them and \
             insert_block for symbols. layer and color apply to every shape of the call (the \
             layer is created when missing; color is \"ByLayer\", \"ACI n\" or \"#rrggbb\"). \
             Place it where it hides nothing, look with view_image, and say what you drew.",
            json!({
                "shapes": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "kind": {
                                "type": "string",
                                "enum": ["line", "polyline", "rectangle", "circle", "arc", "hatch", "revcloud"]
                            },
                            "a": point_described("line: start; rectangle: one corner."),
                            "b": point_described("line: end; rectangle: the opposite corner."),
                            "points": points("polyline: the vertices; hatch: the ring to fill; revcloud: the polygon."),
                            "bulges": { "type": "array", "items": { "type": "number" }, "description": "polyline: per vertex the bulge (tan of a quarter of the arc's angle, + counter-clockwise) of the segment starting there." },
                            "closed": { "type": "boolean", "description": "polyline: join the last point to the first." },
                            "centre": point_described("circle, arc: the centre."),
                            "radius": { "type": "number", "description": "circle, arc: above 0." },
                            "start": { "type": "number", "description": "arc: start angle in degrees, counter-clockwise from +X." },
                            "end": { "type": "number", "description": "arc: end angle in degrees." },
                            "pattern": { "type": "string", "enum": ["solid", "ansi31", "ansi37"], "description": "hatch: the fill. Default solid." },
                            "arc_length": { "type": "number", "description": "revcloud: the length of each arc, above 0." }
                        },
                        "required": ["kind"],
                        "additionalProperties": false
                    },
                    "description": "The shapes to draw, at most 200."
                },
                "layer": { "type": "string", "description": "The layer; the current layer when left out." },
                "color": { "type": "string", "description": "\"ByLayer\", \"ACI n\" (1 to 255) or \"#rrggbb\"; the current colour when left out." }
            }),
            &["shapes"],
            true,
        ),
        tool(
            "insert_block",
            "Insert references of blocks the drawing already has (block_summary lists them) at \
             points, one undo step for all of them; fills the block's attributes by tag. Use it \
             for symbols in a legend or to place a door, window or column block. scale 1 and \
             rotation 0 by default; point in drawing coordinates. layer as add_text.",
            json!({
                "blocks": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "The block's name." },
                            "point": point_described("Insertion point: [x, y] in drawing coordinates."),
                            "scale": { "type": "number", "description": "Above 0. Default 1." },
                            "rotation": { "type": "number", "description": "Degrees counter-clockwise. Default 0." },
                            "attributes": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "tag": { "type": "string" },
                                        "value": { "type": "string" }
                                    },
                                    "required": ["tag", "value"],
                                    "additionalProperties": false
                                },
                                "description": "Attribute values by tag."
                            }
                        },
                        "required": ["name", "point"],
                        "additionalProperties": false
                    },
                    "description": "The references to insert, at most 200."
                },
                "layer": { "type": "string", "description": "The layer; the current layer when left out." }
            }),
            &["blocks"],
            true,
        ),
        tool(
            "modify",
            "Move, copy, rotate, scale, mirror or array drawn entities (handles from \
             find_entities: the drawing's own entities on the sheet, not inside blocks or xrefs). \
             One undo step; the take-off follows. move and copy: delta [dx, dy] (copy: count \
             copies, each delta further). rotate: base and angle (degrees, counter-clockwise). \
             scale: base and factor. mirror: the line p1-p2. copy true keeps the originals \
             (rotate, scale, mirror). array: rows, cols, dx, dy. Drawing units.",
            json!({
                "kind": { "type": "string", "enum": ["move", "copy", "rotate", "scale", "mirror", "array"] },
                "handles": { "type": "array", "items": { "type": "integer" }, "description": "The entities." },
                "delta": point_described("[dx, dy] for move and copy."),
                "count": { "type": "integer", "description": "Copies, 1 or more (copy)." },
                "base": point_described("The base point (rotate, scale)."),
                "angle": { "type": "number", "description": "Degrees, counter-clockwise (rotate)." },
                "factor": { "type": "number", "description": "Above 0 (scale)." },
                "copy": { "type": "boolean", "description": "Keep the originals (rotate, scale, mirror)." },
                "p1": point_described("The mirror line's first point."),
                "p2": point_described("The mirror line's second point."),
                "rows": { "type": "integer", "description": "Array rows, 1 or more." },
                "cols": { "type": "integer", "description": "Array columns, 1 or more." },
                "dx": { "type": "number", "description": "Array column spacing." },
                "dy": { "type": "number", "description": "Array row spacing." }
            }),
            &["kind", "handles"],
            true,
        ),
        tool(
            "edit",
            "Offset, trim, extend, fillet or chamfer one drawn entity (its handle from \
             find_entities). One undo step; the take-off follows. offset: distance and point on \
             the side to offset to. trim: point on the piece to cut away (edges: the cutters; \
             left out, everything drawn). extend: point near the end to extend (edges: the \
             boundaries). fillet: other line, point and point2 on the kept sides, radius (0 \
             meets them). chamfer: other line, point, point2, distance and distance2. Drawing units.",
            json!({
                "kind": { "type": "string", "enum": ["offset", "trim", "extend", "fillet", "chamfer"] },
                "handle": handle("The entity."),
                "other": handle("The second line (fillet, chamfer)."),
                "distance": { "type": "number", "description": "Offset distance, or the chamfer back along the first line." },
                "distance2": { "type": "number", "description": "The chamfer back along the second line." },
                "radius": { "type": "number", "description": "The fillet's radius." },
                "point": point_described("See the description, per kind."),
                "point2": point_described("A point on the second line's kept side (fillet, chamfer)."),
                "edges": { "type": "array", "items": { "type": "integer" }, "description": "Trim's cutters or extend's boundaries." }
            }),
            &["kind", "handle", "point"],
            true,
        ),
        tool(
            "erase",
            "Erase drawn entities (handles from find_entities: the drawing's own, on the sheet). \
             One undo step. Erase only what the user asked to erase.",
            json!({ "handles": { "type": "array", "items": { "type": "integer" }, "description": "The entities." } }),
            &["handles"],
            true,
        ),
        tool(
            "set_text",
            "Change a text or MText you or the user wrote, in place: its words, height (metres), \
             rotation (degrees) or layer. To move it, use modify move. Fix a label this way, \
             never by writing another one over it.",
            json!({
                "handle": handle("The text (find_entities kind Text or MText)."),
                "text": { "type": "string", "description": "The new words; \\n starts a new line in an MText." },
                "height": { "type": "number", "description": "Text height in metres." },
                "rotation": { "type": "number", "description": "Degrees, counter-clockwise." },
                "layer": { "type": "string", "description": "A layer the drawing has." }
            }),
            &["handle"],
            true,
        ),
        tool(
            "undo_last",
            "Undo your own newest step (a drawing, take-off or BOQ change you made). Refused when \
             the newest step is the user's: never undo their work.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "view_image",
            "A PNG picture of a sheet, drawn as the engineer sees it, so you can look at the \
             drawing: images and PDF underlays under the lines, model space in a layout's \
             viewports, lines, fills and text in their colours on white, the take-off in violet. \
             Look at the whole sheet first, then zoom in with bounds where you need detail: the \
             caption says how tall the text is in pixels (letters read from about 8 px) and how \
             big a pixel is. It never moves the user's view.",
            json!({
                "bounds": bbox("The part to show; the whole sheet when left out."),
                "width": { "type": "integer", "description": "Width in pixels, at most 1600. Default 800." },
                "sheet": sheet("A layout or sheet name; the one shown when left out."),
                "highlight": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "description": "Handles of entities on the sheet to draw thick violet (and mark first)."
                },
                "marks": {
                    "type": "boolean",
                    "description": "Put numbered badges on up to 40 entities in view; the caption \
                                    gives each number's handle, kind and layer. Use them to name \
                                    what you see and to act on it by handle."
                }
            }),
            &[],
            false,
        ),
        tool(
            "view_screen",
            "A picture of exactly what the user sees now in Qiyas's main view, as the window \
             draws it (their colours and background, images and PDF underlays, layout \
             viewports, the selection highlighted), with numbered marks on the entities: the \
             caption gives each mark's handle, and what the user selected is marked first. Use \
             it when the user refers to what they are looking at. It shows only the view on \
             screen: use view_image to look elsewhere or closer.",
            json!({
                "marks": { "type": "boolean", "description": "Numbered marks on the entities in view. Default true." }
            }),
            &[],
            false,
        ),
        tool(
            "get_selection",
            "What the user points at in the window now: the entities they selected (handle, \
             kind, layer, length and area each), a region they marked, and the view on screen. \
             Each message's [state] block gives it; call this when they may have changed it \
             since.",
            json!({}),
            &[],
            false,
        ),
        tool(
            "select_entities",
            "Selects entities in the window for the user, as if they had picked them with the \
             Select tool, so they can check them or run Move, Erase or Properties on them. \
             Entities at the top of the sheet on screen only. Changes nothing in the drawing and \
             is no undo step.",
            json!({
                "handles": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "description": "The entities to select; the selection is replaced."
                },
                "zoom": { "type": "boolean", "description": "Zoom the view to them. Default false." }
            }),
            &["handles"],
            false,
        ),
    ]
}

fn tool(
    name: &'static str,
    description: &'static str,
    properties: Value,
    required: &[&str],
    mutating: bool,
) -> ToolSpec {
    ToolSpec {
        name,
        description,
        input_schema: json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        }),
        mutating,
    }
}

fn sheet(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn handle(description: &str) -> Value {
    json!({ "type": "integer", "description": description })
}

/// The block references an entity sits inside.
fn chain() -> Value {
    json!({
        "type": "array",
        "items": { "type": "integer" },
        "description": "For an entity inside blocks or an xref: the handles of the block \
                        references it sits inside, outermost first (an xref row's chain from \
                        find_entities). Leave out for an entity on the sheet."
    })
}

fn id() -> Value {
    json!({ "type": "string", "description": "The measurement's id, from the take-off." })
}

fn offset() -> Value {
    json!({
        "type": "integer",
        "description": "Rows to skip; next_offset from the previous page. Default 0."
    })
}

/// An `[x, y]` point in drawing coordinates.
fn point() -> Value {
    json!({ "type": "array", "items": { "type": "number" }, "description": "[x, y]" })
}

fn point_described(description: &str) -> Value {
    json!({ "type": "array", "items": { "type": "number" }, "description": description })
}

fn points(description: &str) -> Value {
    json!({ "type": "array", "items": point(), "description": description })
}

/// The points room_areas and label_spaces look for spaces round.
fn seeds_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "The room's name, if it has one." },
                "point": point_described("A point inside the space: [x, y] in drawing coordinates.")
            },
            "required": ["point"],
            "additionalProperties": false
        },
        "description": "Points inside the spaces wanted, with their names; when given, the \
                        texts of the drawing are not used."
    })
}

fn bbox(description: &str) -> Value {
    json!({
        "type": "array",
        "items": point(),
        "description": format!("{description} [[min_x, min_y], [max_x, max_y]] in drawing coordinates.")
    })
}
