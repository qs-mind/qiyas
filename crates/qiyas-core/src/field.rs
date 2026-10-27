//! Fields: text that Qiyas works out (`Command::Field`, or field codes typed into a text, an
//! MText or a table cell) and works out again (`Command::UpdateFields`).
//!
//! A field is written as AutoCAD writes its field codes: `%<\AcVar Date \f "yyyy-MM-dd">%`, the
//! file name `%<\AcVar Filename>%`, an entity's area or length `%<\AcObjProp
//! Object(%<\_ObjId 2A>%).Area \f "%lu2%pr3">%` (the entity's handle in hex) and, Qiyas's own, a
//! count of block references on the sheet `%<\AcVar QiyasCount "DOOR">%`. The text keeps the
//! value as its visible text (what any CAD program shows); the code (the whole text as typed, field
//! codes and the words around them) is kept in `FIELD_APP` xdata: on a text or an MText as its
//! strings; on a table as (row, column, strings) for each cell holding one.
//!
//! Area and length fields follow the entity they read: after every modify, edit and erase they
//! are worked out again in the same undo step (`Session::follow_fields`). Dates, file names and
//! counts change on `UpdateFields`. Areas and lengths are in square metres and metres (as the
//! take-off shows them), 2 decimals and, in a text that does not give the unit itself, with it
//! (`17.81 m²`; the take-off keeps full precision); an entity gone, or a code Qiyas does not know, shows `####`.

pub(crate) use crate::xdata::chunks;
use opencadcodec::entities::Table;
use opencadcodec::tables::normalize_name;
use opencadcodec::xdata::XDataValue;
use opencadcodec::{AppId, EntityType, Handle};
use qiyas_geom::Xform;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::session::effective_block_name;
use crate::takeoff::in_metres;
use crate::text::escape_mtext;
use crate::{CoreError, Kind, Session, Target};

/// The xdata application holding a field's code (never `QIYAS`: `embed_measurements` deletes
/// those).
pub const FIELD_APP: &str = "QIYAS_FIELD";
/// What a field shows when it can't be worked out, as AutoCAD does.
const UNKNOWN: &str = "####";

/// A field to insert (`Command::Field`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldKind {
    /// Today's date, `yyyy-MM-dd` (UTC).
    Date,
    /// The drawing's file name.
    FileName,
    /// The area of the closed entity `target` (at the top of the sheet), in square metres.
    Area { target: Target },
    /// The length of the entity `target`, in metres.
    Length { target: Target },
    /// How many references of the block `block` the sheet holds.
    Count { block: String },
}

