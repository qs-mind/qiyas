//! Tables drawn as themselves. A table (ACAD_TABLE) is a grid of rows and columns placed from its
//! insertion point (its top left corner): columns go along its horizontal direction, rows go down.
//! AutoCAD keeps what it draws in the table's anonymous `*T` block, in table coordinates; when a
//! table has no such block (or an empty one), `table_entities` makes the same picture from its
//! rows, columns and cells: the border and the lines between cells (none inside a merged cell)
//! and each cell's text as an MText aligned in its cell.

use opencadcodec::entities::{AttachmentPoint, EntityType, MText, Table, TableCell};
use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};
use opencadcodec::{BlockRecord, CadDocument, Color, Line, Vector3};

use crate::flatten::{is_logical_text, LOGICAL_TEXT_APP};
use crate::Xform;

/// Text height when a cell gives none, as a share of its row's height (Qiyas's own tables put
/// text at half the row): AutoCAD's fixed 0.18 is a hair in millimetres and a wall in metres.
const DEFAULT_TEXT_SHARE: f64 = 0.5;

/// The block record a table refers to: by its name when the drawing has a block of that name,
/// else by its record handle (a DWG reader sets only the handle).
pub fn table_block<'a>(doc: &'a CadDocument, t: &Table) -> Option<&'a BlockRecord> {
    let by_name = (!t.block_name.is_empty())
        .then(|| doc.block_records.get(&t.block_name))
        .flatten();
    by_name.or_else(|| {
        let h = t.block_record_handle?;
        doc.block_records.iter().find(|b| b.handle == h)
    })
}

/// Table coordinates (x along the columns from the insertion point, y up, so rows go to negative
/// y) to world coordinates on plan.
pub fn table_xform(t: &Table) -> Xform {
    let h = unit(&t.horizontal_direction).unwrap_or(Vector3::UNIT_X);
    let n = unit(&t.normal).unwrap_or(Vector3::UNIT_Z);
    // "Down" the rows is the direction crossed with the normal; up is its opposite.
    let up = unit(&n.cross(&h)).unwrap_or(Vector3::UNIT_Y);
    let p = t.insertion_point;
    Xform {
        m: [[h.x, up.x, p.x], [h.y, up.y, p.y], [0.0, 0.0, 1.0]],
    }
}

fn unit(v: &Vector3) -> Option<Vector3> {
    (v.length() > 1e-12).then(|| v.normalize())
}

/// The table's grid: the x of each column edge (from 0) and the y of each row edge (from 0,
/// going down), in table coordinates.
fn edges(t: &Table) -> (Vec<f64>, Vec<f64>) {
    let mut xs = vec![0.0];
    for c in &t.columns {
        xs.push(xs[xs.len() - 1] + c.width);
    }
    let mut ys = vec![0.0];
    for r in &t.rows {
        ys.push(ys[ys.len() - 1] - r.height);
    }
    (xs, ys)
}

/// Grows each row to hold its cells' text once wrapped, as AutoCAD's tables do: `measure` gives
/// the height an MText takes laid out (`None`: unknown, the row is left as it is). A cell merged
/// over several rows is left to them. Rows never shrink. True when a row grew.
pub fn fit_rows(t: &mut Table, measure: &mut dyn FnMut(&MText) -> Option<f64>) -> bool {
    let (rows, cols) = (t.rows.len(), t.columns.len());
    if rows == 0 || cols == 0 {
        return false;
    }
    let (xs, ys) = edges(t);
    let owner = merge_owners(t);
    let logical = is_logical_text(&t.common);
    let mut grown = Vec::new();
    for r in 0..rows {
        let mut need = t.rows[r].height;
        for c in 0..cols {
            if owner[r][c] != (r, c) || (r + 1 < rows && owner[r + 1][c] == (r, c)) {
                continue;
            }
            let Some(cell) = t.cell(r, c) else {
                continue;
            };
            let value = cell.text_value();
            if value.trim().is_empty() {
                continue;
            }
            let last_col = (c..cols)
                .take_while(|&j| owner[r][j] == (r, c))
                .last()
                .unwrap_or(c);
            let bounds = [xs[c], ys[r], xs[last_col + 1], ys[r + 1]];
            let m = cell_text(t, cell, value, bounds, logical);
            let style = cell.style.as_ref().or(t.base_style.as_ref());
            let (top, bottom) = style.map_or((m.height / 3.0, m.height / 3.0), |s| {
                (s.margin_top, s.margin_bottom)
            });
            if let Some(h) = measure(&m).filter(|h| h.is_finite() && *h > 0.0) {
                need = need.max(h + top + bottom);
            }
        }
        grown.push(need);
    }
    let mut changed = false;
    for (row, need) in t.rows.iter_mut().zip(grown) {
        if need > row.height + 1e-9 {
            row.height = need;
            changed = true;
        }
    }
    changed
}

