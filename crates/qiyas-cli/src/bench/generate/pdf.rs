//! The vector variant uses Qiyas's plotter. The scan contains only raster pixels, including
//! the real drawing glyphs, so a PDF text extractor cannot accidentally read hidden text.

use std::fs;
use std::path::Path;

use qiyas_core::Session;
use qiyas_plot::{Area, Orientation, Paper, PlotJob, Scale};
use serde_json::{json, Value};

use super::{error, Result};

pub(super) fn write(session: &Session, path: &Path, scan: bool) -> Result<Value> {
    if !scan {
        let job = PlotJob {
            sheet: session.sheet.clone(),
            area: Area::Extents,
            paper: Paper {
                name: "A3".into(),
                w_mm: 297.0,
                h_mm: 420.0,
            },
            orientation: Orientation::Landscape,
            scale: Scale::Fit,
            centre: true,
            style: None,
            lineweights: false,
            output: None,
        };
        let output = qiyas_plot::plot_pdf_to(session, &job, path).map_err(error)?;
        if !output.skipped.is_empty() {
            return Err(format!("PDF omitted drawing content: {:?}", output.skipped));
        }
        return Ok(
            json!({"kind":"vector","exporter":"qiyas-plot","paper":"A3 landscape","scale":"fit","pages":1}),
        );
    }
    let bounds = session.batch().bounds;
    let px_per_unit = 1000.0 / (bounds[1][0] - bounds[0][0]).max(1e-9);
    let (batch, pages) = session.picture_batch(px_per_unit);
    let png = qiyas_ai::render_png_with_text(&batch, &pages, &[], bounds, 1000);
    let pixmap = tiny_skia::Pixmap::decode_png(&png).map_err(error)?;
    let grey: Vec<u8> = pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000) as u8
        })
        .collect();
    let data = run_length(&grey);
    let (w, h) = (pixmap.width(), pixmap.height());
    let stream = format!("q {w} 0 0 {h} 0 0 cm /Scan Do Q\n");
    let objects=[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Resources << /XObject << /Scan 5 0 R >> >> /Contents 4 0 R >>").into_bytes(),
        pdf_stream("",stream.as_bytes()),
        pdf_stream(&format!("/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /RunLengthDecode"),&data),
    ];
    let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![0];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend(object);
        pdf.extend(b"\nendobj\n");
    }
    let start = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in offsets.iter().skip(1) {
        pdf.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    fs::write(path, pdf).map_err(error)?;
    Ok(
        json!({"kind":"scan","exporter":"Qiyas rasterizer","width_px":w,"height_px":h,"pages":1,"drawing_bounds":bounds,"text_layer":false}),
    )
}

fn pdf_stream(dict: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", bytes.len()).into_bytes();
    out.extend(bytes);
    out.extend(b"\nendstream");
    out
}

/// PDF RunLengthDecode packets: 0..127 literal bytes, 129..255 repeated byte, 128 end.
fn run_length(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let mut repeated = 1;
        while repeated < 128 && at + repeated < bytes.len() && bytes[at + repeated] == bytes[at] {
            repeated += 1;
        }
        if repeated >= 3 {
            out.push((257 - repeated) as u8);
            out.push(bytes[at]);
            at += repeated;
        } else {
            let begin = at;
            at += repeated;
            while at < bytes.len() && at - begin < 128 {
                if at + 2 < bytes.len() && bytes[at] == bytes[at + 1] && bytes[at] == bytes[at + 2]
                {
                    break;
                }
                at += 1;
            }
            out.push((at - begin - 1) as u8);
            out.extend_from_slice(&bytes[begin..at]);
        }
    }
    out.push(128);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raster_packets_round_trip_runs_and_literals() {
        let mut pixels = vec![255; 257];
        pixels.extend(0..=255);
        pixels.extend([17; 128]);
        pixels.extend([42, 42, 43, 44, 44]);
        let encoded = run_length(&pixels);
        let mut decoded = Vec::new();
        let mut i = 0;
        loop {
            let packet = encoded[i];
            i += 1;
            match packet {
                128 => break,
                0..=127 => {
                    let n = usize::from(packet) + 1;
                    decoded.extend_from_slice(&encoded[i..i + n]);
                    i += n;
                }
                _ => {
                    decoded.extend(std::iter::repeat_n(encoded[i], 257 - usize::from(packet)));
                    i += 1;
                }
            }
        }
        assert_eq!(decoded, pixels);
        assert_eq!(i, encoded.len());
    }
    #[test]
    fn scan_has_only_an_image_and_vector_uses_the_plotter() {
        let dir = tempfile::tempdir().unwrap();
        let project =
            super::super::build(&dir.path().join("drawing.dxf"), "office", 55, false).unwrap();
        for scan in [false, true] {
            let path = dir
                .path()
                .join(if scan { "scan.pdf" } else { "vector.pdf" });
            let metadata = write(&project.session, &path, scan).unwrap();
            assert_eq!(metadata["kind"], if scan { "scan" } else { "vector" });
            let bytes = fs::read(path).unwrap();
            assert!(bytes.starts_with(b"%PDF-"));
            if scan {
                let text = String::from_utf8_lossy(&bytes);
                assert!(text.contains("/Subtype /Image"));
                assert!(!text.contains("/Font"));
            }
        }
    }
}
