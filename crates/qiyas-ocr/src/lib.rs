//! Local Arabic and English text reading. No runtime or model downloads.
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use oar_ocr::oarocr::{OAROCRBuilder, OAROCR};

const DETECTOR: &str = "pp-ocrv5_mobile_det.onnx";
const RECOGNIZER: &str = "arabic_pp-ocrv5_mobile_rec.onnx";
const DICTIONARY: &str = "ppocrv5_arabic_dict.txt";
static RESOURCE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
static ENGINE: OnceLock<Mutex<Option<OAROCR>>> = OnceLock::new();
static ORT: OnceLock<()> = OnceLock::new();

#[derive(Debug, Clone, PartialEq)]
pub enum OcrError {
    NotInstalled,
    Library(String),
}

impl std::fmt::Display for OcrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInstalled => write!(f, "Text reading is not installed: run scripts/ocr.sh"),
            Self::Library(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for OcrError {}

/// Image pixels, origin top left; corners retain the detector's order.
#[derive(Debug, Clone)]
pub struct TextBox {
    pub text: String,
    pub confidence: f32,
    pub quad: [[f64; 2]; 4],
}

/// Resource folder to use before default locations, until the engine is loaded.
pub fn set_resource_dir(dir: impl Into<PathBuf>) {
    *RESOURCE_DIR.lock().unwrap_or_else(PoisonError::into_inner) = Some(dir.into());
}

fn library_name() -> String {
    format!(
        "{}onnxruntime{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )
}

fn candidates() -> Vec<PathBuf> {
    if let Some(dir) = std::env::var_os("QIYAS_OCR") {
        return vec![dir.into()];
    }
    let mut dirs = Vec::new();
    if let Some(dir) = RESOURCE_DIR
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
    {
        dirs.push(dir);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        dirs.push(dir.join("resources/ocr"));
    }
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../qiyas-desktop/resources/ocr"));
    dirs
}

fn resource_dir() -> Option<PathBuf> {
    candidates().into_iter().find(|dir| {
        [library_name().as_str(), DETECTOR, RECOGNIZER, DICTIONARY]
            .iter()
            .all(|file| dir.join(file).is_file())
    })
}

/// Whether the runtime, detector, recognizer and dictionary are installed. Does not load them.
pub fn available() -> bool {
    resource_dir().is_some()
}

pub fn read(image: &image::RgbImage) -> Result<Vec<TextBox>, OcrError> {
    let mut engine = ENGINE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if engine.is_none() {
        let dir = resource_dir().ok_or(OcrError::NotInstalled)?;
        // The engine mutex serializes initialization as well as inference. Failed installs
        // may be repaired and retried; a successfully loaded ORT is never initialized twice.
        if ORT.get().is_none() {
            let _committed = ort::init_from(dir.join(library_name()))
                .map_err(|e| OcrError::Library(e.to_string()))?
                .commit();
            let _ = ORT.set(());
        }
        *engine = Some(
            OAROCRBuilder::new(
                dir.join(DETECTOR),
                dir.join(RECOGNIZER),
                dir.join(DICTIONARY),
            )
            .build()
            .map_err(|e| OcrError::Library(e.to_string()))?,
        );
    }
    let results = engine
        .as_ref()
        .expect("initialized engine")
        .predict(vec![image.clone()])
        .map_err(|e| OcrError::Library(e.to_string()))?;
    let mut texts = Vec::new();
    for result in results {
        for region in result.text_regions {
            let Some((text, confidence)) = region.text_with_confidence() else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            let points = &region
                .dt_poly
                .as_ref()
                .unwrap_or(&region.bounding_box)
                .points;
            let points: &[_; 4] = points.as_slice().try_into().map_err(|_| {
                OcrError::Library("OCR detector returned a non-quadrilateral box".into())
            })?;
            texts.push(TextBox {
                text: arabic_logical_text(text),
                confidence,
                quad: points.map(|p| [f64::from(p.x), f64::from(p.y)]),
            });
        }
    }
    Ok(texts)
}

// The Arabic model uses Paddle's reversed label order. oar-ocr-core 0.10.2 implements
// pred_reverse in processors/decode.rs:150, but both decoder constructors leave its private
// reverse flag false. Apply that operation here, preserving Latin/number runs as units.
fn arabic_logical_text(text: &str) -> String {
    if !text.chars().any(|c| matches!(c, '\u{0600}'..='\u{06ff}' | '\u{0750}'..='\u{077f}' | '\u{08a0}'..='\u{08ff}')) {
        return text.to_owned();
    }
    let mut parts = Vec::new();
    let mut latin = String::new();
    for c in text.chars() {
        // Digits run left to right in Arabic text too, Arabic-Indic ones included (and their
        // decimal and thousands marks), so a number keeps its order.
        if c.is_ascii_alphanumeric()
            || " :*./%+-".contains(c)
            || matches!(c, '\u{0660}'..='\u{066c}' | '\u{06f0}'..='\u{06f9}')
        {
            latin.push(c);
        } else {
            if !latin.is_empty() {
                parts.push(std::mem::take(&mut latin));
            }
            parts.push(c.to_string());
        }
    }
    if !latin.is_empty() {
        parts.push(latin);
    }
    parts.reverse();
    parts.concat()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
