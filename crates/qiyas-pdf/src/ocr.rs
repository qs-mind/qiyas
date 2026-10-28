//! OCR runs after rendering has left the pdfium worker; no pdfium objects cross threads.
use std::path::Path;

use crate::{page_size, page_text, render_page, PdfError, Result};

/// Text in displayed PDF points, bottom-left origin, with an axis-aligned bounding box.
#[derive(Debug, Clone)]
pub struct PageText {
    pub text: String,
    pub confidence: f32,
    pub bbox: [[f64; 2]; 2],
}

pub fn has_text_layer(path: &Path, page: u32) -> Result<bool> {
    Ok(page_text(path, &[page])?
        .iter()
        .any(|(_, text)| !text.trim().is_empty()))
}

pub fn ocr_page(path: &Path, page: u32) -> Result<Vec<PageText>> {
    let (width, height) = page_size(path, page)?;
    let dpi = 300.0_f64.min(6000.0 * 72.0 / width.max(height));
    let rgba = render_page(path, page, dpi, |_| {})?;
    let image = image::DynamicImage::ImageRgba8(rgba).into_rgb8();
    let sx = width / f64::from(image.width());
    let sy = height / f64::from(image.height());
    // page_size and render_page already include /Rotate and the crop box. Applying the
    // display matrix again would rotate twice. Actual pixel sizes account for rounding.
    qiyas_ocr::read(&image)
        .map_err(|e| PdfError::Page(e.to_string()))
        .map(|texts| {
            texts
                .into_iter()
                .map(|t| {
                    let mut bbox = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
                    for [x, y] in t.quad {
                        let p = [x * sx, height - y * sy];
                        for axis in 0..2 {
                            bbox[0][axis] = bbox[0][axis].min(p[axis]);
                            bbox[1][axis] = bbox[1][axis].max(p[axis]);
                        }
                    }
                    PageText {
                        text: t.text,
                        confidence: t.confidence,
                        bbox,
                    }
                })
                .collect()
        })
}
