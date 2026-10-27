//! The drawing-pack check card (W6): one short card before take-off. Each line passes or names
//! the fix. It builds on what Qiyas already knows: the drawing's units, the annotation scale, the
//! PDF pages and images attached (vector or scanned, and whether a scan was calibrated), the page
//! orientation and the sheet number and revision the register rules read.
//!
//! It reads; it changes nothing but the note of which pictures were calibrated (see
//! `mark_calibrated`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use opencadcodec::objects::ObjectType;
use opencadcodec::EntityType;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::knowledge::{self, Kind};
use crate::{CoreError, Session, Units};

/// The knowledge entry (under the measurement kind) that lists the pictures calibrated.
const CALIBRATED: &str = "calibrated";

/// One line of the card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackLine {
    pub id: String,
    pub passes: bool,
    /// What was found, in English and Arabic.
    pub found_en: String,
    pub found_ar: String,
    /// What to do, when it does not pass.
    pub fix_en: String,
    pub fix_ar: String,
}

/// The card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackCard {
    /// Every line passes.
    pub ready: bool,
    pub lines: Vec<PackLine>,
}

impl PackCard {
    pub fn line(&self, id: &str) -> Option<&PackLine> {
        self.lines.iter().find(|l| l.id == id)
    }
}

/// An English and an Arabic text.
type Pair = (String, String);

fn both(en: impl Into<String>, ar: impl Into<String>) -> Pair {
    (en.into(), ar.into())
}

fn line(id: &str, passes: bool, found: Pair, fix: Pair) -> PackLine {
    PackLine {
        id: id.into(),
        passes,
        found_en: found.0,
        found_ar: found.1,
        fix_en: if passes { String::new() } else { fix.0 },
        fix_ar: if passes { String::new() } else { fix.1 },
    }
}

/// A PDF page or an image placed in the drawing.
struct Picture {
    handle: u64,
    sheet: String,
    /// "plan.pdf p2" or "scan.png".
    name: String,
    pdf: Option<(PathBuf, u32)>,
}

impl Session {
    /// The pictures calibrated with `Command::Calibrate`, by handle.
    fn calibrated_pictures(&self) -> Result<BTreeSet<u64>, CoreError> {
        match knowledge::get(&self.drawing.doc, Kind::Measurement, CALIBRATED)? {
            Some(entry) => serde_json::from_value(entry.data)
                .map_err(|e| CoreError::InvalidCommand(e.to_string())),
            None => Ok(BTreeSet::new()),
        }
    }

    /// Notes that the picture `handle` was calibrated, so the card can say a scan is to scale.
    pub(crate) fn mark_calibrated(&mut self, handle: u64) -> Result<(), CoreError> {
        let mut set = self.calibrated_pictures()?;
        if set.insert(handle) {
            knowledge::put(
                &mut self.drawing.doc,
                Kind::Measurement,
                CALIBRATED,
                &json!(set),
            )?;
        }
        Ok(())
    }

