//! The tools that need the window: what the user sees now (`view_screen`), what they point at
//! (`get_selection`), and selecting entities for them (`select_entities`). The window does its
//! part through `DrawingHost` while the session's lock is let go (it draws the screen from the
//! session); the session then reads what came back.

use std::sync::{Mutex, PoisonError};

use qiyas_core::{Session, Target};
use serde_json::{json, Value};

use crate::context::{describe, TurnContext};
use crate::execute::{marks_legend, text_note};
use crate::host::DrawingHost;
use crate::image::mark_screen;
use crate::prompt::Lang;
use crate::ToolOutput;

/// The tools run through the window.
pub const WINDOW_TOOLS: [&str; 3] = ["view_screen", "get_selection", "select_entities"];
/// Longest side of a picture of the screen, in pixels.
const SCREEN_SIDE: u32 = 1600;
/// Most entities marked on a picture of the screen.
const SCREEN_MARKS: usize = 40;
/// Most entities `select_entities` selects at once.
const MAX_SELECT: usize = 10_000;
const NO_DRAWING: &str = "No drawing is open. Ask the user to open one.";

/// Whether `name` is one of `WINDOW_TOOLS`.
pub fn is_window_tool(name: &str) -> bool {
    WINDOW_TOOLS.contains(&name)
}

/// Runs the window tool `name` with `input`: the window first, without the session's lock, then
/// the session. `lang` is the language of a description.
pub fn run_window(
    host: &dyn DrawingHost,
    session: &Mutex<Option<Session>>,
    name: &str,
    input: &Value,
    lang: Lang,
) -> Result<ToolOutput, String> {
    match name {
        "view_screen" => view_screen(host, session, input),
        "get_selection" => {
            let pointed = host.pointed()?;
            let guard = session.lock().unwrap_or_else(PoisonError::into_inner);
            let s = guard.as_ref().ok_or(NO_DRAWING)?;
            Ok(ToolOutput::Json(json!({
                "selection": pointed.selection,
                "region": pointed.region,
                "view": pointed.view,
                "description": describe(s, &TurnContext { picture: false, ..pointed.clone() }, lang),
            })))
        }
        "select_entities" => select_entities(host, session, input),
        other => Err(format!("{other} is not a window tool")),
    }
}

fn view_screen(
    host: &dyn DrawingHost,
    session: &Mutex<Option<Session>>,
    input: &Value,
) -> Result<ToolOutput, String> {
    let marks = input.get("marks").and_then(Value::as_bool).unwrap_or(true);
    // What the user selected is marked first.
    let first: Vec<u64> = host
        .pointed()
        .map(|p| tops(&p.selection))
        .unwrap_or_default();
    let screen = host.capture_screen(SCREEN_SIDE)?;
    let guard = session.lock().unwrap_or_else(PoisonError::into_inner);
    let s = guard.as_ref().ok_or(NO_DRAWING)?;
    let batch = s.batch();
    let max = if marks { SCREEN_MARKS } else { 0 };
    let drawn = mark_screen(&screen.png, &batch, screen.bounds, &first, max)?;
    let [lo, hi] = screen.bounds;
    let mut caption = format!(
        "What the user sees now, as the window draws it (its own colours and background; the \
         selection highlighted): sheet {}, bounds [[{:.3}, {:.3}], [{:.3}, {:.3}]] in drawing \
         units ({}). It shows only the view on screen: use view_image to look at another part \
         or closer.",
        s.sheet,
        lo[0],
        lo[1],
        hi[0],
        hi[1],
        s.info().units,
    );
    caption.push_str(&text_note(drawn.text));
    caption.push_str(&marks_legend(s, &drawn.marks));
    Ok(ToolOutput::Image {
        png: drawn.png,
        caption,
    })
}

