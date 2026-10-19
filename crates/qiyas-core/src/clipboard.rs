//! The clipboard: `Command::CopyToClipboard` writes the entities picked, with the layers,
//! linetypes, text and dimension styles and blocks they use, as a small DXF drawing (text),
//! marked as Qiyas's (`CLIPBOARD_MARK`), their coordinates taken from the base point.
//! `Command::PasteFromClipboard` reads it back into the sheet on screen through the block
//! `Importer` (new handles; a layer, block or style the drawing lacks is copied, one it has is
//! kept as it is), placed so the base point lands on the point given; as a block (PASTEBLOCK), a
//! new block `A$C…` holds them under one reference. Plain text pastes as a text. The system
//! clipboard itself is the desktop app's (`arboard`): the core only makes and reads the text.

use std::io::Cursor;

use opencadcodec::types::Transform;
use opencadcodec::{BlockRecord, Vector3};
use opencadcodec::{CadDocument, DxfReader, DxfReaderConfiguration, DxfWriter, EntityType, Handle};
use qiyas_geom::{flatten_with, is_xref_block, Xform};

use crate::blocks::Importer;
use crate::modify::{place, place_keeping_blocks, Placing, MAX_NEW};
use crate::{CoreError, Session, Target, Units};

/// The first line of every clipboard text Qiyas writes (a DXF comment), so paste knows it.
pub const CLIPBOARD_MARK: &str = "999\nQIYAS CLIPBOARD 1\n";

const MODEL: &str = "*Model_Space";
/// Longest clipboard drawing pasted, in bytes.
const MAX_CLIPBOARD_DXF: usize = 32 << 20;
/// Longest plain text pasted as a text, in bytes.
const MAX_CLIPBOARD_TEXT: usize = 64 << 10;

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

/// Moves the entity `h` of `doc` by `d`; a dimension takes its own block along (its block is in
/// world coordinates, see `Session::shift_block`).
fn shift_in(doc: &mut CadDocument, h: Handle, d: [f64; 2]) {
    place_in(doc, h, &Placing::shift(d));
}

/// `shift_in` for any placing (a scale from another drawing's units).
fn place_in(doc: &mut CadDocument, h: Handle, p: &Placing) {
    let Some(e) = doc.get_entity_mut(h) else {
        return;
    };
    let Some(block) = place_keeping_blocks(e, p) else {
        return;
    };
    let inside = doc
        .block_records
        .get(&block)
        .map(|b| b.entity_handles.clone())
        .unwrap_or_default();
    for h in inside {
        if let Some(e) = doc.get_entity_mut(h) {
            place(e, p);
        }
    }
}

/// The lower left corner of everything `handles` draw, or `None` when they draw nothing.
fn lower_left(session: &Session, handles: &[Handle]) -> Option<[f64; 2]> {
    let xrefs = session.xref_docs();
    let doc = &session.drawing.doc;
    let mut low: Option<[f64; 2]> = None;
    for &h in handles {
        let Some(e) = doc.get_entity(h) else {
            continue;
        };
        let outcome = flatten_with(doc, &xrefs, e, &Xform::identity(), session.batch_tol());
        for p in outcome.paths.iter().flat_map(|f| &f.path.points) {
            let l = low.get_or_insert(*p);
            l[0] = l[0].min(p[0]);
            l[1] = l[1].min(p[1]);
        }
    }
    low
}

impl Session {
    /// `Command::CopyToClipboard`: the clipboard text for the entities `targets` at the top of
    /// the sheet (locked layers too: nothing changes), taken from `base` (by default the lower
    /// left corner of what they draw), and how many entities it holds. Viewports, xref
    /// references, images and PDF underlays are left out.
    pub(crate) fn copy_to_clipboard(
        &self,
        targets: &[Target],
        base: Option<[f64; 2]>,
    ) -> Result<(String, usize), CoreError> {
        let handles = self.sheet_targets_any(targets)?;
        let base = match base {
            Some(b) if b.iter().all(|v| v.is_finite()) => b,
            Some(_) => return Err(invalid("The base point must have finite coordinates")),
            None => lower_left(self, &handles).unwrap_or([0.0, 0.0]),
        };
        let src = &self.drawing.doc;
        let mut doc = CadDocument::new();
        doc.header.insertion_units = src.header.insertion_units;
        let model = doc
            .block_records
            .get(MODEL)
            .map(|b| b.handle)
            .ok_or_else(|| invalid("A new drawing has no model space"))?;
        let mut copied = Vec::new();
        {
            let mut importer = Importer::new(src, &mut doc);
            for &h in &handles {
                let e = src.get_entity(h).expect("sheet_targets_any found it");
                let xref = matches!(e, EntityType::Insert(i) if is_xref_block(src, &i.block_name));
                if xref || matches!(e, EntityType::Viewport(_)) {
                    continue;
                }
                if let Some(added) = importer.copy(e, model)? {
                    copied.push(Handle::new(added));
                }
            }
        }
        if copied.is_empty() {
            return Err(invalid(
                "Nothing to copy: viewports, pictures and xrefs stay",
            ));
        }
        for &h in &copied {
            shift_in(&mut doc, h, [-base[0], -base[1]]);
        }
        let bytes = DxfWriter::new(&doc)
            .write_to_vec()
            .map_err(|e| CoreError::Unwritable(e.to_string()))?;
        let dxf = String::from_utf8_lossy(&bytes);
        Ok((format!("{CLIPBOARD_MARK}{dxf}"), copied.len()))
    }

