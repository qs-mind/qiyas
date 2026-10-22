//! A standalone drawing, the production MCP handler, and a confined output host.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use clap::Args;
use qiyas_ai::host::DrawingHost;
use qiyas_core::Session;
use rmcp::ServiceExt;
use serde_json::{json, Value};

#[derive(Args)]
pub struct McpArgs {
    #[arg(long)]
    pub drawing: PathBuf,
    #[arg(long)]
    pub workdir: Option<PathBuf>,
}

/// Reject links, parent traversal and non-workspace paths before reading or creating files.
pub(crate) fn confined(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let full = root.join(path);
    if !full.starts_with(root) || full.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("File operations must stay inside the workdir".into());
    }
    crate::eval::validate_path(&full)?;
    if full.components().any(|c| {
        let name = c.as_os_str().to_string_lossy().to_ascii_lowercase();
        matches!(
            name.as_str(),
            "truth.json" | "settings.json" | "recipes" | "recipe.json"
        )
    }) {
        return Err("Truth, recipes and settings are unavailable to the MCP host".into());
    }
    Ok(full)
}

struct Host {
    session: Arc<Mutex<Option<Session>>>,
    root: PathBuf,
    started: Instant,
    note: String,
    answer: Mutex<String>,
    calls: Mutex<usize>,
}

impl Host {
    fn check_paths(&self, value: &Value) -> Result<(), String> {
        self.check_paths_at(value, false)
    }

    fn check_paths_at(&self, value: &Value, record_updates: bool) -> Result<(), String> {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    // A record update's path is a JSON pointer, not a file name.
                    if (key == "path" || key.ends_with("_path"))
                        && !(record_updates && key == "path")
                        && !value.is_null()
                    {
                        let path = value
                            .as_str()
                            .ok_or("File paths must be literal strings in headless tools")?;
                        confined(&self.root, Path::new(path))?;
                    }
                    self.check_paths_at(value, key == "updates")?;
                }
            }
            Value::Array(values) => {
                for value in values {
                    self.check_paths_at(value, record_updates)?;
                }
            }
            Value::String(text) => {
                if let Ok(value @ (Value::Object(_) | Value::Array(_))) = serde_json::from_str(text)
                {
                    self.check_paths(&value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl DrawingHost for Host {
    fn before_tool(&self, name: &str, input: &Value) -> Result<(), String> {
        let path = confined(&self.root, Path::new("mcp-events.jsonl"))?;
        let mut log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        writeln!(log, "{}", json!({"tool":name,"input":input})).map_err(|e| e.to_string())?;
        *self.calls.lock().map_err(|e| e.to_string())? += 1;
        self.check_paths(input)?;
        if name == qiyas_ai::surface::EXECUTE && input["op"] == "type" {
            let args = qiyas_ai::surface::args_of(input)?;
            if let Some(line) = args["line"].as_str() {
                if let Ok(qiyas_core::cmdline::Step::Command(command)) =
                    qiyas_core::cmdline::Machine::default().step(line)
                {
                    self.check_paths(&json!(command))?;
                }
            }
        }
        Ok(())
    }
    fn list(&self) -> Result<Value, String> {
        let guard = self.session.lock().map_err(|e| e.to_string())?;
        Ok(
            json!({"drawings":[{"id":1,"path":guard.as_ref().ok_or("No drawing")?.info().path,"current":true}]}),
        )
    }
    fn new_drawing(&self, _units: &str) -> Result<Value, String> {
        Err(
            "This headless session owns one drawing; start another MCP process for a new drawing"
                .into(),
        )
    }
    fn open(&self, path: &str, _user_named: bool) -> Result<Value, String> {
        let path = confined(&self.root, Path::new(path))?;
        let guard = self.session.lock().map_err(|e| e.to_string())?;
        if guard.as_ref().is_some_and(|s| s.drawing.path == path) {
            Ok(json!({"id":1,"path":path}))
        } else {
            Err("This headless session owns one drawing; start another MCP process to open a different drawing".into())
        }
    }
    fn switch(&self, id: u32) -> Result<Value, String> {
        if id != 1 {
            return Err("Only drawing id 1 is open".into());
        }
        self.list()
    }
    fn save(&self) -> Result<Value, String> {
        self.finish("")
    }
    fn close(&self, id: Option<u32>) -> Result<Value, String> {
        if id.is_some_and(|id| id != 1) {
            return Err("Only drawing id 1 is open".into());
        }
        self.finish("")
    }
    fn finish(&self, answer: &str) -> Result<Value, String> {
        if !answer.is_empty() {
            *self.answer.lock().map_err(|e| e.to_string())? = answer.into();
        }
        let mut guard = self.session.lock().map_err(|e| e.to_string())?;
        let session = guard.as_mut().ok_or("No drawing")?;
        let drawing = confined(&self.root, &session.drawing.path)?;
        let submission = crate::eval::save_artifacts(
            session,
            &self.root,
            &drawing,
            self.started.elapsed().as_millis(),
        )?;
        let count = crate::bench::submission::from_session(session)?.items.len();
        let report = json!({"status":if count == 0 {"failed: done with 0 submitted items"} else {"done"},
            "answer":*self.answer.lock().map_err(|e|e.to_string())?,"preparation":self.note,
            "tool_calls":*self.calls.lock().map_err(|e|e.to_string())?,
            "facts":{"submitted_items":count,"measurement":session.measurement_report().map_err(|e|e.to_string())?,
                "self_check":session.takeoff_check(0,&[]).map_err(|e|e.to_string())?}});
        crate::bench::write_json(&self.root.join("end-report.json"), &report)?;
        Ok(json!({"saved":drawing,"submission":submission,"end_report":report}))
    }
}

pub fn run(args: &McpArgs) -> crate::Done {
    crate::eval::validate_path(&args.drawing)?;
    let source = args.drawing.canonicalize().map_err(|e| e.to_string())?;
    let root = args
        .workdir
        .clone()
        .unwrap_or_else(|| source.parent().unwrap().into());
    crate::eval::validate_path(&root)?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    // Copy only the explicitly supplied drawing. Never load adjacent project files.
    let local = confined(
        &root,
        Path::new(source.file_name().ok_or("Drawing needs a filename")?),
    )?;
    if source != local {
        fs::copy(&source, &local).map_err(|e| e.to_string())?;
    }
    let (session, note) = if local
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
    {
        let prepared = crate::eval::prepare_case(&local, &root)?;
        (prepared.session, prepared.note)
    } else {
        (
            Session::open_standalone(&local).map_err(|e| e.to_string())?,
            String::new(),
        )
    };
    let session = Arc::new(Mutex::new(Some(session)));
    let host = Arc::new(Host {
        session: session.clone(),
        root,
        started: Instant::now(),
        note,
        answer: Mutex::new(String::new()),
        calls: Mutex::new(0),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let result = runtime.block_on(async {
        let service = qiyas_ai::mcp::Qiyas::new(
            session,
            Some(host.clone()),
            Arc::new(|| {}),
            Arc::new(|_| {}),
        )
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| e.to_string())?;
        service.waiting().await.map_err(|e| e.to_string())?;
        Ok::<_, String>(())
    });
    // EOF, including a client that never initialized successfully, still retains partial work.
    host.finish("")?;
    let exit_path = confined(&host.root, Path::new("mcp-exit.json"))?;
    crate::bench::write_json(
        &exit_path,
        &json!({"closed":true,"error":result.as_ref().err()}),
    )?;
    result?;
    Ok(None)
}
