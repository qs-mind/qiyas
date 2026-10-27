//! Page count, page sizes and thumbnails.

use std::path::Path;

use image::RgbaImage;
use pdfium_render::prelude::{PdfPage, PdfPageRenderRotation};

use crate::raster::render_on_worker;
use crate::{open, page_error, page_index, run, Result};

/// How many pages the PDF at `path` has.
pub fn page_count(path: &Path) -> Result<u32> {
    let path = path.to_path_buf();
    run(move |pdfium| {
        let doc = open(pdfium, &path)?;
        Ok(doc.pages().len().max(0) as u32)
    })
}

/// Width and height in points of page `page` (from 1) as shown, without reading the page itself.
pub fn page_size(path: &Path, page: u32) -> Result<(f64, f64)> {
    let path = path.to_path_buf();
    run(move |pdfium| {
        let doc = open(pdfium, &path)?;
        let index = page_index(&doc, page)?;
        let size = doc.pages().page_size(index).map_err(page_error)?;
        Ok((
            f64::from(size.width().value),
            f64::from(size.height().value),
        ))
    })
}

/// How far page `page` (from 1) is stored turned (`/Rotate`): 0, 90, 180 or 270 degrees
/// clockwise. The page is shown upright whatever it says; this is for telling the engineer.
pub fn page_rotation(path: &Path, page: u32) -> Result<u16> {
    let path = path.to_path_buf();
    run(move |pdfium| {
        let doc = open(pdfium, &path)?;
        let index = page_index(&doc, page)?;
        let page = doc.pages().get(index).map_err(page_error)?;
        Ok(match page.rotation().map_err(page_error)? {
            PdfPageRenderRotation::None => 0,
            PdfPageRenderRotation::Degrees90 => 90,
            PdfPageRenderRotation::Degrees180 => 180,
            PdfPageRenderRotation::Degrees270 => 270,
        })
    })
}

/// The pages `pages` (from 1) drawn to fit in `px` x `px` pixels, keeping their shape, in the
/// order asked. Only those pages are read. A page number past the end is `NoSuchPage`.
pub fn thumbnails(path: &Path, pages: &[u32], px: u32) -> Result<Vec<RgbaImage>> {
    let path = path.to_path_buf();
    let pages = pages.to_vec();
    let px = px.clamp(16, 1024);
    run(move |pdfium| {
        let doc = open(pdfium, &path)?;
        let mut out = Vec::with_capacity(pages.len());
        for page in pages {
            let index = page_index(&doc, page)?;
            let size = doc.pages().page_size(index).map_err(page_error)?;
            let (w, h) = (size.width().value.max(1.0), size.height().value.max(1.0));
            let fit = px as f32 / w.max(h);
            let (pw, ph) = (
                ((w * fit).round() as u32).max(1),
                ((h * fit).round() as u32).max(1),
            );
            let page = doc.pages().get(index).map_err(page_error)?;
            out.push(render_on_worker(&page, pw, ph, &mut |_| {})?);
        }
        Ok(out)
    })
}

/// From the page's own coordinates (PDF user space) to the page as shown: its crop box moved to
/// the origin and turned by its `/Rotate` (clockwise), in points, y up. `[a, b, c, d, e, f]` as in
/// PDF: `x' = a x + c y + e`, `y' = b x + d y + f`.
pub(crate) fn display_matrix(page: &PdfPage) -> Result<[f64; 6]> {
    let boundaries = page.boundaries();
    let rect = boundaries
        .crop()
        .or_else(|_| boundaries.media())
        .map_err(page_error)?
        .bounds;
    let (l, b) = (f64::from(rect.left().value), f64::from(rect.bottom().value));
    let (w, h) = (
        f64::from(rect.width().value),
        f64::from(rect.height().value),
    );
    // x0, y0: the point relative to the crop box's bottom-left corner.
    let m = match page.rotation().map_err(page_error)? {
        PdfPageRenderRotation::None => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        // Shown turned a quarter clockwise: x' = y0, y' = w - x0.
        PdfPageRenderRotation::Degrees90 => [0.0, -1.0, 1.0, 0.0, 0.0, w],
        PdfPageRenderRotation::Degrees180 => [-1.0, 0.0, 0.0, -1.0, w, h],
        // x' = h - y0, y' = x0.
        PdfPageRenderRotation::Degrees270 => [0.0, 1.0, -1.0, 0.0, h, 0.0],
    };
    // Then move the crop box to the origin first: x0 = x - l, y0 = y - b.
    Ok([
        m[0],
        m[1],
        m[2],
        m[3],
        m[4] - m[0] * l - m[2] * b,
        m[5] - m[1] * l - m[3] * b,
    ])
}

/// Plain page text, read on the pdfium worker. Scanned pages return an empty string, never
/// invented OCR. Pages retain their one-based source numbers and are read in requested order.
pub fn page_text(path: &Path, pages: &[u32]) -> Result<Vec<(u32, String)>> {
    let path = path.to_path_buf();
    let pages = pages.to_vec();
    if pages.is_empty() || pages.len() > 100 {
        return Err(crate::PdfError::Page("Request 1..100 page numbers".into()));
    }
    run(move |pdfium| {
        let doc = open(pdfium, &path)?;
        let mut result = Vec::new();
        let mut bytes = 0;
        for number in pages {
            let page = doc
                .pages()
                .get(page_index(&doc, number)?)
                .map_err(page_error)?;
            let text = page.text().map_err(page_error)?.all();
            bytes += text.len();
            if bytes > 2 * 1024 * 1024 {
                return Err(crate::PdfError::Page(
                    "Text exceeds 2 MiB; request fewer pages".into(),
                ));
            }
            result.push((number, text));
        }
        Ok(result)
    })
}