    /// `Command::PasteFromClipboard`: what `text` holds, placed at `at` on the sheet on screen
    /// (see the module documentation); the new entities' handles. Plain text becomes a text (an
    /// MText when it has several lines) `height` tall, by default 2.5 mm at the annotation scale.
    pub(crate) fn paste_from_clipboard(
        &mut self,
        at: [f64; 2],
        text: Option<&str>,
        as_block: bool,
        height: Option<f64>,
    ) -> Result<Vec<u64>, CoreError> {
        if !at.iter().all(|v| v.is_finite()) {
            return Err(invalid("The point must have finite coordinates"));
        }
        let text = text.unwrap_or_default();
        let Some(dxf) = text.strip_prefix(CLIPBOARD_MARK) else {
            if text.len() > MAX_CLIPBOARD_TEXT {
                return Err(invalid(format!(
                    "The clipboard text is longer than {} KB: paste less at a time",
                    MAX_CLIPBOARD_TEXT >> 10
                )));
            }
            return self.paste_text(at, text, height);
        };
        if dxf.len() > MAX_CLIPBOARD_DXF {
            return Err(invalid(format!(
                "The clipboard drawing is larger than {} MB: copy less at a time",
                MAX_CLIPBOARD_DXF >> 20
            )));
        }
        let config = DxfReaderConfiguration {
            failsafe: true,
            ..Default::default()
        };
        let read = DxfReader::from_reader(Cursor::new(dxf.as_bytes().to_vec()))
            .and_then(|r| r.with_configuration(config).read_with_stats())
            .map_err(|e| invalid(format!("The clipboard could not be read: {e}")))?;
        let src = read.document;
        // Drawn in other units (unitless as millimetres): scaled to this drawing's.
        let factor = Units::from_header(src.header.insertion_units).to_metres()
            / self.drawing.units.to_metres();
        let count = src.entities_in_block(MODEL).count();
        if count == 0 {
            return Err(invalid("The clipboard holds no entities"));
        }
        if count > MAX_NEW {
            return Err(invalid(format!(
                "The clipboard holds more than {MAX_NEW} entities: copy fewer at a time"
            )));
        }
        if as_block {
            let name = (1u32..)
                .map(|n| format!("A$C{n:08X}"))
                .find(|n| !self.drawing.doc.block_records.contains(n))
                .expect("a free name");
            {
                let mut importer = Importer::new(&src, &mut self.drawing.doc);
                let owner = importer.new_record(&name, &BlockRecord::new(&name))?;
                importer.fill(MODEL, owner, Vector3::ZERO)?;
            }
            let values = Default::default();
            return Ok(vec![
                self.insert_block(&name, at, factor, 0.0, &values, None)?
            ]);
        }
        let owner = self
            .sheet_owner()
            .ok_or_else(|| invalid(format!("The sheet {} is missing", self.sheet)))?;
        let mut added = Vec::new();
        {
            let mut importer = Importer::new(&src, &mut self.drawing.doc);
            for e in src.entities_in_block(MODEL) {
                if let Some(h) = importer.copy(e, owner)? {
                    added.push(h);
                }
            }
        }
        let scale = Transform::from_scaling(Vector3::new(factor, factor, factor));
        for &h in &added {
            if factor != 1.0 {
                place_in(
                    &mut self.drawing.doc,
                    Handle::new(h),
                    &Placing::plain(scale),
                );
            }
            shift_in(&mut self.drawing.doc, Handle::new(h), at);
        }
        Ok(added)
    }

    /// Plain clipboard text as a text at `at`.
    fn paste_text(
        &mut self,
        at: [f64; 2],
        text: &str,
        height: Option<f64>,
    ) -> Result<Vec<u64>, CoreError> {
        let value = text.replace("\r\n", "\n");
        let value = value.trim_end_matches('\n');
        if value.trim().is_empty() {
            return Err(invalid("The clipboard is empty"));
        }
        let height = height
            .unwrap_or_else(|| 0.0025 / self.drawing.units.to_metres() * self.annotation_scale().1);
        let h = if value.contains('\n') {
            self.add_mtext(at, 0.0, height, value, None)?
        } else {
            self.add_text(at, height, 0.0, value.to_string(), None)?
        };
        Ok(vec![h])
    }
}