fn select_entities(
    host: &dyn DrawingHost,
    session: &Mutex<Option<Session>>,
    input: &Value,
) -> Result<ToolOutput, String> {
    let handles: Vec<u64> = input
        .get("handles")
        .and_then(Value::as_array)
        .ok_or("handles is required: a list of entity handles")?
        .iter()
        .map(|h| h.as_u64().ok_or("Each handle is a whole number"))
        .collect::<Result<_, _>>()?;
    if handles.len() > MAX_SELECT {
        return Err(format!("At most {MAX_SELECT} entities at once"));
    }
    let zoom = input.get("zoom").and_then(Value::as_bool).unwrap_or(false);
    // Only what the sheet on screen draws at its top can be selected.
    let (ok, refused) = {
        let guard = session.lock().unwrap_or_else(PoisonError::into_inner);
        let s = guard.as_ref().ok_or(NO_DRAWING)?;
        let batch = s.batch();
        let drawn: std::collections::HashSet<u64> = batch.ranges.iter().map(|r| r.top).collect();
        let (mut ok, mut refused) = (Vec::new(), Vec::new());
        for h in handles {
            if drawn.contains(&h) {
                if !ok.contains(&h) {
                    ok.push(h);
                }
            } else {
                refused.push(h);
            }
        }
        (ok, refused)
    };
    host.select(&ok, zoom)?;
    let mut answer = json!({
        "selected": ok.len(),
        "message": format!("Selected {} entities in the window", ok.len()),
    });
    if !refused.is_empty() {
        answer["not_selected"] = json!(refused);
        answer["why_not"] =
            json!("Not drawn at the top of the sheet on screen (gone, inside a block, or on another sheet)");
    }
    Ok(ToolOutput::Json(answer))
}

