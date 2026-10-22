//! PDF pages in the drawing: imported as lines, polylines and texts, or attached as PDF underlays
//! (an `Underlay` entity and its `UnderlayDefinition` in the `ACAD_PDFDEFINITIONS` dictionary,
//! as AutoCAD stores them, so AutoCAD shows them too). Underlays are drawn as pictures under the
//! lines: the batch carries where (`qiyas_render::ImagePlacement`), the window asks for the
//! pixels (`Session::underlay_image`).
//!
//! Underlay scale follows AutoCAD: at scale 1 the page is drawn at its paper size in the
//! drawing's units (an A1 sheet is 841 units wide in a millimetre drawing), so the entity's scale
//! is the `scale` asked for (drawing units per PDF point) over the drawing units in one point.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use opencadcodec::entities::{Underlay, UnderlayDefinition};
use opencadcodec::objects::{Dictionary, ObjectType};
use opencadcodec::{CadDocument, Color, EntityType, Handle, Layer, Vector3};
use qiyas_render::{layer_shown, ImagePlacement, IMAGE_FADED, IMAGE_MONOCHROME};
use serde::{Deserialize, Serialize};

use crate::{Command, CoreError, Outcome, Session, Source, Units};

/// How `Session::attach_pdf` brings a page in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachMode {
    /// A picture of the page under the drawing, kept in the DWG as a PDF underlay.
    Underlay,
    /// The page's vectors and texts as entities.
    Import,
}

/// The layer new underlays go on, so the layers panel can hide them all at once.
pub const UNDERLAY_LAYER: &str = "PDF-UNDERLAY";
/// The named-object dictionary entry AutoCAD keeps PDF definitions in.
const DEFINITIONS: &str = "ACAD_PDFDEFINITIONS";
/// The longest side of an underlay picture, in pixels.
const MAX_IMAGE_PX: f64 = 2048.0;
/// The shortest an underlay picture's long side gets, in pixels.
const MIN_IMAGE_PX: f64 = 64.0;

/// Underlay bookkeeping kept across batches.
#[derive(Debug, Default)]
pub(crate) struct Underlays {
    /// The underlay handle of each image id (the index).
    ids: Vec<u64>,
    /// Page sizes in points by file and page; the error text when it could not be read.
    pages: HashMap<(PathBuf, u32), Result<(f64, f64), qiyas_pdf::PdfError>>,
    /// What went wrong drawing the underlays of the last batch.
    pub problems: Vec<String>,
}

/// What to draw for one picture of the batch (IPC `underlay_image`): a page of a PDF or a raster
/// image file, at a resolution for the zoom asked for (`draw_picture` draws it).
#[derive(Debug, Clone, PartialEq)]
pub struct UnderlayImage {
    pub path: PathBuf,
    /// As in the batch's image table.
    pub version: u64,
    pub source: PictureSource,
}

/// Which pixels an `UnderlayImage` is.
#[derive(Debug, Clone, PartialEq)]
pub enum PictureSource {
    /// Page `page` (from 1) of a PDF, rendered at `dpi`.
    Pdf { page: u32, dpi: f64 },
    /// A raster image (png, jpg, tif, bmp, webp): the part `crop` (left, top, right, bottom as
    /// shares 0..1 of the image as shown, from its top-left corner; `None` for all of it), its
    /// long side `long_px` pixels or the file's own when smaller.
    Raster {
        crop: Option<[f64; 4]>,
        long_px: u32,
    },
}

impl UnderlayImage {
    /// Tells two resolutions of the same picture apart, for a cache key.
    pub fn resolution(&self) -> u64 {
        match self.source {
            PictureSource::Pdf { dpi, .. } => dpi.to_bits(),
            PictureSource::Raster { long_px, .. } => u64::from(long_px),
        }
    }
}

/// A pickable picture's handle and corners.
pub(crate) type Outline = (u64, [[f64; 2]; 4]);