    fn pictures(&self) -> Vec<Picture> {
        let mut out = Vec::new();
        let folder = self
            .drawing
            .path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        for sheet in self.drawing.sheets() {
            for entity in self.drawing.doc.entities_in_block(&sheet.block) {
                match entity {
                    EntityType::Underlay(u) => {
                        let Some(ObjectType::UnderlayDefinition(def)) =
                            self.drawing.doc.objects.get(&u.definition_handle)
                        else {
                            continue;
                        };
                        let path = Path::new(&def.file_path);
                        let path = if path.is_absolute() {
                            path.to_path_buf()
                        } else {
                            folder.join(path)
                        };
                        let page = def.page_name.trim().parse::<u32>().unwrap_or(1);
                        let file = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        out.push(Picture {
                            handle: u.common.handle.value(),
                            sheet: sheet.name.clone(),
                            name: format!("{file} p{page}"),
                            pdf: Some((path, page)),
                        });
                    }
                    EntityType::RasterImage(image) => {
                        let name = match image
                            .definition_handle
                            .and_then(|h| self.drawing.doc.objects.get(&h))
                        {
                            Some(ObjectType::ImageDefinition(def)) => Path::new(&def.file_name)
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| def.file_name.clone()),
                            _ => "image".into(),
                        };
                        out.push(Picture {
                            handle: image.common.handle.value(),
                            sheet: sheet.name.clone(),
                            name,
                            pdf: None,
                        });
                    }
                    _ => {}
                }
            }
        }
        out
    }

    /// The card for the drawing as it is now.
    pub fn pack_check(&self) -> Result<PackCard, CoreError> {
        let mut lines = vec![self.units_line(), self.annotation_line()];
        let pictures = self.pictures();
        if !pictures.is_empty() {
            lines.extend(self.picture_lines(&pictures)?);
        }
        lines.push(self.register_line()?);
        Ok(PackCard {
            ready: lines.iter().all(|l| l.passes),
            lines,
        })
    }

    fn units_line(&self) -> PackLine {
        let known = self.drawing.units != Units::Unknown;
        let found = if known {
            both("The drawing's units are known.", "وحدات الرسم معروفة.")
        } else {
            both(
                "The drawing has no known units (INSUNITS 0), so lengths would be read as millimetres.",
                "وحدات الرسم غير معروفة (INSUNITS 0)، فستُقرأ الأطوال بالمليمتر.",
            )
        };
        line(
            "units",
            known,
            found,
            both(
                "Ask who made the drawing which unit it uses, and set it in the drawing before measuring.",
                "اسأل من أعدّ الرسم عن الوحدة المستخدمة، واضبطها في الرسم قبل القياس.",
            ),
        )
    }

    fn annotation_line(&self) -> PackLine {
        let set = self
            .drawing
            .doc
            .drawing_variable("CANNOSCALE")
            .is_some_and(|v| !v.trim().is_empty())
            || !self
                .drawing
                .doc
                .header
                .current_annotation_scale
                .trim()
                .is_empty();
        let (name, _) = self.annotation_scale();
        let found = if set {
            both(
                format!("The annotation scale is {name}."),
                format!("مقياس الكتابة {name}."),
            )
        } else {
            both("No annotation scale is set.", "لم يُضبط مقياس الكتابة.")
        };
        line(
            "annotation_scale",
            set,
            found,
            both(
                "Set the annotation scale (for example 1:100) so text and dimensions are sized right.",
                "اضبط مقياس الكتابة (مثل 1:100) ليكون حجم النصوص والأبعاد صحيحًا.",
            ),
        )
    }

    fn picture_lines(&self, pictures: &[Picture]) -> Result<Vec<PackLine>, CoreError> {
        let calibrated = self.calibrated_pictures()?;
        let pdfium = qiyas_pdf::available();
        let mut kinds = Vec::new();
        let mut unknown = Vec::new();
        let mut rotated = Vec::new();
        let mut scans: Vec<(&Picture, bool)> = Vec::new();
        for p in pictures {
            match &p.pdf {
                None => {
                    kinds.push(format!("{} scanned", p.name));
                    scans.push((p, calibrated.contains(&p.handle)));
                }
                Some((path, page)) => {
                    if !pdfium || !path.exists() {
                        unknown.push(p.name.clone());
                        continue;
                    }
                    let vector = qiyas_pdf::extract_vectors(path, *page, 1.0)
                        .map(|e| e.iter().any(|e| !matches!(e, EntityType::Text(_))))
                        .unwrap_or(false);
                    kinds.push(format!(
                        "{} {}",
                        p.name,
                        if vector { "vector" } else { "scanned" }
                    ));
                    if !vector {
                        scans.push((p, calibrated.contains(&p.handle)));
                    }
                    if let Ok(turn) = qiyas_pdf::page_rotation(path, *page) {
                        if turn != 0 {
                            rotated.push(format!("{} ({turn}°)", p.name));
                        }
                    }
                }
            }
        }
        let mut out = Vec::new();
        let listed = kinds.join("; ");
        let found = if unknown.is_empty() {
            both(
                format!("Each page is vector or scanned: {listed}."),
                format!("كل صفحة إما متجهية أو ممسوحة: {listed}."),
            )
        } else {
            both(
                format!("Could not read: {}.", unknown.join(", ")),
                format!("تعذّرت قراءة: {}.", unknown.join("، ")),
            )
        };
        let fix = if pdfium {
            both(
                "Check the file is where the drawing says, or attach it again.",
                "تأكد أن الملف في مكانه المذكور في الرسم، أو أرفقه من جديد.",
            )
        } else {
            both(
                "Install PDF support (scripts/pdfium.sh) so pages can be told apart.",
                "ثبّت دعم PDF (scripts/pdfium.sh) ليمكن التمييز بين الصفحات.",
            )
        };
        if pictures.iter().any(|p| p.pdf.is_some()) {
            out.push(line("pdf_pages", unknown.is_empty(), found, fix));
        }
        if !scans.is_empty() {
            let missing: Vec<String> = scans
                .iter()
                .filter(|(_, done)| !done)
                .map(|(p, _)| format!("{} on {}", p.name, p.sheet))
                .collect();
            let found = if missing.is_empty() {
                both(
                    "Every scan is calibrated.",
                    "كل صورة ممسوحة مضبوطة المقياس.",
                )
            } else {
                both(
                    format!("Not calibrated: {}.", missing.join(", ")),
                    format!("غير مضبوطة المقياس: {}.", missing.join("، ")),
                )
            };
            out.push(line(
                "calibration",
                missing.is_empty(),
                found,
                both(
                    "Calibrate each scan: pick two points on a dimension you know and type its real length.",
                    "اضبط مقياس كل صورة ممسوحة: اختر نقطتين على بُعد معلوم واكتب طوله الحقيقي.",
                ),
            ));
        }
        if pictures.iter().any(|p| p.pdf.is_some()) && pdfium {
            let found = if rotated.is_empty() {
                both(
                    "Every page is stored upright.",
                    "كل الصفحات مخزّنة بالاتجاه المعتاد.",
                )
            } else {
                both(
                    format!("Stored turned: {}.", rotated.join(", ")),
                    format!("مخزّنة بعد تدوير: {}.", rotated.join("، ")),
                )
            };
            out.push(line(
                "orientation",
                rotated.is_empty(),
                found,
                both(
                    "Look at each turned page once and confirm it reads upright before calibrating.",
                    "راجع كل صفحة مُدوَّرة مرة وتأكد أنها تُقرأ باتجاه صحيح قبل ضبط المقياس.",
                ),
            ));
        }
        Ok(out)
    }

    fn register_line(&self) -> Result<PackLine, CoreError> {
        let mut rules =
            crate::deliverables::default_layout(crate::deliverables::Kind::Register).register_rules;
        if let Some(entry) = knowledge::get(&self.drawing.doc, Kind::Reports, "register")? {
            if let Ok(layout) = serde_json::from_value::<crate::deliverables::Layout>(entry.data) {
                rules = layout.register_rules;
            }
        }
        let fix = || {
            both(
                "Say where the sheet number and revision are written (an attribute tag or a text pattern), or write them in the title block.",
                "حدّد أين يُكتب رقم اللوحة والمراجعة (وسم خاصية أو نمط نص)، أو اكتبهما في إطار العنوان.",
            )
        };
        if rules.is_empty() {
            return Ok(line(
                "register",
                false,
                both(
                    "No rule says where the sheet number and revision are written.",
                    "لا توجد قاعدة تحدد مكان رقم اللوحة والمراجعة.",
                ),
                fix(),
            ));
        }
        let path = self.drawing.path.to_string_lossy().to_string();
        let sheets: Vec<String> = self
            .drawing
            .sheets()
            .into_iter()
            .filter(|s| !s.is_model)
            .map(|s| s.name)
            .collect();
        let rows = crate::projects::drawing_register(self, &path, &sheets, &rules)?;
        let bad: Vec<String> = rows
            .iter()
            .filter(|r| r.number.is_none() || r.revision.is_none() || !r.flags.is_empty())
            .map(|r| {
                let why = if r.number.is_none() {
                    "no number"
                } else if r.revision.is_none() {
                    "no revision"
                } else {
                    "conflicting values"
                };
                format!("{} ({why})", r.sheet)
            })
            .collect();
        let passes = !rows.is_empty() && bad.is_empty();
        let found = if passes {
            both(
                format!(
                    "Sheet number and revision are read for {} sheet(s).",
                    rows.len()
                ),
                format!("قُرئ رقم اللوحة والمراجعة لعدد {} من اللوحات.", rows.len()),
            )
        } else if rows.is_empty() {
            both(
                "No sheet has a title block to read.",
                "لا توجد لوحة بإطار عنوان للقراءة.",
            )
        } else {
            both(
                format!("Not read: {}.", bad.join(", ")),
                format!("لم تُقرأ: {}.", bad.join("، ")),
            )
        };
        Ok(line("register", passes, found, fix()))
    }
}

/// The card as the JSON `measurement.pack_check` returns.
pub fn as_json(card: &PackCard) -> Value {
    json!(card)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_that_passes_names_no_fix() {
        let l = line("x", true, both("ok", "تمام"), both("do this", "افعل هذا"));
        assert!(l.fix_en.is_empty() && l.fix_ar.is_empty());
        let l = line("x", false, both("bad", "سيئ"), both("do this", "افعل هذا"));
        assert_eq!(l.fix_en, "do this");
    }
}