/// The entities at the top of the sheet `selection` names: a pick inside a block by its outer
/// block reference.
fn tops(selection: &[Target]) -> Vec<u64> {
    let mut tops: Vec<u64> = selection
        .iter()
        .map(|t| t.chain.first().copied().unwrap_or(t.handle))
        .collect();
    tops.dedup();
    tops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Screen;
    use std::sync::Arc;

    /// A window showing the sample's first 20 x 10 units, with the rectangle selected.
    struct Window {
        selected: Mutex<Vec<(Vec<u64>, bool)>>,
        pointed: TurnContext,
    }

    impl DrawingHost for Window {
        fn list(&self) -> Result<Value, String> {
            Ok(json!({}))
        }
        fn new_drawing(&self, _: &str) -> Result<Value, String> {
            Ok(json!({}))
        }
        fn open(&self, _: &str, _: bool) -> Result<Value, String> {
            Ok(json!({}))
        }
        fn switch(&self, _: u32) -> Result<Value, String> {
            Ok(json!({}))
        }
        fn save(&self) -> Result<Value, String> {
            Ok(json!({}))
        }
        fn close(&self, _: Option<u32>) -> Result<Value, String> {
            Ok(json!({}))
        }
        fn capture_screen(&self, max_side: u32) -> Result<Screen, String> {
            assert_eq!(max_side, SCREEN_SIDE);
            let mut pixmap = tiny_skia::Pixmap::new(400, 200).unwrap();
            pixmap.fill(tiny_skia::Color::from_rgba8(30, 30, 30, 255));
            Ok(Screen {
                png: pixmap.encode_png().unwrap(),
                bounds: [[-1.0, -3.0], [19.0, 7.0]],
            })
        }
        fn select(&self, handles: &[u64], zoom: bool) -> Result<(), String> {
            self.selected.lock().unwrap().push((handles.to_vec(), zoom));
            Ok(())
        }
        fn pointed(&self) -> Result<TurnContext, String> {
            Ok(self.pointed.clone())
        }
    }

    fn sample() -> (tempfile::TempDir, Arc<Mutex<Option<Session>>>, u64) {
        let dir = tempfile::tempdir().unwrap();
        let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
        let s = Session::open(&path).unwrap();
        let rect = s
            .find_entities(&qiyas_core::EntityFilter {
                kind: Some("LwPolyline".into()),
                ..Default::default()
            })
            .rows[0]
            .handle;
        (dir, Arc::new(Mutex::new(Some(s))), rect)
    }

    fn window(rect: u64) -> Window {
        Window {
            selected: Mutex::new(Vec::new()),
            pointed: TurnContext {
                selection: vec![Target {
                    handle: rect,
                    chain: vec![],
                }],
                view: Some([[-1.0, -3.0], [19.0, 7.0]]),
                ..TurnContext::default()
            },
        }
    }

    #[test]
    fn the_screen_comes_back_marked_with_the_selection_first() {
        let (_dir, session, rect) = sample();
        let host = window(rect);
        let out = run_window(&host, &session, "view_screen", &json!({}), Lang::En).unwrap();
        let ToolOutput::Image { png, caption } = out else {
            panic!("a picture");
        };
        let pixmap = tiny_skia::Pixmap::decode_png(&png).unwrap();
        assert_eq!((pixmap.width(), pixmap.height()), (400, 200));
        assert!(caption.contains("What the user sees now"), "{caption}");
        assert!(
            caption.contains(&format!("#1 handle {rect} LwPolyline")),
            "{caption}"
        );
        // The window's own dark background stays: the marks are drawn over it.
        let orange = pixmap
            .pixels()
            .iter()
            .filter(|p| p.red() > 200 && p.green() < 120 && p.blue() < 60)
            .count();
        assert!(orange > 20, "{orange} badge pixels");
    }

    #[test]
    fn only_entities_drawn_on_the_sheet_are_selected() {
        let (_dir, session, rect) = sample();
        let host = window(rect);
        let out = run_window(
            &host,
            &session,
            "select_entities",
            &json!({ "handles": [rect, rect, 999_999], "zoom": true }),
            Lang::En,
        )
        .unwrap();
        let ToolOutput::Json(v) = out else { panic!() };
        assert_eq!(v["selected"], 1);
        assert_eq!(v["not_selected"], json!([999_999]));
        assert_eq!(*host.selected.lock().unwrap(), vec![(vec![rect], true)]);
        assert!(run_window(&host, &session, "select_entities", &json!({}), Lang::En).is_err());
    }

    #[test]
    fn get_selection_describes_what_the_user_points_at() {
        let (_dir, session, rect) = sample();
        let host = window(rect);
        let ToolOutput::Json(v) =
            run_window(&host, &session, "get_selection", &json!({}), Lang::En).unwrap()
        else {
            panic!()
        };
        assert_eq!(v["selection"][0]["handle"], rect);
        let text = v["description"].as_str().unwrap();
        assert!(
            text.contains(&format!("- handle {rect} LwPolyline")),
            "{text}"
        );
    }

    #[test]
    fn without_a_window_they_say_so() {
        struct NoWindow;
        impl DrawingHost for NoWindow {
            fn list(&self) -> Result<Value, String> {
                Ok(json!({}))
            }
            fn new_drawing(&self, _: &str) -> Result<Value, String> {
                Ok(json!({}))
            }
            fn open(&self, _: &str, _: bool) -> Result<Value, String> {
                Ok(json!({}))
            }
            fn switch(&self, _: u32) -> Result<Value, String> {
                Ok(json!({}))
            }
            fn save(&self) -> Result<Value, String> {
                Ok(json!({}))
            }
            fn close(&self, _: Option<u32>) -> Result<Value, String> {
                Ok(json!({}))
            }
        }
        let (_dir, session, _) = sample();
        for tool in WINDOW_TOOLS {
            let err = run_window(
                &NoWindow,
                &session,
                tool,
                &json!({ "handles": [] }),
                Lang::En,
            )
            .unwrap_err();
            assert!(
                err.contains("view_image") || err.contains("window"),
                "{tool}: {err}"
            );
        }
    }
}
