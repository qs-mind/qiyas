//! Tables (ACAD_TABLE) on the sheet on screen: `Command::Table` adds one on the current layer in
//! the current text style; `EditCell`, `InsertRow`, `InsertCol`, `DeleteRow`, `DeleteCol` and
//! `MergeCells` change one, one undo step each. Every change draws the table's anonymous `*T`
//! block again (`rebuild_table`, from `qiyas_geom::table_entities`), so another CAD program shows
//! what Qiyas shows. Cell text is plain text (stored escaped, as MText), in reading order (the
//! table is marked `QIYAS_TEXT`); field codes typed into a cell make it a field (see
//! `crate::field`).

use opencadcodec::entities::{CellRange, CellStyle, CellValue, CellValueType, Table, TableCell};
use opencadcodec::objects::ObjectType;
use opencadcodec::{BlockRecord, EntityType, Handle, MText, Vector3};
use qiyas_geom::{merge_owners, table_entities};
use serde::Serialize;

use crate::field::{cell_fields, has_fields, set_cell_fields};
use crate::query::plain_text;
use crate::text::{escape_mtext, mark_logical};
use crate::{CoreError, Session, Target};

/// Most cells a new table may have.
const MAX_CELLS: usize = 10_000;
/// A new table's text is this share of its row height.
const TEXT_IN_ROW: f64 = 0.5;

/// A table's cells for the table editor (`EntityInfo::table`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TableInfo {
    /// Each cell's text as read (formatting codes gone), row by row.
    pub cells: Vec<Vec<String>>,
    /// Column widths and row heights, in drawing units.
    pub widths: Vec<f64>,
    pub heights: Vec<f64>,
    /// Merged cells as `[top row, left column, bottom row, right column]`.
    pub merges: Vec<[usize; 4]>,
    /// The cells holding a field, as `[row, column]`.
    pub fields: Vec<[usize; 2]>,
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

/// The table's cells, widths, merges and fields.
pub(crate) fn table_info(t: &Table) -> TableInfo {
    let owners = merge_owners(t);
    let (rows, cols) = (t.rows.len(), t.columns.len());
    let mut merges = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            if owners[r][c] != (r, c) {
                continue;
            }
            let bottom = (r..rows).take_while(|&i| owners[i][c] == (r, c)).last();
            let right = (c..cols).take_while(|&j| owners[r][j] == (r, c)).last();
            let (bottom, right) = (bottom.unwrap_or(r), right.unwrap_or(c));
            if bottom > r || right > c {
                merges.push([r, c, bottom, right]);
            }
        }
    }
    TableInfo {
        cells: (0..rows)
            .map(|r| {
                (0..cols)
                    .map(|c| t.cell_text(r, c).map(plain_text).unwrap_or_default())
                    .collect()
            })
            .collect(),
        widths: t.columns.iter().map(|c| c.width).collect(),
        heights: t.rows.iter().map(|r| r.height).collect(),
        merges,
        fields: cell_fields(t).into_iter().map(|(r, c, _)| [r, c]).collect(),
    }
}

/// Sets the stored (escaped) text of cell (`r`, `c`), keeping its content's format; an empty
/// one is cleared.
pub(crate) fn set_cell_value(t: &mut Table, r: usize, c: usize, stored: &str) {
    let Some(cell) = t.cell_mut(r, c) else {
        return;
    };
    if stored.is_empty() {
        cell.contents.clear();
        return;
    }
    match cell.contents.first_mut() {
        Some(content) => {
            let mut value = CellValue::text(stored);
            value.unit_type = content.value.unit_type;
            content.value = value;
        }
        None => {
            let style = cell.style.clone();
            cell.set_text(stored);
            if let (Some(style), Some(content)) = (style, cell.contents.first_mut()) {
                content.text_height = style.text_height;
                content.text_style_name = style.text_style_name.clone();
                content.text_style_handle = style.text_style_handle;
            }
        }
    }
    cell.cell_type = opencadcodec::entities::CellType::Text;
}

/// A cell of a new table: middle centre, `height` text in `style`, margins a third of it.
fn new_cell_style(height: f64, style: &str, handle: Option<Handle>) -> CellStyle {
    let mut s = CellStyle::new();
    s.text_height = height;
    s.text_style_name = style.to_string();
    s.text_style_handle = handle;
    s.alignment = 5;
    s.set_margins(height / 3.0);
    s
}

fn finite(values: &[f64]) -> Result<(), CoreError> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(invalid("Points and sizes must be finite numbers"))
    }
}