/// The field code of `kind`, as it goes into a text.
pub fn field_code(kind: &FieldKind) -> String {
    match kind {
        FieldKind::Date => r#"%<\AcVar Date \f "yyyy-MM-dd">%"#.to_string(),
        FieldKind::FileName => r#"%<\AcVar Filename>%"#.to_string(),
        FieldKind::Area { target } => object_code(target.handle, "Area"),
        FieldKind::Length { target } => object_code(target.handle, "Length"),
        FieldKind::Count { block } => {
            format!(r#"%<\AcVar QiyasCount "{}">%"#, block.replace('"', ""))
        }
    }
}

fn object_code(handle: u64, property: &str) -> String {
    format!(r#"%<\AcObjProp Object(%<\_ObjId {handle:X}>%).{property} \f "%lu2%pr3">%"#)
}

/// True when `text` holds a field code.
pub(crate) fn has_fields(text: &str) -> bool {
    text.contains("%<\\")
}

/// True when `text` holds a field reading an entity (area, length): one to follow edits.
fn has_object_fields(text: &str) -> bool {
    text.contains("%<\\AcObjProp")
        || text.contains("%<\\AcVar QiyasOpeningCount")
        || text.contains("%<\\AcVar QiyasArea")
        || text.contains("%<\\AcVar QiyasLength")
        || text.contains("%<\\AcVar QiyasRoomArea")
        || text.contains("%<\\AcVar QiyasObject")
}

/// What one field code reads.
enum Code {
    Date,
    FileName,
    Area(u64),
    Length(u64),
    Sum {
        handles: Vec<u64>,
        area: bool,
    },
    RoomArea {
        handles: Vec<u64>,
        decimals: usize,
    },
    Count(String),
    OpeningCount(u64, String),
    /// Object ids, quantity name, decimals, and whether the sheet is in Arabic (digits too).
    ObjectValue(Vec<u64>, String, usize, bool),
    Unknown,
}

/// The outermost field codes in `text` (`%<` to its matching `>%`, codes nested inside counted),
/// as byte ranges.
fn code_spans(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let (mut depth, mut start, mut i) = (0usize, 0usize, 0usize);
    while i + 1 < bytes.len() {
        match (bytes[i], bytes[i + 1]) {
            (b'%', b'<') => {
                if depth == 0 {
                    start = i;
                }
                depth += 1;
                i += 2;
            }
            (b'>', b'%') if depth > 0 => {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    spans.push((start, i));
                }
            }
            _ => i += 1,
        }
    }
    spans
}

fn parse_code(code: &str) -> Code {
    let inner = code
        .strip_prefix("%<")
        .and_then(|c| c.strip_suffix(">%"))
        .unwrap_or(code)
        .trim();
    if let Some(rest) = inner.strip_prefix("\\AcVar") {
        let rest = rest.trim_start();
        let word: String = rest.chars().take_while(|c| c.is_alphanumeric()).collect();
        return match word.to_ascii_lowercase().as_str() {
            "qiyasroomarea" => {
                let mut parts = rest.split('"');
                let _ = parts.next();
                let handles = parts.next().and_then(|ids| {
                    ids.split(',')
                        .map(|h| u64::from_str_radix(h, 16).ok())
                        .collect::<Option<Vec<_>>>()
                });
                let decimals = parts
                    .next()
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .unwrap_or(2)
                    .min(6);
                handles.map_or(Code::Unknown, |handles| Code::RoomArea {
                    handles,
                    decimals,
                })
            }
            "date" => Code::Date,
            "filename" => Code::FileName,
            "qiyasarea" | "qiyaslength" => rest
                .split('"')
                .nth(1)
                .and_then(|list| {
                    list.split(',')
                        .map(|h| u64::from_str_radix(h, 16).ok())
                        .collect::<Option<Vec<_>>>()
                })
                .map_or(Code::Unknown, |handles| Code::Sum {
                    handles,
                    area: word.eq_ignore_ascii_case("qiyasarea"),
                }),
            "qiyascount" => rest
                .split('"')
                .nth(1)
                .map_or(Code::Unknown, |name| Code::Count(name.to_string())),
            "qiyasobject" => {
                let parts: Vec<_> = rest.split('"').collect();
                match (parts.get(1), parts.get(3)) {
                    (Some(ids), Some(name)) => ids
                        .split(',')
                        .filter(|id| !id.is_empty())
                        .map(|id| u64::from_str_radix(id, 16).ok())
                        .collect::<Option<Vec<_>>>()
                        .map_or(Code::Unknown, |ids| {
                            let decimals = parts
                                .get(4)
                                .and_then(|s| s.split_whitespace().next()?.parse::<usize>().ok())
                                .unwrap_or_else(|| crate::numbers::default_decimals(name))
                                .min(6);
                            Code::ObjectValue(
                                ids,
                                (*name).into(),
                                decimals,
                                parts
                                    .get(4)
                                    .is_some_and(|s| s.split_whitespace().nth(1) == Some("ar")),
                            )
                        }),
                    _ => Code::Unknown,
                }
            }
            "qiyasopeningcount" => {
                let mut quoted = rest.split('"');
                let source = quoted.nth(1).and_then(|s| u64::from_str_radix(s, 16).ok());
                let mark = quoted.nth(1);
                match (source, mark) {
                    (Some(source), Some(mark)) => Code::OpeningCount(source, mark.to_string()),
                    _ => Code::Unknown,
                }
            }
            _ => Code::Unknown,
        };
    }
    if inner.starts_with("\\AcObjProp") {
        let Some(id) = inner.split("\\_ObjId").nth(1) else {
            return Code::Unknown;
        };
        let hex: String = id
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        let Ok(handle) = u64::from_str_radix(&hex, 16) else {
            return Code::Unknown;
        };
        let property: String = inner
            .rsplit(").")
            .next()
            .unwrap_or_default()
            .chars()
            .take_while(|c| c.is_alphanumeric())
            .collect();
        return match property.to_ascii_lowercase().as_str() {
            "area" => Code::Area(handle),
            "length" => Code::Length(handle),
            _ => Code::Unknown,
        };
    }
    Code::Unknown
}

/// Today's date (UTC) as `yyyy-MM-dd`.
pub(crate) fn today() -> String {
    let d = time::OffsetDateTime::now_utc().date();
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

/// Today's date as the computer's clock says it where it can (Unix: `localtime_r`), else UTC.
pub(crate) fn local_today() -> String {
    #[cfg(unix)]
    {
        // SAFETY: `time(NULL)` has no preconditions; `localtime_r` writes only into the zeroed
        // `tm` owned here and returns null on failure, which is checked.
        unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            if !libc::localtime_r(&now, &mut tm).is_null() {
                return format!(
                    "{:04}-{:02}-{:02}",
                    tm.tm_year + 1900,
                    tm.tm_mon + 1,
                    tm.tm_mday
                );
            }
        }
    }
    today()
}

/// The field code a text keeps, if any.
pub(crate) fn text_field(common: &opencadcodec::entities::EntityCommon) -> Option<String> {
    crate::xdata::read_text(common, FIELD_APP)
}

/// Keeps `template` as the text's field code; `None` drops it.
pub(crate) fn set_text_field(
    common: &mut opencadcodec::entities::EntityCommon,
    template: Option<&str>,
    app_handle: u64,
) {
    crate::xdata::write_text(common, FIELD_APP, app_handle, template);
}

/// The field codes a table keeps, as (row, column, code).
pub(crate) fn cell_fields(t: &Table) -> Vec<(usize, usize, String)> {
    let Some(record) = t.common.extended_data.get_record(FIELD_APP) else {
        return Vec::new();
    };
    let mut out: Vec<(usize, usize, String)> = Vec::new();
    let mut values = record.values.iter().peekable();
    while let Some(v) = values.next() {
        let (XDataValue::Integer32(r), Some(XDataValue::Integer32(c))) = (v, values.next()) else {
            continue;
        };
        let mut code = String::new();
        while let Some(XDataValue::String(s)) = values.peek() {
            code.push_str(s);
            values.next();
        }
        if let (Ok(r), Ok(c)) = (usize::try_from(*r), usize::try_from(*c)) {
            out.push((r, c, code));
        }
    }
    out
}

/// Keeps `fields` as the table's field codes (none: the record goes).
pub(crate) fn set_cell_fields(t: &mut Table, fields: &[(usize, usize, String)], app_handle: u64) {
    let mut values = Vec::new();
    for (r, c, code) in fields {
        values.push(XDataValue::Integer32(*r as i32));
        values.push(XDataValue::Integer32(*c as i32));
        values.extend(chunks(code));
    }
    crate::xdata::write_values(
        &mut t.common,
        FIELD_APP,
        app_handle,
        (!fields.is_empty()).then_some(values),
    );
}

/// Dependencies of persisted field templates, including table cells. Counts depend on
/// membership rather than a single source; their holders have a separate index bucket.
pub(crate) fn field_dependencies(entity: &EntityType) -> (std::collections::BTreeSet<u64>, bool) {
    let codes = match entity {
        EntityType::Table(table) => cell_fields(table)
            .into_iter()
            .map(|(_, _, code)| code)
            .collect(),
        _ => text_field(entity.common()).into_iter().collect::<Vec<_>>(),
    };
    let mut sources = std::collections::BTreeSet::new();
    let mut counts = false;
    for code in codes {
        for (a, b) in code_spans(&code) {
            match parse_code(&code[a..b]) {
                Code::Area(h) | Code::Length(h) => {
                    sources.insert(h);
                }
                Code::Sum { handles, .. }
                | Code::RoomArea { handles, .. }
                | Code::ObjectValue(handles, ..) => sources.extend(handles),
                Code::Count(_) | Code::OpeningCount(..) => counts = true,
                _ => {}
            }
        }
    }
    (sources, counts)
}

impl Session {
    /// True when the drawing has no `FIELD_APP` application yet.
    pub(crate) fn needs_field_app(&self) -> bool {
        !self.drawing.doc.app_ids.contains(FIELD_APP)
    }

    pub(crate) fn register_field_app(&mut self) {
        if self.needs_field_app() {
            let doc = &mut self.drawing.doc;
            let mut app = AppId::new(FIELD_APP);
            app.handle = doc.allocate_handle();
            doc.app_ids.add(app).expect("checked it is missing");
        }
    }

    /// `template` with every field code replaced by its value now; `sheet` is the block the text
    /// sits in (counts are counted there). A measured area or length is shown with its unit
    /// unless the text after it gives one (`Area %<…>% m²`).
    pub(crate) fn evaluate_fields(&self, template: &str, sheet: Handle) -> String {
        self.evaluate_fields_with(template, sheet, true)
    }

    /// The same for a table cell, whose column heading gives the unit.
    pub(crate) fn evaluate_cell_fields(&self, template: &str, sheet: Handle) -> String {
        self.evaluate_fields_with(template, sheet, false)
    }

    fn evaluate_fields_with(&self, template: &str, sheet: Handle, units: bool) -> String {
        let _read = self.evaluation_read().ok();
        let mut out = String::with_capacity(template.len());
        let mut at = 0;
        let arabic_text = crate::numbers::has_arabic(template);
        for (a, b) in code_spans(template) {
            out.push_str(&template[at..a]);
            let code = &template[a..b];
            let value = self.field_value(code, sheet);
            let (dim, flagged) = match parse_code(code) {
                Code::Area(_) | Code::Sum { area: true, .. } | Code::RoomArea { .. } => {
                    (Some(crate::numbers::Dim::Area), false)
                }
                Code::Length(_) | Code::Sum { area: false, .. } => {
                    (Some(crate::numbers::Dim::Length), false)
                }
                Code::ObjectValue(_, name, _, arabic) => {
                    (crate::numbers::dim_of_name(&name), arabic)
                }
                _ => (None, false),
            };
            let arabic = arabic_text || flagged;
            if let (Some(dim), true) = (dim, value.parse::<f64>().is_ok()) {
                if units {
                    out.push_str(&crate::numbers::with_unit(
                        &value,
                        dim,
                        arabic,
                        &template[b..],
                    ));
                } else if arabic {
                    out.push_str(&crate::numbers::arabic_digits(&value));
                } else {
                    out.push_str(&value);
                }
            } else if flagged && value.parse::<f64>().is_ok() {
                // A count or another number in an Arabic schedule: Arabic digits.
                out.push_str(&crate::numbers::arabic_digits(&value));
            } else if arabic_text
                && (template.contains("المساحة") || template.contains("مساحة مرشحة"))
            {
                out.push_str(&crate::numbers::arabic_digits(&value));
            } else {
                out.push_str(&value);
            }
            at = b;
        }
        out.push_str(&template[at..]);
        // Saved Qiyas room fields keep their binding while adopting the approved plan wording.
        if has_object_fields(template) {
            if let Some(rest) = out.strip_prefix("Unapproved candidate ") {
                return format!("Area {rest}");
            }
            if let Some(rest) = out.strip_prefix("مساحة مرشحة غير معتمدة ") {
                return format!("المساحة {rest}");
            }
        }
        out
    }

    fn field_value(&self, code: &str, sheet: Handle) -> String {
        let doc = &self.drawing.doc;
        let measure = |h: u64, area: bool| {
            let e = doc.get_entity(Handle::new(h))?;
            let extent = self.extent(doc, e, &Xform::identity());
            let (kind, v) = if area {
                (Kind::Area, extent.area?)
            } else {
                (Kind::Length, extent.length?)
            };
            // In metres and square metres, as the take-off shows them.
            let (_, shown) = in_metres(kind, v, self.drawing.units);
            Some(format!("{shown:.2}"))
        };
        let value = match parse_code(code) {
            Code::RoomArea { handles, decimals } => self.room_field_value(&handles, decimals),
            Code::Date => Some(today()),
            Code::FileName => self
                .drawing
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned()),
            Code::Area(h) => measure(h, true),
            Code::Length(h) => measure(h, false),
            Code::Sum { handles, area } => handles
                .iter()
                .map(|h| {
                    let e = doc.get_entity(Handle::new(*h))?;
                    let extent = self.extent(doc, e, &Xform::identity());
                    let value = if area { extent.area? } else { extent.length? };
                    Some(
                        in_metres(
                            if area { Kind::Area } else { Kind::Length },
                            value,
                            self.drawing.units,
                        )
                        .1,
                    )
                })
                .collect::<Option<Vec<_>>>()
                .map(|values| format!("{:.2}", values.iter().sum::<f64>())),
            Code::Count(block) => {
                let name = normalize_name(&block);
                let sheet = doc.block_records.iter().find(|b| b.handle == sheet);
                sheet.map(|b| {
                    doc.entities_in_block(&b.name)
                        .filter(|e| match e {
                            EntityType::Insert(i) => {
                                !i.common.invisible
                                    && normalize_name(&effective_block_name(doc, i)) == name
                            }
                            _ => false,
                        })
                        .count()
                        .to_string()
                })
            }
            Code::OpeningCount(source, mark) => self
                .opening_count(Handle::new(source), &mark)
                .map(|count| count.to_string()),
            Code::ObjectValue(ids, name, decimals, _) => {
                self.object_field_value(&ids, &name, decimals)
            }
            Code::Unknown => None,
        };
        value.unwrap_or_else(|| UNKNOWN.to_string())
    }

    /// Works every field of the drawing out again (with `objects_only`, those reading an
    /// entity's area or length only), writing the texts and cells whose value changed; a table
    /// changed gets its block drawn again. A field is a derived value, so one on a locked layer
    /// (or outside the block edited in place) is worked out too: the lock stops the user, not
    /// the field (as AutoCAD). How many texts and tables changed.
    pub(crate) fn update_fields(&mut self, objects_only: bool) -> Result<usize, CoreError> {
        self.update_fields_scoped(objects_only, None, true)
    }
    fn update_fields_scoped(
        &mut self,
        objects_only: bool,
        dirty: Option<&std::collections::BTreeSet<u64>>,
        counts: bool,
    ) -> Result<usize, CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("field.update_fields_scoped");
        if self.needs_field_app() {
            return Ok(0);
        }
        let wanted = |code: &str, holder: u64| {
            (!objects_only || has_object_fields(code))
                && dirty.is_none_or(|dirty| {
                    dirty.contains(&holder)
                        || code_spans(code)
                            .iter()
                            .any(|(a, b)| match parse_code(&code[*a..*b]) {
                                Code::Area(h) | Code::Length(h) => dirty.contains(&h),
                                Code::Sum { handles, .. }
                                | Code::RoomArea { handles, .. }
                                | Code::ObjectValue(handles, ..) => {
                                    handles.iter().any(|h| dirty.contains(h))
                                }
                                Code::Count(_) | Code::OpeningCount(..) => counts,
                                _ => false,
                            })
                })
        };
        let holders: Vec<Handle> = {
            let _read = self.evaluation_read()?;
            let cache = self.evaluation.borrow();
            let index = &cache.index;
            let mut selected = std::collections::BTreeSet::new();
            if let Some(dirty) = dirty {
                for source in dirty {
                    if index.field_holders.contains(source) {
                        selected.insert(*source);
                    }
                    selected.extend(index.fields.get(source).into_iter().flatten().copied());
                }
                if counts {
                    selected.extend(index.count_fields.iter().copied());
                }
            } else {
                selected.extend(index.field_holders.iter().copied());
            }
            // Preserve document order: cell rebuilds can allocate block handles.
            index
                .field_order
                .iter()
                .copied()
                .filter(|h| selected.contains(h))
                .map(Handle::new)
                .collect()
        };
        let mut changed = 0;
        for h in holders {
            let e = self.drawing.doc.get_entity(h).expect("listed above");
            let sheet = e.common().owner_handle;
            match e {
                EntityType::Text(t) => {
                    let Some(code) = text_field(&t.common).filter(|c| wanted(c, h.value())) else {
                        continue;
                    };
                    let value = self.evaluate_fields(&code, sheet);
                    if t.value != value {
                        if let Some(EntityType::Text(t)) = self.drawing.doc.get_entity_mut(h) {
                            t.value = value;
                            t.common.graphic_data = None;
                        }
                        changed += 1;
                    }
                }
                EntityType::MText(t) => {
                    let Some(code) = text_field(&t.common).filter(|c| wanted(c, h.value())) else {
                        continue;
                    };
                    let value = escape_mtext(&self.evaluate_fields(&code, sheet));
                    if t.value != value {
                        if let Some(EntityType::MText(t)) = self.drawing.doc.get_entity_mut(h) {
                            t.value = value;
                            t.common.graphic_data = None;
                        }
                        changed += 1;
                    }
                }
                EntityType::Table(t) => {
                    let mut table = t.clone();
                    let mut any = false;
                    for (r, c, code) in cell_fields(t) {
                        if !wanted(&code, h.value()) {
                            continue;
                        }
                        // The column heading gives a measured number's unit when it has one.
                        let units = r == 0
                            || !t
                                .cell_text(0, c)
                                .is_some_and(crate::numbers::heading_gives_unit);
                        let shown = if units {
                            self.evaluate_fields(&code, sheet)
                        } else {
                            self.evaluate_cell_fields(&code, sheet)
                        };
                        let value = escape_mtext(&shown);
                        if table.cell_text(r, c).is_some_and(|now| now != value) {
                            crate::table::set_cell_value(&mut table, r, c, &value);
                            any = true;
                        }
                    }
                    if any {
                        *self.drawing.doc.get_entity_mut(h).expect("listed above") =
                            EntityType::Table(table);
                        self.rebuild_table(h)?;
                        changed += 1;
                    }
                }
                _ => {}
            }
        }
        Ok(changed)
    }

    /// After a modify, an edit or an erase: the fields reading an entity are worked out again,
    /// in the same step.
    pub(crate) fn follow_fields(&mut self) -> Result<(), CoreError> {
        self.refresh_object_tags()?;
        self.update_fields(true).map(|_| ())
    }
    pub(crate) fn follow_fields_scoped(
        &mut self,
        dirty: Option<&std::collections::BTreeSet<u64>>,
        counts: bool,
    ) -> Result<(), CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("field.follow_fields_scoped");
        // Linked object tags follow their objects in the scoped pass too (Goals C and G).
        self.refresh_object_tags()?;
        self.update_fields_scoped(true, dirty, counts).map(|_| ())
    }

    /// Follows replaced wall entities, retaining sums when one measured hatch splits.
    pub(crate) fn remap_wall_fields(
        &mut self,
        mapping: &std::collections::BTreeMap<u64, Vec<u64>>,
    ) -> Result<(), CoreError> {
        if mapping.is_empty() {
            return Ok(());
        }
        fn remap(text: &str, mapping: &std::collections::BTreeMap<u64, Vec<u64>>) -> String {
            let mut out = String::new();
            let mut at = 0;
            for (a, b) in code_spans(text) {
                out.push_str(&text[at..a]);
                let original = &text[a..b];
                if let Code::ObjectValue(ids, name, decimals, arabic) = parse_code(original) {
                    let mut replaced = std::collections::BTreeSet::new();
                    for id in ids {
                        replaced.extend(mapping.get(&id).cloned().unwrap_or_else(|| vec![id]));
                    }
                    out.push_str(&crate::object_schedules::object_value_code(
                        &replaced.into_iter().collect::<Vec<_>>(),
                        &name,
                        decimals,
                        arabic,
                    ));
                    at = b;
                    continue;
                }
                let selection = match parse_code(original) {
                    Code::Area(h) => Some((vec![h], true)),
                    Code::Length(h) => Some((vec![h], false)),
                    Code::Sum { handles, area } => Some((handles, area)),
                    _ => None,
                };
                if let Some((handles, area)) =
                    selection.filter(|(hs, _)| hs.iter().any(|h| mapping.contains_key(h)))
                {
                    let mut replaced = std::collections::BTreeSet::new();
                    for handle in handles {
                        replaced.extend(
                            mapping
                                .get(&handle)
                                .cloned()
                                .unwrap_or_else(|| vec![handle]),
                        );
                    }
                    if replaced.len() == 1 {
                        out.push_str(&object_code(
                            *replaced.first().unwrap(),
                            if area { "Area" } else { "Length" },
                        ));
                    } else if !replaced.is_empty() {
                        let ids = replaced
                            .iter()
                            .map(|h| format!("{h:X}"))
                            .collect::<Vec<_>>()
                            .join(",");
                        out.push_str(&format!(
                            r#"%<\AcVar {} "{ids}">%"#,
                            if area { "QiyasArea" } else { "QiyasLength" }
                        ));
                    } else {
                        out.push_str(original);
                    }
                } else {
                    out.push_str(original);
                }
                at = b;
            }
            out.push_str(&text[at..]);
            out
        }
        let holders: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter(|e| e.common().extended_data.get_record(FIELD_APP).is_some())
            .map(|e| e.common().handle)
            .collect();
        let field_app = crate::xdata::app_handle(&self.drawing.doc, FIELD_APP);
        for handle in holders {
            let entity = self
                .drawing
                .doc
                .get_entity_mut(handle)
                .expect("listed field holder");
            match entity {
                EntityType::Table(table) => {
                    let fields = cell_fields(table)
                        .into_iter()
                        .map(|(r, c, text)| (r, c, remap(&text, mapping)))
                        .collect::<Vec<_>>();
                    set_cell_fields(table, &fields, field_app);
                }
                _ => {
                    if let Some(text) = text_field(entity.common()) {
                        set_text_field(
                            entity.common_mut(),
                            Some(&remap(&text, mapping)),
                            field_app,
                        );
                    }
                }
            }
        }
        self.remap_object_tags(mapping)?;
        // Evaluate after geometry and schedules finish, in the shared derivation pass.
        Ok(())
    }

    /// True when following fields may draw a table's block again: a table holds a field reading
    /// an entity (so the step keeps the block records).
    pub(crate) fn tables_follow_fields(&self) -> bool {
        !self.needs_field_app()
            && self.drawing.doc.entities().any(|e| match e {
                EntityType::Table(t) => cell_fields(t).iter().any(|(_, _, c)| has_object_fields(c)),
                _ => false,
            })
    }

    /// `Command::Field`: a one-line text at `at` holding the field `kind`, `height` tall, in the
    /// current text style.
    pub(crate) fn add_field(
        &mut self,
        at: [f64; 2],
        height: f64,
        kind: &FieldKind,
    ) -> Result<u64, CoreError> {
        if let FieldKind::Area { target } | FieldKind::Length { target } = kind {
            self.sheet_targets_any(std::slice::from_ref(target))?;
        }
        if let FieldKind::Count { block } = kind {
            if block.trim().is_empty() {
                return Err(CoreError::InvalidCommand("Name the block to count".into()));
            }
        }
        self.add_text(at, height, 0.0, field_code(kind), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_found_whole_with_their_nested_ids() {
        let area = object_code(0x2A, "Area");
        let text = format!("Area: {area} m2, {}", field_code(&FieldKind::Date));
        let spans = code_spans(&text);
        assert_eq!(spans.len(), 2);
        assert_eq!(&text[spans[0].0..spans[0].1], area);
        assert!(matches!(parse_code(&area), Code::Area(0x2A)));
        assert!(matches!(
            parse_code(&object_code(0xFF, "Length")),
            Code::Length(0xFF)
        ));
        assert!(matches!(
            parse_code(&field_code(&FieldKind::Count {
                block: "DOOR".into()
            })),
            Code::Count(ref b) if b == "DOOR"
        ));
        assert!(matches!(parse_code("%<\\AcVar Filename>%"), Code::FileName));
        assert!(matches!(
            parse_code("%<\\AcDiesel $(edtime)>%"),
            Code::Unknown
        ));
        // An unclosed code is left as it is.
        assert!(code_spans("50%<\\AcVar Date").is_empty());
    }

    #[test]
    fn legacy_room_fields_adopt_plan_wording_without_losing_the_binding() {
        let session = Session::new_blank(std::path::Path::new("legacy-label.dxf"), 6);
        for (old, current) in [
            ("Unapproved candidate", "Area"),
            ("مساحة مرشحة غير معتمدة", "المساحة"),
        ] {
            let template = format!(r#"{old} %<\AcVar QiyasRoomArea "DEAD" 2>% m²"#);
            assert_eq!(
                session.evaluate_fields(&template, Handle::new(0)),
                format!("{current} #### m²")
            );
        }
    }

    #[test]
    fn long_codes_are_cut_into_xdata_strings() {
        let long = "ج".repeat(300);
        let values = chunks(&long);
        assert_eq!(values.len(), 3);
        let mut t = Table::new(opencadcodec::Vector3::ZERO, 2, 2);
        set_cell_fields(&mut t, &[(1, 0, long.clone()), (0, 1, "x".into())], 0);
        assert_eq!(cell_fields(&t), [(1, 0, long), (0, 1, "x".to_string())]);
        set_cell_fields(&mut t, &[], 0);
        assert!(cell_fields(&t).is_empty());
    }
}