/// For every cell, the first cell (row, column) of the merged cell it is part of (itself when it
/// is merged with none), from the table's merged ranges and each cell's merge size.
pub fn merge_owners(t: &Table) -> Vec<Vec<(usize, usize)>> {
    let (rows, cols) = (t.rows.len(), t.columns.len());
    let mut owner: Vec<Vec<(usize, usize)>> = (0..rows)
        .map(|r| (0..cols).map(|c| (r, c)).collect())
        .collect();
    if rows == 0 || cols == 0 {
        return owner;
    }
    let mut ranges: Vec<[usize; 4]> = t
        .merged_ranges
        .iter()
        .filter(|m| m.top_row <= m.bottom_row && m.left_col <= m.right_col)
        .map(|m| [m.top_row, m.left_col, m.bottom_row, m.right_col])
        .collect();
    for (r, row) in t.rows.iter().enumerate() {
        for (c, cell) in row.cells.iter().take(cols).enumerate() {
            let (w, h) = (
                cell.merge_width.max(1) as usize,
                cell.merge_height.max(1) as usize,
            );
            if w > 1 || h > 1 {
                ranges.push([r, c, r + h - 1, c + w - 1]);
            }
        }
    }
    for [top, left, bottom, right] in ranges {
        if top >= rows || left >= cols {
            continue;
        }
        for row in owner.iter_mut().take(bottom.min(rows - 1) + 1).skip(top) {
            for cell in row.iter_mut().take(right.min(cols - 1) + 1).skip(left) {
                *cell = (top, left);
            }
        }
    }
    owner
}

/// What a table without a block draws, in table coordinates (see `table_xform`): its grid lines
/// and its cells' texts, on layer "0" and ByBlock (so they take the table's), the texts marked
/// as Qiyas's own reading order when the table is.
pub fn table_entities(t: &Table) -> Vec<EntityType> {
    let (rows, cols) = (t.rows.len(), t.columns.len());
    if rows == 0 || cols == 0 {
        return Vec::new();
    }
    let (xs, ys) = edges(t);
    let owner = merge_owners(t);
    let mut out = Vec::new();
    let mut line = |a: [f64; 2], b: [f64; 2]| {
        let mut l = Line::from_points(Vector3::new(a[0], a[1], 0.0), Vector3::new(b[0], b[1], 0.0));
        l.common.layer = "0".to_string();
        l.common.color = Color::ByBlock;
        out.push(EntityType::Line(l));
    };
    // Lines between rows: wherever the cells above and below are not one merged cell, joined
    // into runs.
    for k in 0..=rows {
        let drawn = |c: usize| k == 0 || k == rows || owner[k - 1][c] != owner[k][c];
        let mut c = 0;
        while c < cols {
            if !drawn(c) {
                c += 1;
                continue;
            }
            let start = c;
            while c < cols && drawn(c) {
                c += 1;
            }
            line([xs[start], ys[k]], [xs[c], ys[k]]);
        }
    }
    for k in 0..=cols {
        let drawn = |r: usize| k == 0 || k == cols || owner[r][k - 1] != owner[r][k];
        let mut r = 0;
        while r < rows {
            if !drawn(r) {
                r += 1;
                continue;
            }
            let start = r;
            while r < rows && drawn(r) {
                r += 1;
            }
            line([xs[k], ys[start]], [xs[k], ys[r]]);
        }
    }
    let logical = is_logical_text(&t.common);
    for r in 0..rows {
        for c in 0..cols {
            if owner[r][c] != (r, c) {
                continue;
            }
            let Some(cell) = t.cell(r, c) else {
                continue;
            };
            let value = cell.text_value();
            if value.trim().is_empty() {
                continue;
            }
            // The merged cell reaches as far as the cells it owns.
            let last_row = (r..rows)
                .take_while(|&i| owner[i][c] == (r, c))
                .last()
                .unwrap_or(r);
            let last_col = (c..cols)
                .take_while(|&j| owner[r][j] == (r, c))
                .last()
                .unwrap_or(c);
            let bounds = [xs[c], ys[r], xs[last_col + 1], ys[last_row + 1]];
            out.push(EntityType::MText(cell_text(
                t, cell, value, bounds, logical,
            )));
        }
    }
    out
}