impl Session {
    /// The conventional table sizes, shared with the input-default report.
    pub(crate) fn table_sizes(
        &self,
        col_width: f64,
        row_height: f64,
        data: &[Vec<String>],
    ) -> (f64, f64) {
        let row_height = if row_height == 0.0 {
            self.default_text_height() / TEXT_IN_ROW
        } else {
            row_height
        };
        let col_width = if col_width == 0.0 {
            let longest = data
                .iter()
                .flatten()
                .flat_map(|cell| cell.lines())
                .map(|line| line.chars().count())
                .max()
                .unwrap_or(0);
            longest as f64 * 0.7 * row_height * TEXT_IN_ROW + row_height
        } else {
            col_width
        };
        (col_width, row_height)
    }

    /// The table `target` at the top of the sheet on screen.
    fn table_target(&self, target: &Target) -> Result<(Handle, Table), CoreError> {
        let h = self.sheet_targets(std::slice::from_ref(target))?[0];
        match self.drawing.doc.get_entity(h) {
            Some(EntityType::Table(t)) => Ok((h, t.clone())),
            _ => Err(invalid("Pick a table")),
        }
    }

    /// The cell (row, column) of the table `handle` under `p` (drawing coordinates); a merged
    /// cell by its first cell. `None` outside it, or when `handle` is no table.
    pub fn table_cell_at(&self, handle: u64, p: [f64; 2]) -> Option<[usize; 2]> {
        match self.drawing.doc.get_entity(Handle::new(handle))? {
            EntityType::Table(t) => qiyas_geom::table_cell_at(t, p).map(|(r, c)| [r, c]),
            _ => None,
        }
    }

    /// Writes `plain` (plain text, field codes worked out) into cell (`r`, `c`) of `t`, which sits
    /// on the sheet `sheet`, keeping or dropping the cell's field.
    fn put_cell_text(&mut self, t: &mut Table, r: usize, c: usize, plain: &str, sheet: Handle) {
        let mut fields = cell_fields(t);
        fields.retain(|(fr, fc, _)| (*fr, *fc) != (r, c));
        let shown = if has_fields(plain) {
            self.register_field_app();
            fields.push((r, c, plain.to_string()));
            self.evaluate_fields(plain, sheet)
        } else {
            plain.to_string()
        };
        set_cell_value(t, r, c, &escape_mtext(&shown));
        set_cell_fields(t, &fields);
    }

    /// `Command::Table`: a table of `rows` x `cols` cells with its top left corner at `at`,
    /// columns `col_width` wide and rows `row_height` tall, its cells filled from `data` (row by
    /// row, plain text), on the current layer with the current text style.
    pub(crate) fn add_table(
        &mut self,
        at: [f64; 2],
        rows: usize,
        cols: usize,
        col_width: f64,
        row_height: f64,
        data: &[Vec<String>],
    ) -> Result<u64, CoreError> {
        finite(&[at[0], at[1], col_width, row_height])?;
        if rows == 0 || cols == 0 {
            return Err(invalid("A table needs one row and one column at least"));
        }
        if rows.saturating_mul(cols) > MAX_CELLS {
            return Err(invalid(format!("A table takes {MAX_CELLS} cells at most")));
        }
        if col_width < 0.0 || row_height < 0.0 {
            return Err(invalid("Column widths and row heights must be 0 or more"));
        }
        if data.len() > rows || data.iter().any(|r| r.len() > cols) {
            return Err(invalid("There is more text than cells"));
        }
        let (col_width, row_height) = self.table_sizes(col_width, row_height, data);
        let style = self.text_style();
        let doc = &self.drawing.doc;
        let style_handle = doc.text_styles.get(&style).map(|s| s.handle);
        let table_style = doc
            .objects
            .values()
            .filter_map(|o| match o {
                ObjectType::TableStyle(s) => Some(s),
                _ => None,
            })
            .min_by_key(|s| !s.name.eq_ignore_ascii_case("Standard"))
            .map(|s| s.handle);
        let height = row_height * TEXT_IN_ROW;
        let mut t = Table::new(Vector3::new(at[0], at[1], 0.0), rows, cols);
        t.table_style_handle = table_style;
        t.set_uniform_column_width(col_width);
        t.set_uniform_row_height(row_height);
        for row in &mut t.rows {
            for cell in &mut row.cells {
                cell.style = Some(new_cell_style(height, &style, style_handle));
            }
        }
        let sheet = self.sheet_owner().unwrap_or(Handle::NULL);
        for (r, line) in data.iter().enumerate() {
            for (c, text) in line.iter().enumerate() {
                self.put_cell_text(&mut t, r, c, text, sheet);
            }
        }
        self.register_text_app();
        mark_logical(&mut t.common);
        let h = self.add_styled(vec![EntityType::Table(t)])?[0];
        self.rebuild_table(Handle::new(h))?;
        Ok(h)
    }