/// One picture as drawn on the sheet: a PDF underlay's page or a raster image.
pub(crate) struct Placed {
    pub handle: u64,
    pub file: PathBuf,
    pub picture: Picture,
    /// Top-left, top-right, bottom-right, bottom-left of the picture as it stands upright.
    pub corners: [[f64; 2]; 4],
    pub flags: u32,
    /// A raster image, picked by its outline (PDF underlays are not).
    pub pickable: bool,
}

/// What a `Placed` shows.
pub(crate) enum Picture {
    /// Page `page` of `size` points.
    Pdf { page: u32, size: (f64, f64) },
    /// See `PictureSource::Raster`.
    Raster { crop: Option<[f64; 4]> },
}

impl Placed {
    /// The picture's longer side on the sheet, in drawing units.
    fn long_units(&self) -> f64 {
        let [tl, tr, _, bl] = self.corners;
        let side = |a: [f64; 2], b: [f64; 2]| (b[0] - a[0]).hypot(b[1] - a[1]);
        side(tl, tr).max(side(tl, bl))
    }

    /// Names the picture: file, page or crop, the file's date and size, and the size on the
    /// sheet (a rescaled picture needs another resolution). Cut to 53 bits for JavaScript.
    fn version(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut write = |bytes: &[u8]| {
            for &b in bytes {
                hash = (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        write(self.file.as_os_str().as_encoded_bytes());
        match &self.picture {
            Picture::Pdf { page, .. } => write(&page.to_le_bytes()),
            Picture::Raster { crop } => {
                for v in crop.iter().flatten() {
                    write(&v.to_le_bytes());
                }
            }
        }
        if let Ok(meta) = std::fs::metadata(&self.file) {
            write(&meta.len().to_le_bytes());
            let modified = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            write(&modified.to_le_bytes());
        }
        write(&self.long_units().to_le_bytes());
        hash & ((1 << 53) - 1)
    }
}

/// Drawing units in one PDF point (1/72 inch); unknown units count as millimetres.
fn units_per_point(units: Units) -> f64 {
    0.0254 / units.to_metres() / 72.0
}

fn check_scale(scale: f64) -> Result<(), CoreError> {
    if scale > 0.0 && scale.is_finite() {
        Ok(())
    } else {
        Err(CoreError::InvalidCommand(
            "The scale must be a number above 0".to_string(),
        ))
    }
}

/// The colour a `PDF-RRGGBB` or `PDF-FILL-RRGGBB` layer is named after; white for `PDF-TEXT`.
fn layer_color(name: &str) -> Color {
    let hex = name
        .strip_prefix(qiyas_pdf::FILL_LAYER_PREFIX)
        .or_else(|| name.strip_prefix(qiyas_pdf::LAYER_PREFIX))
        .and_then(|h| u32::from_str_radix(h, 16).ok().filter(|_| h.len() == 6));
    match hex {
        Some(v) => {
            let [_, r, g, b] = v.to_be_bytes();
            Color::Rgb { r, g, b }
        }
        None => Color::Index(7),
    }
}

/// Moves an imported entity by `d`.
fn translate(entity: &mut EntityType, d: [f64; 2]) {
    let shift = |p: &mut Vector3| {
        p.x += d[0];
        p.y += d[1];
    };
    match entity {
        EntityType::Line(l) => {
            shift(&mut l.start);
            shift(&mut l.end);
        }
        EntityType::LwPolyline(p) => {
            for v in &mut p.vertices {
                v.location.x += d[0];
                v.location.y += d[1];
            }
        }
        EntityType::Text(t) => {
            shift(&mut t.insertion_point);
            if let Some(a) = t.alignment_point.as_mut() {
                shift(a);
            }
        }
        _ => {}
    }
}

/// A PDF page read and ready to add (`Session::apply_pdf`): its entities for an import, or its
/// size for an underlay. Reading needs no session, so it can run without holding one.
#[derive(Debug, Clone)]
pub struct PreparedPdf {
    /// Absolute.
    path: PathBuf,
    page: u32,
    scale: f64,
    content: Content,
}

#[derive(Debug, Clone)]
enum Content {
    Entities(Vec<EntityType>),
    Size((f64, f64)),
}

impl PreparedPdf {
    /// Reads page `page` (from 1) of the PDF at `path` for `mode`, `scale` drawing units per
    /// point: its vectors and texts (import) or its size (underlay; the file and page are checked).
    pub fn read(path: &Path, page: u32, scale: f64, mode: AttachMode) -> Result<Self, CoreError> {
        check_scale(scale)?;
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let content = match mode {
            AttachMode::Import => {
                Content::Entities(qiyas_pdf::extract_vectors(&path, page, scale)?)
            }
            AttachMode::Underlay => Content::Size(qiyas_pdf::page_size(&path, page)?),
        };
        Ok(PreparedPdf {
            path,
            page,
            scale,
            content,
        })
    }
}

/// The file a stored link (an underlay's or image's definition) names, for a drawing at
/// `drawing`: as written when that file exists, else relative to the drawing's folder, else by
/// its file name in the drawing's folder, else by its file name in an `images` folder there.
pub(crate) fn resolve_link(drawing: &Path, stored: &str) -> Option<PathBuf> {
    let normalized = if cfg!(windows) {
        stored.to_string()
    } else {
        stored.replace('\\', "/")
    };
    let as_written = PathBuf::from(&normalized);
    let folder = drawing.parent().unwrap_or(Path::new("."));
    let mut tries = vec![];
    if as_written.is_absolute() {
        tries.push(as_written.clone());
    } else {
        tries.push(folder.join(&as_written));
    }
    if let Some(name) = as_written.file_name() {
        tries.push(folder.join(name));
        tries.push(folder.join("images").join(name));
    }
    tries.into_iter().find(|p| p.is_file())
}

/// `file` as a link of a drawing at `drawing` stores it: relative to the drawing's folder when
/// inside it, else absolute.
pub(crate) fn stored_link(drawing: &Path, file: &Path) -> String {
    let absolute = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    let folder = drawing.parent().and_then(|f| std::path::absolute(f).ok());
    match folder.and_then(|f| absolute.strip_prefix(f).ok().map(Path::to_path_buf)) {
        Some(relative) => relative.display().to_string(),
        None => absolute.display().to_string(),
    }
}

/// Rewrites the PDF underlay and image links of `doc`, written for a drawing at `from`, for the
/// same drawing saved at `to`: each file found from `from` is stored relative to `to`'s folder
/// when inside it, else by its absolute path. A link whose file is not found is kept as it is.
pub(crate) fn rebase_links(doc: &mut CadDocument, from: &Path, to: &Path) {
    for object in doc.objects.values_mut() {
        let stored = match object {
            ObjectType::UnderlayDefinition(def) => &mut def.file_path,
            ObjectType::ImageDefinition(def) => &mut def.file_name,
            _ => continue,
        };
        if let Some(file) = resolve_link(from, stored) {
            *stored = stored_link(to, &file);
        }
    }
}

impl Session {
    /// Brings page `page` (from 1) of the PDF at `path` into the sheet on screen, its bottom-left
    /// corner at `insert_at`, `scale` drawing units per PDF point: as an underlay (returns its
    /// handle) or imported as entities (returns how many). One undo step either way.
    pub fn attach_pdf(
        &mut self,
        path: &Path,
        page: u32,
        insert_at: [f64; 2],
        scale: f64,
        mode: AttachMode,
    ) -> Result<u64, CoreError> {
        let path = path.display().to_string();
        let cmd = match mode {
            AttachMode::Underlay => Command::AttachPdfUnderlay {
                path,
                page,
                insert_at,
                scale,
            },
            AttachMode::Import => Command::ImportPdf {
                path,
                page,
                insert_at,
                scale,
            },
        };
        let Outcome::Added { handles } = self.run(cmd)? else {
            return Ok(0);
        };
        Ok(match mode {
            AttachMode::Underlay => handles.first().copied().unwrap_or(0),
            AttachMode::Import => handles.len() as u64,
        })
    }

    /// Rescales the underlay or image `handle` about `p1` so that `p1` to `p2` becomes
    /// `real_distance` drawing units long. One undo step.
    pub fn calibrate(
        &mut self,
        handle: u64,
        p1: [f64; 2],
        p2: [f64; 2],
        real_distance: f64,
    ) -> Result<(), CoreError> {
        self.run(Command::Calibrate {
            handle,
            p1,
            p2,
            real_distance,
        })
        .map(|_| ())
    }

    /// The topmost PDF underlay or raster image drawn on the sheet on screen that covers `p`.
    pub fn underlay_at(&self, p: [f64; 2]) -> Option<u64> {
        let placed = self.placed_pictures();
        placed
            .iter()
            .rev()
            .find(|u| inside(&u.corners, p))
            .map(|u| u.handle)
    }

    /// What to draw for image `id` of the batch at a zoom of `px_per_unit` screen pixels per
    /// drawing unit: the zoom is rounded up to a power of two (so a picture serves zooms up to
    /// twice apart) and the picture's long side kept between 64 and 2048 pixels (a raster image
    /// never beyond its own pixels).
    pub fn underlay_image(&self, id: u32, px_per_unit: f64) -> Result<UnderlayImage, CoreError> {
        let handle = self.underlays.borrow().ids.get(id as usize).copied();
        let missing = || CoreError::InvalidCommand(format!("No underlay picture {id}"));
        let handle = handle.ok_or_else(missing)?;
        let placed = self.placed_pictures();
        let u = placed
            .iter()
            .find(|u| u.handle == handle)
            .ok_or_else(missing)?;
        let long_units = u.long_units();
        let bucket = if px_per_unit > 0.0 && px_per_unit.is_finite() {
            2f64.powf(px_per_unit.log2().ceil())
        } else {
            1024.0 / long_units
        };
        let long_px = (long_units * bucket).clamp(MIN_IMAGE_PX, MAX_IMAGE_PX);
        let source = match &u.picture {
            Picture::Pdf { page, size: (w, h) } => PictureSource::Pdf {
                page: *page,
                dpi: long_px / (w.max(*h) / 72.0),
            },
            Picture::Raster { crop } => PictureSource::Raster {
                crop: *crop,
                long_px: long_px.round() as u32,
            },
        };
        Ok(UnderlayImage {
            path: u.file.clone(),
            version: u.version(),
            source,
        })
    }

    /// The pictures (PDF underlays and raster images) drawn on the sheet `sheet`, on screen or
    /// not, for a plot: each one's handle, placement (`id` 0) and what to draw for it at
    /// `px_per_unit` pixels per drawing unit, its long side at most `max_px` (a raster image
    /// never beyond its own pixels); and why others can't be drawn (file not found, ...).
    pub fn plot_pictures(
        &self,
        sheet: &str,
        px_per_unit: f64,
        max_px: f64,
    ) -> (Vec<(u64, ImagePlacement, UnderlayImage)>, Vec<String>) {
        let mut problems = Vec::new();
        let placed = self.placed_on(sheet, &mut problems);
        let pictures = placed
            .iter()
            .map(|u| {
                let long_px =
                    (u.long_units() * px_per_unit).clamp(MIN_IMAGE_PX, max_px.max(MIN_IMAGE_PX));
                let source = match &u.picture {
                    Picture::Pdf { page, size: (w, h) } => PictureSource::Pdf {
                        page: *page,
                        dpi: long_px / (w.max(*h) / 72.0),
                    },
                    Picture::Raster { crop } => PictureSource::Raster {
                        crop: *crop,
                        long_px: long_px.round() as u32,
                    },
                };
                let placement = ImagePlacement {
                    id: 0,
                    version: u.version(),
                    corners: u.corners,
                    flags: u.flags,
                };
                let image = UnderlayImage {
                    path: u.file.clone(),
                    version: u.version(),
                    source,
                };
                (u.handle, placement, image)
            })
            .collect();
        (pictures, problems)
    }

    /// Where the pictures (PDF underlays and raster images) on the sheet on screen are drawn, by
    /// image id, and the raster images' outlines to pick them by; notes their problems.
    pub(crate) fn underlay_placements(&self) -> (Vec<ImagePlacement>, Vec<Outline>) {
        let placed = self.placed_pictures();
        let mut underlays = self.underlays.borrow_mut();
        let placements = placed
            .iter()
            .map(|u| {
                let id = match underlays.ids.iter().position(|h| *h == u.handle) {
                    Some(i) => i,
                    None => {
                        underlays.ids.push(u.handle);
                        underlays.ids.len() - 1
                    }
                };
                ImagePlacement {
                    id: id as u32,
                    version: u.version(),
                    corners: u.corners,
                    flags: u.flags,
                }
            })
            .collect();
        let outlines = placed
            .iter()
            .filter(|u| u.pickable)
            .map(|u| (u.handle, u.corners))
            .collect();
        (placements, outlines)
    }

    /// The PDF underlays and raster images on the sheet on screen that are drawn: visible, on, on
    /// a shown layer, with their file (and page) found. The others are listed in
    /// `Underlays::problems`.
    pub(crate) fn placed_pictures(&self) -> Vec<Placed> {
        let mut problems = Vec::new();
        let out = self.placed_on(&self.sheet, &mut problems);
        self.underlays.borrow_mut().problems = problems;
        out
    }

    /// The pictures drawn on the sheet `sheet` (on screen or not), and in `problems` why the
    /// others are not.
    fn placed_on(&self, sheet: &str, problems: &mut Vec<String>) -> Vec<Placed> {
        let doc = &self.drawing.doc;
        let per_point = units_per_point(self.drawing.units);
        let mut underlays = self.underlays.borrow_mut();
        let mut out = Vec::new();
        for entity in doc.entities_in_block(sheet) {
            if let EntityType::RasterImage(image) = entity {
                match crate::image::placed_raster(doc, &self.drawing.path, &self.visibility, image)
                {
                    Ok(Some(placed)) => out.push(placed),
                    Ok(None) => {}
                    Err(problem) => {
                        if !problems.contains(&problem) {
                            problems.push(problem);
                        }
                    }
                }
                continue;
            }
            let EntityType::Underlay(u) = entity else {
                continue;
            };
            if u.common.invisible
                || !u.is_on()
                || !layer_shown(doc, &self.visibility, &u.common.layer)
            {
                continue;
            }
            let Some(ObjectType::UnderlayDefinition(def)) = doc.objects.get(&u.definition_handle)
            else {
                continue;
            };
            if def.unloaded {
                continue;
            }
            let Some(file) = resolve_link(&self.drawing.path, &def.file_path) else {
                problems.push(format!("PDF underlay file not found: {}", def.file_path));
                continue;
            };
            let page = def.page_name.trim().parse::<u32>().unwrap_or(1).max(1);
            let size = match underlays.pages.get(&(file.clone(), page)) {
                Some(found) => found.clone(),
                None => {
                    let found = qiyas_pdf::page_size(&file, page);
                    // Not kept when pdfium is missing, so installing it is seen without reopening.
                    if !matches!(found, Err(qiyas_pdf::PdfError::NotInstalled)) {
                        underlays.pages.insert((file.clone(), page), found.clone());
                    }
                    found
                }
            };
            let size = match size {
                Ok(size) => size,
                Err(e) => {
                    let problem = format!("PDF underlay {} page {page}: {e}", def.file_path);
                    if !problems.contains(&problem) {
                        problems.push(problem);
                    }
                    continue;
                }
            };
            let mut flags = 0;
            if u.is_monochrome() {
                flags |= IMAGE_MONOCHROME;
            }
            if u.fade > 0 {
                flags |= IMAGE_FADED;
            }
            let (sx, sy) = (u.x_scale * per_point, u.y_scale * per_point);
            let (sin, cos) = u.rotation.sin_cos();
            let at = [u.insertion_point.x, u.insertion_point.y];
            let place = |x: f64, y: f64| {
                let (x, y) = (x * sx, y * sy);
                [at[0] + x * cos - y * sin, at[1] + x * sin + y * cos]
            };
            let (w, h) = size;
            out.push(Placed {
                handle: u.common.handle.value(),
                file,
                picture: Picture::Pdf { page, size },
                corners: [place(0.0, h), place(w, h), place(w, 0.0), place(0.0, 0.0)],
                flags,
                pickable: false,
            });
        }
        out
    }

    /// The handle of the sheet on screen's block record.
    pub(crate) fn sheet_owner(&self) -> Option<Handle> {
        self.drawing
            .doc
            .block_records
            .get(&self.sheet)
            .map(|b| b.handle)
    }

    /// Adds the layer `name` with `color` when the drawing has none of that name.
    pub(crate) fn ensure_layer(&mut self, name: &str, color: Color) {
        let doc = &mut self.drawing.doc;
        if !doc.layers.contains(name) {
            let mut layer = Layer::new(name);
            layer.handle = doc.allocate_handle();
            layer.color = color;
            doc.layers.add(layer).expect("checked it is missing");
        }
    }

    /// Adds a page read by `PreparedPdf::read` at `insert_at`: its entities, or an underlay of it
    /// and its definition. One undo step. The handles added: the underlay's, or the entities'.
    /// Reading the PDF (slow) and adding it (quick) are apart so a caller can read without
    /// holding the session.
    pub fn apply_pdf(
        &mut self,
        prepared: PreparedPdf,
        insert_at: [f64; 2],
    ) -> Result<Vec<u64>, CoreError> {
        self.record_document_change(Source::User, "attach_pdf", true, false, |s| {
            s.apply_prepared(prepared, insert_at)
        })
    }

    /// `apply_pdf` without the undo step (the command bus records it).
    pub(crate) fn apply_prepared(
        &mut self,
        prepared: PreparedPdf,
        insert_at: [f64; 2],
    ) -> Result<Vec<u64>, CoreError> {
        let PreparedPdf {
            path,
            page,
            scale,
            content,
        } = prepared;
        match content {
            Content::Entities(entities) => {
                let handles = self.import_entities(entities, insert_at)?;
                let app = self.object_app(crate::recognition::SOURCE_APP)?;
                for handle in &handles {
                    let source = crate::recognition::Evidence {
                        handle: None,
                        path: Some(path.display().to_string()),
                        page: Some(page),
                        detail: format!("PDF vector; {scale} drawing units per point"),
                    };
                    let e = self
                        .drawing
                        .doc
                        .get_entity_mut(Handle::new(*handle))
                        .expect("imported entity");
                    crate::xdata::write_json(
                        e.common_mut(),
                        crate::recognition::SOURCE_APP,
                        app,
                        &source,
                    )?;
                }
                Ok(handles)
            }
            Content::Size(size) => self
                .attach_underlay(&path, page, size, insert_at, scale)
                .map(|h| vec![h]),
        }
    }

    /// Adds imported entities moved by `insert_at`, on their layers.
    fn import_entities(
        &mut self,
        entities: Vec<EntityType>,
        insert_at: [f64; 2],
    ) -> Result<Vec<u64>, CoreError> {
        let owner = self.sheet_owner();
        let mut handles = Vec::with_capacity(entities.len());
        for mut entity in entities {
            translate(&mut entity, insert_at);
            let layer = entity.common().layer.clone();
            self.ensure_layer(&layer, layer_color(&layer));
            if let Some(owner) = owner {
                entity.common_mut().owner_handle = owner;
            }
            let handle = self
                .drawing
                .doc
                .add_entity(entity)
                .map_err(|e| CoreError::InvalidCommand(e.to_string()))?;
            handles.push(handle.value());
        }
        Ok(handles)
    }

    /// Adds an underlay of page `page` of `path` (`size` points) and its definition.
    fn attach_underlay(
        &mut self,
        path: &Path,
        page: u32,
        size: (f64, f64),
        insert_at: [f64; 2],
        scale: f64,
    ) -> Result<u64, CoreError> {
        self.underlays
            .get_mut()
            .pages
            .insert((path.to_path_buf(), page), Ok(size));
        let stored = stored_link(&self.drawing.path, path);
        let page_name = page.to_string();
        self.ensure_layer(UNDERLAY_LAYER, Color::Index(8));
        let owner = self.sheet_owner();
        let per_point = units_per_point(self.drawing.units);
        let doc = &mut self.drawing.doc;
        // PDFUNDERLAY and PDFDEFINITION need their classes for the DWG writer.
        doc.classes.update_defaults();
        let dictionary = named_dictionary(doc, DEFINITIONS);
        let existing = match doc.objects.get(&dictionary) {
            Some(ObjectType::Dictionary(d)) => {
                d.entries
                    .iter()
                    .find_map(|(_, h)| match doc.objects.get(h) {
                        Some(ObjectType::UnderlayDefinition(def))
                            if def.file_path == stored && def.page_name == page_name =>
                        {
                            Some(*h)
                        }
                        _ => None,
                    })
            }
            _ => None,
        };
        let definition = match existing {
            Some(h) => h,
            None => {
                let stem = path
                    .file_stem()
                    .map_or("PDF".to_string(), |s| s.to_string_lossy().to_string());
                let mut def = UnderlayDefinition::pdf(&stored, &page_name);
                def.handle = doc.allocate_handle();
                def.owner_handle = dictionary;
                def.name = unique_entry(doc, dictionary, &format!("{stem} - {page}"));
                let h = def.handle;
                if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&dictionary) {
                    d.add_entry(def.name.clone(), h);
                }
                doc.objects.insert(h, ObjectType::UnderlayDefinition(def));
                h
            }
        };
        let mut underlay = Underlay::pdf();
        underlay.insertion_point = Vector3::new(insert_at[0], insert_at[1], 0.0);
        underlay.set_scale(scale / per_point);
        underlay.definition_handle = definition;
        underlay.common.layer = UNDERLAY_LAYER.to_string();
        if let Some(owner) = owner {
            underlay.common.owner_handle = owner;
        }
        let handle = doc
            .add_entity(EntityType::Underlay(underlay))
            .map_err(|e| CoreError::InvalidCommand(e.to_string()))?;
        if let Some(ObjectType::UnderlayDefinition(def)) = doc.objects.get_mut(&definition) {
            def.reactors.push(handle);
        }
        Ok(handle.value())
    }

    /// Scales an underlay or image about `p1` (`Command::Calibrate`).
    pub(crate) fn rescale(
        &mut self,
        handle: u64,
        p1: [f64; 2],
        p2: [f64; 2],
        real_distance: f64,
    ) -> Result<(), CoreError> {
        let measured = (p2[0] - p1[0]).hypot(p2[1] - p1[1]);
        if !(measured > 0.0 && measured.is_finite()) {
            return Err(CoreError::InvalidCommand(
                "Pick two different points to calibrate".to_string(),
            ));
        }
        if !(real_distance > 0.0 && real_distance.is_finite()) {
            return Err(CoreError::InvalidCommand(
                "The real distance must be a number above 0".to_string(),
            ));
        }
        let f = real_distance / measured;
        let about = |p: &mut Vector3| {
            p.x = p1[0] + (p.x - p1[0]) * f;
            p.y = p1[1] + (p.y - p1[1]) * f;
        };
        match self.drawing.doc.get_entity_mut(Handle::new(handle)) {
            Some(EntityType::Underlay(u)) => {
                about(&mut u.insertion_point);
                u.x_scale *= f;
                u.y_scale *= f;
                u.z_scale *= f;
            }
            Some(EntityType::RasterImage(image)) => {
                about(&mut image.insertion_point);
                for v in [&mut image.u_vector, &mut image.v_vector] {
                    v.x *= f;
                    v.y *= f;
                    v.z *= f;
                }
            }
            Some(_) => {
                return Err(CoreError::InvalidCommand(
                    "Only an underlay or an image can be calibrated".to_string(),
                ))
            }
            None => return Err(CoreError::NoSuchEntity(handle)),
        }
        Ok(())
    }
}

/// The named-object dictionary entry `name` (`ACAD_PDFDEFINITIONS`, `ACAD_IMAGE_DICT`), a
/// dictionary, added when missing.
pub(crate) fn named_dictionary(doc: &mut opencadcodec::CadDocument, name: &str) -> Handle {
    let root = doc.header.named_objects_dict_handle;
    if let Some(ObjectType::Dictionary(d)) = doc.objects.get(&root) {
        if let Some(h) = d.get(name) {
            if matches!(doc.objects.get(&h), Some(ObjectType::Dictionary(_))) {
                return h;
            }
        }
    }
    let mut dictionary = Dictionary::new();
    dictionary.handle = doc.allocate_handle();
    dictionary.owner = root;
    let h = dictionary.handle;
    doc.objects.insert(h, ObjectType::Dictionary(dictionary));
    if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&root) {
        d.entries.retain(|(k, _)| k != name);
        d.add_entry(name, h);
    }
    h
}

/// `name`, or `name (2)`, `name (3)`... when the dictionary has it already.
pub(crate) fn unique_entry(
    doc: &opencadcodec::CadDocument,
    dictionary: Handle,
    name: &str,
) -> String {
    let taken = |n: &str| match doc.objects.get(&dictionary) {
        Some(ObjectType::Dictionary(d)) => d.get(n).is_some(),
        _ => false,
    };
    if !taken(name) {
        return name.to_string();
    }
    (2..)
        .map(|i| format!("{name} ({i})"))
        .find(|n| !taken(n))
        .expect("some number is free")
}

/// Whether `p` is inside the convex quad `q` (either winding).
fn inside(q: &[[f64; 2]; 4], p: [f64; 2]) -> bool {
    let side =
        |a: [f64; 2], b: [f64; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let sides: Vec<f64> = (0..4).map(|i| side(q[i], q[(i + 1) % 4])).collect();
    sides.iter().all(|s| *s >= 0.0) || sides.iter().all(|s| *s <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_colours_come_from_the_name() {
        assert_eq!(
            layer_color("PDF-FF8000"),
            Color::Rgb {
                r: 255,
                g: 128,
                b: 0
            }
        );
        assert_eq!(
            layer_color("PDF-FILL-0000FF"),
            Color::Rgb { r: 0, g: 0, b: 255 }
        );
        assert_eq!(layer_color("PDF-TEXT"), Color::Index(7));
    }

    #[test]
    fn one_point_is_paper_size_in_drawing_units() {
        assert!((units_per_point(Units::Millimetres) - 25.4 / 72.0).abs() < 1e-12);
        assert!((units_per_point(Units::Metres) - 0.0254 / 72.0).abs() < 1e-12);
        assert!((units_per_point(Units::Inches) - 1.0 / 72.0).abs() < 1e-12);
    }

    #[test]
    fn inside_a_turned_quad() {
        let q = [[0.0, 1.0], [1.0, 2.0], [2.0, 1.0], [1.0, 0.0]];
        assert!(inside(&q, [1.0, 1.0]));
        assert!(!inside(&q, [0.1, 0.1]));
    }
}