/// A cell's text as an MText in its cell `[x0, top, x1, bottom]`, placed by the cell's alignment
/// (middle centre when it gives none) inside its margins.
fn cell_text(t: &Table, cell: &TableCell, value: &str, bounds: [f64; 4], logical: bool) -> MText {
    let [x0, top, x1, bottom] = bounds;
    let content = cell.content();
    let style = cell.style.as_ref().or(t.base_style.as_ref());
    let height = style
        .map(|s| s.text_height)
        .filter(|h| *h > 0.0)
        .or(content.map(|c| c.text_height).filter(|h| *h > 0.0))
        .unwrap_or((top - bottom).abs() * DEFAULT_TEXT_SHARE);
    let alignment = content
        .map(|c| c.alignment)
        .filter(|a| (1..=9).contains(a))
        .or(style.map(|s| s.alignment).filter(|a| (1..=9).contains(a)))
        .unwrap_or(5);
    let [ml, mt, mr, mb] = style.map_or([height / 3.0; 4], |s| {
        [s.margin_left, s.margin_top, s.margin_right, s.margin_bottom]
    });
    let column = (alignment - 1) % 3;
    let row = (alignment - 1) / 3;
    let x = [x0 + ml, (x0 + x1) / 2.0, x1 - mr][column as usize];
    let y = [top - mt, (top + bottom) / 2.0, bottom + mb][row as usize];
    let mut m = MText::with_value(value, Vector3::new(x, y, 0.0)).with_height(height);
    m.rectangle_width = (x1 - x0 - ml - mr).max(0.0);
    m.attachment_point = attachment(alignment);
    m.common.layer = "0".to_string();
    m.common.color = match content.map(|c| c.color) {
        Some(Color::ByLayer) | None => Color::ByBlock,
        Some(color) => color,
    };
    let style_name = content
        .map(|c| c.text_style_name.as_str())
        .filter(|n| !n.is_empty())
        .or(style
            .map(|s| s.text_style_name.as_str())
            .filter(|n| !n.is_empty()));
    if let Some(name) = style_name {
        m.style = name.to_string();
    }
    if logical {
        let mut record = ExtendedDataRecord::new(LOGICAL_TEXT_APP);
        record.add_value(XDataValue::Integer16(1));
        m.common.extended_data.upsert_record(record);
    }
    m
}

fn attachment(alignment: i32) -> AttachmentPoint {
    match alignment {
        1 => AttachmentPoint::TopLeft,
        2 => AttachmentPoint::TopCenter,
        3 => AttachmentPoint::TopRight,
        4 => AttachmentPoint::MiddleLeft,
        6 => AttachmentPoint::MiddleRight,
        7 => AttachmentPoint::BottomLeft,
        8 => AttachmentPoint::BottomCenter,
        9 => AttachmentPoint::BottomRight,
        _ => AttachmentPoint::MiddleCenter,
    }
}

/// The cell (row, column) of the table under the world point `p` on plan; a merged cell is
/// named by its first cell. `None` outside the table.
pub fn table_cell_at(t: &Table, p: [f64; 2]) -> Option<(usize, usize)> {
    let xf = table_xform(t);
    let m = &xf.m;
    let (d0, d1) = (p[0] - m[0][2], p[1] - m[1][2]);
    // The table's axes are a unit pair at right angles: projecting on them inverts the placing.
    let x = d0 * m[0][0] + d1 * m[1][0];
    let y = d0 * m[0][1] + d1 * m[1][1];
    let (xs, ys) = edges(t);
    let c = (0..t.columns.len()).find(|&c| xs[c] <= x && x < xs[c + 1])?;
    let r = (0..t.rows.len()).find(|&r| ys[r] >= y && y > ys[r + 1])?;
    Some(merge_owners(t)[r][c])
}