    /// Draws the `*T` block of the table `h` again from its cells (a new one when it has none).
    pub(crate) fn rebuild_table(&mut self, h: Handle) -> Result<(), CoreError> {
        let Some(EntityType::Table(t)) = self.drawing.doc.get_entity(h) else {
            return Err(invalid("This is no table"));
        };
        let mut t = t.clone();
        // Rows grow to hold their wrapped text (AutoCAD's tables do).
        qiyas_geom::fit_rows(&mut t, &mut |m| self.mtext_height(m));
        let parts = table_entities(&t);
        let owner = self.table_block(&mut t)?;
        for mut e in parts {
            e.common_mut().owner_handle = owner;
            self.drawing
                .doc
                .add_entity(e)
                .map_err(|e| invalid(e.to_string()))?;
        }
        t.common.graphic_data = None;
        *self.drawing.doc.get_entity_mut(h).expect("found above") = EntityType::Table(t);
        Ok(())
    }

    /// The height an MText takes laid out (wrapped at its width) by the screen's text engine.
    fn mtext_height(&self, m: &MText) -> Option<f64> {
        let entity = EntityType::MText(m.clone());
        let flat = qiyas_geom::flatten(
            &self.drawing.doc,
            &entity,
            &qiyas_geom::Xform::identity(),
            self.batch_tol(),
        );
        let text = flat.paths.into_iter().find_map(|p| p.text)?;
        let (_, _, corners) =
            self.text
                .borrow_mut()
                .lay_out(&self.drawing.doc, &text, self.arabic_reversed)?;
        let ys = corners.map(|c| c[1]);
        let (lo, hi) = ys
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), y| (a.min(*y), b.max(*y)));
        Some(hi - lo)
    }

    /// The block record `t`'s picture goes in, emptied: its own `*T` block, or a new one under
    /// the first free `*Tn` name (written into `t`) when it has none or shares it with another
    /// table (as a copy of a DWG table made before copies resolved its block left them).
    fn table_block(&mut self, t: &mut Table) -> Result<Handle, CoreError> {
        let doc = &self.drawing.doc;
        let own = qiyas_geom::table_block(doc, t).map(|b| (b.name.clone(), b.handle));
        let shared = own.as_ref().is_some_and(|(_, record)| {
            doc.entities().any(|e| match e {
                EntityType::Table(other) => {
                    other.common.handle != t.common.handle
                        && qiyas_geom::table_block(doc, other).is_some_and(|b| b.handle == *record)
                }
                _ => false,
            })
        });
        let doc = &mut self.drawing.doc;
        if let Some((name, _)) = own.filter(|_| !shared) {
            let record = doc.block_records.get(&name).expect("found above");
            let (handle, inside) = (record.handle, record.entity_handles.clone());
            for e in inside {
                doc.remove_entity_arc(e);
            }
            if let Some(record) = doc.block_records.get_mut(&name) {
                record.entity_handles.clear();
            }
            t.block_name = name;
            t.block_record_handle = Some(handle);
            return Ok(handle);
        }
        let fresh = (1..)
            .map(|n| format!("*T{n}"))
            .find(|n| doc.block_records.get(n).is_none())
            .expect("a free name");
        let mut record = BlockRecord::new(fresh.clone());
        record.handle = doc.allocate_handle();
        record.block_entity_handle = doc.allocate_handle();
        record.block_end_handle = doc.allocate_handle();
        record.flags.anonymous = true;
        let handle = record.handle;
        doc.block_records
            .add(record)
            .map_err(|e| invalid(e.to_string()))?;
        t.block_name = fresh;
        t.block_record_handle = Some(handle);
        Ok(handle)
    }

    /// Puts the changed table `t` back as `h` and draws its block again.
    fn store_table(&mut self, h: Handle, t: Table) -> Result<(), CoreError> {
        *self.drawing.doc.get_entity_mut(h).expect("a sheet target") = EntityType::Table(t);
        self.rebuild_table(h)
    }

    /// `Command::EditCell`: cell (`row`, `col`) of the table `target` holds `text` (plain text;
    /// a cell inside a merged cell edits the merged cell's first).
    pub(crate) fn edit_cell(
        &mut self,
        target: &Target,
        row: usize,
        col: usize,
        text: &str,
    ) -> Result<(), CoreError> {
        let (h, mut t) = self.table_target(target)?;
        if row >= t.rows.len() || col >= t.columns.len() {
            return Err(invalid(format!(
                "The table has no cell {},{}",
                row + 1,
                col + 1
            )));
        }
        let (r, c) = merge_owners(&t)[row][col];
        let sheet = t.common.owner_handle;
        self.put_cell_text(&mut t, r, c, text, sheet);
        self.register_text_app();
        mark_logical(&mut t.common);
        self.store_table(h, t)
    }

    /// `Command::InsertRow`: a new row before row `at` (`at` = the row count: after the last), as
    /// tall as its neighbour and styled as it.
    pub(crate) fn insert_row(&mut self, target: &Target, at: usize) -> Result<(), CoreError> {
        let (h, mut t) = self.table_target(target)?;
        if at > t.rows.len() {
            return Err(invalid("There is no such row"));
        }
        let near = at.saturating_sub(1);
        let model = t.rows[near].clone();
        t.insert_row(at);
        t.rows[at].height = model.height;
        for (cell, from) in t.rows[at].cells.iter_mut().zip(&model.cells) {
            *cell = blank_like(from);
        }
        t.rows[at].style = model.style;
        let fields: Vec<_> = cell_fields(&t)
            .into_iter()
            .map(|(r, c, f)| (if r >= at { r + 1 } else { r }, c, f))
            .collect();
        set_cell_fields(&mut t, &fields);
        self.store_table(h, t)
    }

    /// `Command::InsertCol`: a new column before column `at`, as wide as its neighbour.
    pub(crate) fn insert_col(&mut self, target: &Target, at: usize) -> Result<(), CoreError> {
        let (h, mut t) = self.table_target(target)?;
        if at > t.columns.len() {
            return Err(invalid("There is no such column"));
        }
        let near = at.saturating_sub(1);
        let width = t.columns[near].width;
        let models: Vec<Option<TableCell>> =
            t.rows.iter().map(|r| r.cells.get(near).cloned()).collect();
        t.insert_column(at, width);
        for (row, model) in t.rows.iter_mut().zip(models) {
            if let (Some(cell), Some(model)) = (row.cells.get_mut(at), model) {
                *cell = blank_like(&model);
            }
        }
        let fields: Vec<_> = cell_fields(&t)
            .into_iter()
            .map(|(r, c, f)| (r, if c >= at { c + 1 } else { c }, f))
            .collect();
        set_cell_fields(&mut t, &fields);
        self.store_table(h, t)
    }

    /// `Command::DeleteRow`: removes row `row`; a table keeps one row at least.
    pub(crate) fn delete_row(&mut self, target: &Target, row: usize) -> Result<(), CoreError> {
        let (h, mut t) = self.table_target(target)?;
        if row >= t.rows.len() {
            return Err(invalid("There is no such row"));
        }
        if t.rows.len() == 1 {
            return Err(invalid("A table keeps one row at least"));
        }
        t.remove_row(row);
        let fields: Vec<_> = cell_fields(&t)
            .into_iter()
            .filter(|(r, _, _)| *r != row)
            .map(|(r, c, f)| (if r > row { r - 1 } else { r }, c, f))
            .collect();
        set_cell_fields(&mut t, &fields);
        self.store_table(h, t)
    }

    /// `Command::DeleteCol`: removes column `col`; a table keeps one column at least.
    pub(crate) fn delete_col(&mut self, target: &Target, col: usize) -> Result<(), CoreError> {
        let (h, mut t) = self.table_target(target)?;
        if col >= t.columns.len() {
            return Err(invalid("There is no such column"));
        }
        if t.columns.len() == 1 {
            return Err(invalid("A table keeps one column at least"));
        }
        t.remove_column(col);
        let fields: Vec<_> = cell_fields(&t)
            .into_iter()
            .filter(|(_, c, _)| *c != col)
            .map(|(r, c, f)| (r, if c > col { c - 1 } else { c }, f))
            .collect();
        set_cell_fields(&mut t, &fields);
        self.store_table(h, t)
    }

    /// `Command::MergeCells`: the cells from `first` to `last` (`[row, column]`, either corner)
    /// become one, keeping the first cell's text; one cell given unmerges the merged cell it is
    /// part of.
    pub(crate) fn merge_cells(
        &mut self,
        target: &Target,
        first: [usize; 2],
        last: [usize; 2],
    ) -> Result<(), CoreError> {
        let (h, mut t) = self.table_target(target)?;
        let (rows, cols) = (t.rows.len(), t.columns.len());
        if first[0].max(last[0]) >= rows || first[1].max(last[1]) >= cols {
            return Err(invalid("The table has no such cells"));
        }
        let range = CellRange::new(
            first[0].min(last[0]),
            first[1].min(last[1]),
            first[0].max(last[0]),
            first[1].max(last[1]),
        );
        if range.cell_count() == 1 {
            let (r, c) = merge_owners(&t)[range.top_row][range.left_col];
            t.unmerge_cell(r, c);
        } else {
            t.merge_cells(range);
        }
        self.store_table(h, t)
    }
}

/// An empty cell styled as `model`.
fn blank_like(model: &TableCell) -> TableCell {
    let mut cell = TableCell::new();
    cell.style = model.style.clone();
    cell
}

/// True when the cell's first content is text find and replace may change.
pub(crate) fn is_text_value(v: &CellValue) -> bool {
    matches!(v.value_type, CellValueType::String | CellValueType::Unknown)
}
