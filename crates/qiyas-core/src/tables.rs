//! Schedules found on a sheet: real table entities, and grids of loose text laid out as a table.

use opencadcodec::entities::Table;
use opencadcodec::{EntityType, Vector3};
use qiyas_geom::Xform;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::query::plain_text;
use crate::Session;

/// A table read off a sheet: a header row and data rows of plain text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Schedule {
    pub name: String,
    /// Block of the sheet it was found on.
    pub sheet: String,
    /// Handle of the table entity, or 0 for a grid of loose text.
    pub source: u64,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Rows share a line when their y differs by less than this many text heights.
const ROW_TOL: f64 = 0.6;
/// A gap wider than this many text heights between x positions starts a new column.
const COLUMN_GAP: f64 = 2.0;
/// The title is the nearest text above the header within this many text heights.
const TITLE_REACH: f64 = 3.0;
/// Smallest grid kept, header included.
const MIN_ROWS: usize = 3;
const MIN_COLUMNS: usize = 2;

impl Session {
    /// The table entities on a sheet, then the grids of loose Text and MText on it; the tables
    /// and loose text at the top of the sheet's xrefs count as on the sheet. Text inside blocks is
    /// not used for grids.
    pub fn read_tables(&self, sheet: Option<&str>) -> Vec<Schedule> {
        let sheet = self.sheet_block(sheet);
        let mut schedules = Vec::new();
        let mut texts = Vec::new();
        let xrefs = self.sheet_xrefs(&sheet);
        let placed = self
            .drawing
            .entities_in(&sheet)
            .map(|e| (e, Xform::identity()))
            .chain(
                xrefs
                    .iter()
                    .flat_map(|x| x.doc.entities_in_block("*Model_Space").map(|e| (e, x.xf))),
            );
        for (e, xf) in placed {
            let cell = |at: &Vector3, height: f64, text: String| {
                let [x, y] = xf.apply([at.x, at.y]);
                let [x0, y0] = xf.apply([0.0, 0.0]);
                let [x1, y1] = xf.apply([0.0, height]);
                Cell {
                    x,
                    y,
                    height: (x1 - x0).hypot(y1 - y0),
                    text,
                }
            };
            match e {
                EntityType::Table(t) => schedules.push(table_schedule(t, &sheet)),
                EntityType::Text(t) => {
                    texts.push(cell(&t.insertion_point, t.height, t.value.clone()))
                }
                EntityType::MText(t) => {
                    texts.push(cell(&t.insertion_point, t.height, plain_text(&t.value)))
                }
                _ => {}
            }
        }
        schedules.extend(text_grids(texts, &sheet));
        schedules
    }
}

/// A table entity as a schedule. When the first row is one cell across the whole table it is
/// the title and the next row is the header; otherwise the first row is the header.
fn table_schedule(t: &Table, sheet: &str) -> Schedule {
    let columns = t
        .rows
        .iter()
        .map(|r| r.cells.len())
        .chain([t.columns.len()])
        .max()
        .unwrap_or(0);
    let row_texts = |r: usize| -> Vec<String> {
        (0..columns)
            .map(|c| {
                t.cell(r, c)
                    .map_or(String::new(), |c| plain_text(c.text_value()))
            })
            .collect()
    };
    let title = (columns > 1)
        .then(|| t.cell(0, 0))
        .flatten()
        .filter(|cell| {
            cell.merge_width as usize >= columns
                || t.merged_ranges
                    .iter()
                    .any(|m| m.top_row == 0 && m.left_col == 0 && m.right_col + 1 >= columns)
        })
        .map(|cell| plain_text(cell.text_value()));
    let header = usize::from(title.is_some());
    let name = title.unwrap_or_else(|| {
        format!(
            "Table at {:.1},{:.1}",
            t.insertion_point.x, t.insertion_point.y
        )
    });
    Schedule {
        name,
        sheet: sheet.to_string(),
        source: t.common.handle.value(),
        columns: if header < t.rows.len() {
            row_texts(header)
        } else {
            Vec::new()
        },
        rows: (header + 1..t.rows.len()).map(row_texts).collect(),
    }
}

/// One loose text placed on the sheet.
struct Cell {
    x: f64,
    y: f64,
    height: f64,
    text: String,
}

/// Grids among loose texts: lines of texts sharing a y, runs of consecutive lines with two or
/// more texts, columns from the x positions across a run.
fn text_grids(mut texts: Vec<Cell>, sheet: &str) -> Vec<Schedule> {
    let mut heights: Vec<f64> = texts
        .iter()
        .map(|t| t.height)
        .filter(|h| *h > 0.0)
        .collect();
    if heights.is_empty() {
        return Vec::new();
    }
    heights.sort_by(f64::total_cmp);
    let h = heights[heights.len() / 2];

    // Lines, top to bottom, each sorted left to right.
    texts.sort_by(|a, b| b.y.total_cmp(&a.y));
    let mut lines: Vec<Vec<Cell>> = Vec::new();
    for t in texts {
        match lines.last_mut() {
            Some(line) if (line[0].y - t.y).abs() < ROW_TOL * h => line.push(t),
            _ => lines.push(vec![t]),
        }
    }
    for line in &mut lines {
        line.sort_by(|a, b| a.x.total_cmp(&b.x));
    }

    let mut grids = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let mut end = start;
        while end < lines.len() && lines[end].len() >= MIN_COLUMNS {
            end += 1;
        }
        if end - start >= MIN_ROWS {
            if let Some(grid) = grid_schedule(&lines[start..end], &lines[..start], h, sheet) {
                grids.push(grid);
            }
        }
        start = end.max(start + 1);
    }
    grids
}

/// The run of `lines` as a schedule, or `None` with fewer than two columns. `above` holds the
/// lines above the run, where the title is looked for.
fn grid_schedule(
    lines: &[Vec<Cell>],
    above: &[Vec<Cell>],
    h: f64,
    sheet: &str,
) -> Option<Schedule> {
    // Column bands: x positions across the run, split where the gap is wide.
    let mut xs: Vec<f64> = lines.iter().flatten().map(|t| t.x).collect();
    xs.sort_by(f64::total_cmp);
    let mut starts = vec![xs[0]];
    for pair in xs.windows(2) {
        if pair[1] - pair[0] > COLUMN_GAP * h {
            starts.push(pair[1]);
        }
    }
    if starts.len() < MIN_COLUMNS {
        return None;
    }
    let column = |x: f64| starts.iter().rposition(|s| *s <= x).unwrap_or(0);
    let row = |line: &Vec<Cell>| -> Vec<String> {
        let mut cells = vec![String::new(); starts.len()];
        for t in line {
            let cell = &mut cells[column(t.x)];
            if !cell.is_empty() {
                cell.push(' ');
            }
            cell.push_str(&t.text);
        }
        cells
    };

    let header = &lines[0];
    let (left, right) = (xs[0] - COLUMN_GAP * h, xs[xs.len() - 1] + COLUMN_GAP * h);
    let top = header[0].y;
    let title = above
        .iter()
        .flatten()
        .filter(|t| t.y > top && t.y - top <= TITLE_REACH * h && (left..=right).contains(&t.x))
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .map(|t| t.text.clone());
    Some(Schedule {
        name: title.unwrap_or_else(|| format!("Grid at {:.1},{:.1}", header[0].x, top)),
        sheet: sheet.to_string(),
        source: 0,
        columns: row(header),
        rows: lines[1..].iter().map(row).collect(),
    })
}
