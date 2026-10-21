//! Explicitly approved, budgeted Mizan evaluations. Tests inject a scripted provider and never
//! load app settings or ask the OS keychain for credentials.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::Utc;
use clap::Args;
use qiyas_ai::provider::{BoxFuture, Sink};
use qiyas_ai::{
    Agent, AgentEvent, AgentSettings, AiError, Autonomy, Provider, TurnRequest, TurnResult, Usage,
};
use qiyas_core::{Boq, BoqItem, Command, ConflictKind, Session};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Args)]
pub struct EvalArgs {
    /// A development case id; holdout cases are never opened by this command.
    #[arg(long)]
    pub case: String,
    #[arg(long, default_value = ".local/benchmark")]
    pub root: PathBuf,
    /// Use the saved app provider, model and effort (also the default).
    #[arg(long)]
    pub provider_from_settings: bool,
    /// Kareem's explicit approval for this one billable run.
    #[arg(long)]
    pub approve_live_run: bool,
    /// Output tokens per provider request, including reasoning tokens.
    #[arg(long, default_value_t = 4096, value_parser = clap::value_parser!(u32).range(1..=64000))]
    pub max_tokens: u32,
    #[arg(long, default_value_t = 16, value_parser = clap::value_parser!(u32).range(1..=100))]
    pub max_requests: u32,
    /// An optional additional ceiling for this run's estimated dollar cost.
    #[arg(long)]
    pub max_cost_usd: Option<f64>,
}

/// Rates must be supplied for the actual service and model. Never guess a model's price.
#[derive(Clone, Deserialize)]
struct Prices {
    /// OpenAI-compatible services can charge different rates for the same model name.
    #[serde(default)]
    endpoint: Option<String>,
    input_per_million: f64,
    output_per_million: f64,
    cache_read_per_million: f64,
    cache_write_per_million: f64,
}

impl Prices {
    fn valid(&self) -> bool {
        [
            self.input_per_million,
            self.output_per_million,
            self.cache_read_per_million,
            self.cache_write_per_million,
        ]
        .iter()
        .all(|n| n.is_finite() && *n >= 0.0)
    }

    fn cost(&self, usage: Usage) -> f64 {
        (usage.input_tokens as f64 * self.input_per_million
            + usage.output_tokens as f64 * self.output_per_million
            + usage.cache_read_input_tokens as f64 * self.cache_read_per_million
            + usage.cache_creation_input_tokens as f64 * self.cache_write_per_million)
            / 1_000_000.0
    }

    fn upper_bound(&self, request: &TurnRequest) -> Result<f64, String> {
        // UTF-8 bytes overestimate ordinary text tokens. Eight bytes of allowance per input
        // byte also cover protocol framing and image tokenisation conservatively. Each image
        // adds a further 200,000-token reserve, even if its compressed payload is very small.
        let messages = serde_json::to_vec(&request.messages).map_err(|e| e.to_string())?;
        let mut bytes = request.system.len() + messages.len() + 4096;
        for tool in &request.tools {
            bytes += tool.name.len()
                + tool.description.len()
                + serde_json::to_vec(&tool.input_schema)
                    .map_err(|e| e.to_string())?
                    .len();
        }
        let mut images = 0;
        for message in &request.messages {
            for block in &message.content {
                match block {
                    qiyas_ai::ContentBlock::Image { .. } => images += 1,
                    qiyas_ai::ContentBlock::ToolResult { content, .. } => {
                        images += content
                            .iter()
                            .filter(|part| matches!(part, qiyas_ai::ResultPart::Png(_)))
                            .count();
                    }
                    _ => {}
                }
            }
        }
        let input_tokens = (bytes as f64 * 8.0) + images as f64 * 200_000.0;
        let input_rate = self
            .input_per_million
            .max(self.cache_write_per_million)
            .max(self.cache_read_per_million);
        let cost = (input_tokens * input_rate
            + request.max_tokens as f64 * self.output_per_million)
            / 1_000_000.0;
        if !cost.is_finite() {
            return Err("The request's cost estimate is not finite".into());
        }
        Ok(cost)
    }
}

#[derive(Deserialize)]
struct Budget {
    #[serde(default = "default_daily")]
    daily_usd: f64,
    #[serde(default)]
    prices: BTreeMap<String, Prices>,
}

fn default_daily() -> f64 {
    5.0
}

impl Budget {
    fn load(dir: &Path) -> Result<Self, String> {
        validate_path(dir)?;
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let path = dir.join("budget.json");
        validate_path(&path)?;
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => file
                .write_all(b"{\"daily_usd\":5}\n")
                .map_err(|e| e.to_string())?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
        let budget: Self = read_json(&path)?;
        if !budget.daily_usd.is_finite()
            || budget.daily_usd < 0.0
            || budget.prices.values().any(|p| !p.valid())
        {
            return Err("Budget and prices must be finite, non-negative dollar amounts".into());
        }
        Ok(budget)
    }
}

#[derive(Serialize, Deserialize)]
struct Charge {
    id: String,
    day: String,
    reserved_usd: f64,
    charged_usd: f64,
    status: String,
    usage: Usage,
}

/// An atomic lock covers reading, reserving and replacing the ledger across CLI processes.
/// A crashed process leaves its pending reservation charged; recovery is deliberately manual.
struct LedgerLock(PathBuf);

impl LedgerLock {
    fn acquire(dir: &Path) -> Result<Self, String> {
        let path = dir.join("budget.lock");
        validate_path(&path)?;
        OpenOptions::new().write(true).create_new(true).open(&path)
            .map_err(|_| "Budget ledger is locked; wait for the other eval, or inspect a stale lock and pending charges after a crash".to_string())?;
        Ok(Self(path))
    }
}

impl Drop for LedgerLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn ledger(dir: &Path) -> Result<Vec<Charge>, String> {
    let path = dir.join("budget-ledger.json");
    validate_path(&path)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let rows: Vec<Charge> = read_json(&path)?;
    if rows.iter().any(|c| {
        !c.charged_usd.is_finite()
            || c.charged_usd < 0.0
            || !c.reserved_usd.is_finite()
            || c.reserved_usd < 0.0
    }) {
        return Err("Budget ledger contains invalid charges".into());
    }
    Ok(rows)
}

fn store_ledger(dir: &Path, rows: &[Charge]) -> Result<(), String> {
    let temp = dir.join("budget-ledger.pending");
    validate_path(&temp)?;
    validate_path(&dir.join("budget-ledger.json"))?;
    let mut file = fs::File::create(&temp).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(rows).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    fs::rename(temp, dir.join("budget-ledger.json")).map_err(|e| e.to_string())
}

fn reserve(dir: &Path, id: &str, day: &str, cost: f64, daily: f64) -> Result<(), String> {
    let _lock = LedgerLock::acquire(dir)?;
    let mut rows = ledger(dir)?;
    let spent: f64 = rows
        .iter()
        .filter(|c| c.day == day)
        .map(|c| c.charged_usd)
        .sum();
    if spent + cost > daily {
        return Err(format!("Daily budget refused the request: ${spent:.6} charged/reserved + ${cost:.6} reserve exceeds ${daily:.2}"));
    }
    rows.push(Charge {
        id: id.into(),
        day: day.into(),
        reserved_usd: cost,
        charged_usd: cost,
        status: "pending".into(),
        usage: Usage::default(),
    });
    store_ledger(dir, &rows)
}

fn settle(dir: &Path, id: &str, usage: Usage, actual: Option<f64>) -> Result<(), String> {
    let _lock = LedgerLock::acquire(dir)?;
    let mut rows = ledger(dir)?;
    let row = rows
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or("Missing budget reservation")?;
    row.usage = usage;
    match actual {
        Some(cost) if cost <= row.reserved_usd => {
            row.charged_usd = cost;
            row.status = "estimated_from_usage".into();
        }
        Some(cost) => {
            row.charged_usd = cost;
            row.status = "estimate_exceeded_reservation".into();
        }
        None => row.status = "usage_unknown_reserve_retained".into(),
    }
    store_ledger(dir, &rows)
}

#[derive(Default, Serialize)]
struct RunUsage {
    requests: u32,
    usage: Usage,
    estimated_usd: f64,
    unknown_usage_requests: u32,
}

struct Metered {
    inner: Box<dyn Provider>,
    dir: PathBuf,
    run: String,
    prices: Prices,
    daily: f64,
    max_cost: f64,
    max_tokens: u32,
    max_requests: u32,
    totals: Arc<Mutex<RunUsage>>,
}

impl Provider for Metered {
    fn stream_turn<'a>(
        &'a self,
        mut req: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            req.max_tokens = req.max_tokens.min(self.max_tokens);
            let upper = self.prices.upper_bound(&req).map_err(AiError::Stream)?;
            let id = {
                let mut totals = self
                    .totals
                    .lock()
                    .map_err(|_| AiError::Stream("Usage lock failed".into()))?;
                if totals.requests >= self.max_requests {
                    return Err(AiError::Stream("Evaluation request limit reached".into()));
                }
                if totals.estimated_usd + upper > self.max_cost {
                    return Err(AiError::Stream(
                        "This run's cost limit would be exceeded".into(),
                    ));
                }
                let id = format!("{}-{}", self.run, totals.requests + 1);
                reserve(
                    &self.dir,
                    &id,
                    &Utc::now().format("%Y-%m-%d").to_string(),
                    upper,
                    self.daily,
                )
                .map_err(AiError::Stream)?;
                totals.requests += 1;
                totals.estimated_usd += upper;
                id
            };
            let answer = self.inner.stream_turn(req, sink).await;
            let usage = answer.as_ref().map(|r| r.usage).unwrap_or_default();
            let known = usage.input_tokens
                + usage.output_tokens
                + usage.cache_creation_input_tokens
                + usage.cache_read_input_tokens
                > 0;
            let actual = known.then(|| self.prices.cost(usage));
            settle(&self.dir, &id, usage, actual).map_err(AiError::Stream)?;
            {
                let mut totals = self
                    .totals
                    .lock()
                    .map_err(|_| AiError::Stream("Usage lock failed".into()))?;
                totals.usage.add(usage);
                totals.estimated_usd += actual.unwrap_or(upper) - upper;
                if !known {
                    totals.unknown_usage_requests += 1;
                }
            }
            if actual.is_some_and(|cost| cost > upper) {
                return Err(AiError::Stream("Provider usage exceeded its conservative reservation; inspect pricing before another run".into()));
            }
            // A missing usage report can be a broken stream. Retain its whole reservation and
            // stop: continuing would repeatedly spend without a measured cost.
            if answer.is_ok() && !known {
                return Err(AiError::Stream(
                    "Provider omitted token usage; reservation retained, evaluation stopped".into(),
                ));
            }
            answer
        })
    }
}

struct SavedProvider {
    name: String,
    model: String,
    base_url: Option<String>,
    effort: String,
    language: String,
}

#[derive(Deserialize)]
struct InputBoq {
    items: Vec<InputBoqItem>,
}

#[derive(Deserialize)]
struct InputBoqItem {
    id: String,
    description: String,
    unit: String,
    quantity: f64,
}

fn visible_inputs(directory: &Path, output: &Path, session: &mut Session) -> Result<(), String> {
    for name in ["input-boq.json", "schedule.json"] {
        if !directory
            .join(name)
            .try_exists()
            .map_err(|e| e.to_string())?
        {
            continue;
        }
        let source = crate::bench::safe_file(directory, Path::new(name))?;
        fs::copy(source, output.join(name)).map_err(|e| e.to_string())?;
        if name == "input-boq.json" {
            let input: InputBoq = read_json(&output.join(name))?;
            let items = input
                .items
                .into_iter()
                .enumerate()
                .map(|(i, item)| {
                    if item.id.is_empty() || !item.quantity.is_finite() || item.quantity < 0.0 {
                        return Err("Invalid visible input BOQ item".to_string());
                    }
                    Ok(BoqItem {
                        code: item.id,
                        description: item.description,
                        unit: item.unit,
                        quantity: item.quantity,
                        rate: None,
                        section: String::new(),
                        tolerance: None,
                        row: (i + 1) as u32,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let boq = Boq::new(name.into(), items, 0);
            session
                .run(Command::SetBoq { boq })
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn findings(session: &Session, truth: &crate::bench::Truth) -> Vec<Value> {
    session
        .conflicts
        .iter()
        .map(|conflict| {
            let kind = match conflict.kind {
                ConflictKind::QuantityMismatch => "wrong_quantity",
                ConflictKind::MissingFromBoq => "missing",
                ConflictKind::UnitDoubt => "wrong_unit",
                ConflictKind::ScheduleVsPlan => "schedule_mismatch",
                _ => conflict.kind.name(),
            };
            // Identity mapping is part of scoring, after the agent has stopped. Neither truth
            // quantities nor these source-to-id mappings are provided to the model.
            let item_id = conflict
                .boq_code
                .clone()
                .or_else(|| {
                    session
                        .takeoff
                        .items
                        .iter()
                        .find(|m| conflict.measurements.contains(&m.id))
                        .and_then(|m| {
                            let kind = serde_json::to_value(m.kind).ok()?;
                            let measured: BTreeSet<_> = m.handles.iter().copied().collect();
                            let candidates = truth
                                .items
                                .iter()
                                .filter(|item| {
                                    let sources = item
                                        .source_objects
                                        .iter()
                                        .map(|s| {
                                            u64::from_str_radix(s.trim_start_matches("0x"), 16)
                                        })
                                        .collect::<Result<BTreeSet<_>, _>>();
                                    item.kind == kind.as_str().unwrap_or("")
                                        && item.unit == m.unit
                                        && !measured.is_empty()
                                        && sources.is_ok_and(|sources| sources == measured)
                                })
                                .collect::<Vec<_>>();
                            if candidates.len() == 1 {
                                Some(candidates[0].id.clone())
                            } else {
                                None
                            }
                        })
                })
                .or_else(|| conflict.schedule.clone())
                .unwrap_or_else(|| conflict.id.clone());
            json!({ "item_id": item_id, "kind": kind })
        })
        .collect()
}

fn save_outputs(
    session: &mut Session,
    output: &Path,
    drawing: &Path,
    elapsed_ms: u128,
    truth: &crate::bench::Truth,
) -> Result<PathBuf, String> {
    session
        .save_as_headless(drawing)
        .map_err(|e| e.to_string())?;
    qiyas_boq::export_takeoff(
        &output.join("takeoff.xlsx"),
        &session.takeoff.items,
        &session.takeoff.totals(),
        "Mizan benchmark evaluation",
    )
    .map_err(|e| e.to_string())?;
    let submission_path = output.join("takeoff.json");
    let mut items = serde_json::to_value(&session.takeoff.items).map_err(|e| e.to_string())?;
    for (item, measurement) in items
        .as_array_mut()
        .ok_or("Invalid take-off serialization")?
        .iter_mut()
        .zip(&session.takeoff.items)
    {
        if let Some(height) = measurement.height {
            item["height_m"] = json!(height * session.drawing.units.to_metres());
        }
    }
    write_json(
        &submission_path,
        &json!({ "items": items, "elapsed_ms": elapsed_ms, "findings": findings(session, truth), "drawing": drawing }),
    )?;
    write_json(&output.join("conflicts.json"), &session.conflicts)?;
    Ok(submission_path)
}

impl SavedProvider {
    fn from_value(value: &Value) -> Result<Self, String> {
        let name = value["provider"]
            .as_str()
            .ok_or("No saved provider; choose one in Qiyas Settings")?;
        if !matches!(name, "anthropic" | "openai_compatible") {
            return Err("Unknown saved provider".into());
        }
        let profile = &value["providers"][name];
        let model = profile["model"]
            .as_str()
            .filter(|m| !m.trim().is_empty())
            .ok_or("Choose a model in Qiyas Settings")?;
        let base_url = profile["base_url"].as_str().map(str::to_string);
        if name == "openai_compatible"
            && base_url.as_deref().is_none_or(|s| {
                !s.starts_with("https://")
                    && !s.starts_with("http://localhost:")
                    && !s.starts_with("http://127.0.0.1:")
            })
        {
            return Err("Saved provider requires an HTTPS address, or a local HTTP server".into());
        }
        if base_url
            .as_deref()
            .is_some_and(|s| s.contains(['@', '?', '#']))
        {
            return Err("Provider addresses containing credentials, queries or fragments cannot be used for evaluation".into());
        }
        Ok(Self {
            name: name.into(),
            model: model.into(),
            base_url,
            effort: value["effort"].as_str().unwrap_or("high").into(),
            language: value["language"].as_str().unwrap_or("en").into(),
        })
    }

    fn settings(&self, max_tokens: u32) -> AgentSettings {
        AgentSettings {
            provider: self.name.clone(),
            model: self.model.clone(),
            effort: self.effort.clone(),
            max_tokens,
            language: qiyas_ai::prompt::Lang::from_code(&self.language),
            autonomy: Autonomy::Autonomous,
            ..AgentSettings::default()
        }
    }

    fn live_provider(&self) -> Result<Box<dyn Provider>, String> {
        keyring::Entry::store_status()
            .as_ref()
            .map_err(|_| "OS keychain is unavailable".to_string())?;
        let entry = keyring_core::Entry::new("qiyas", &self.name)
            .map_err(|_| "Could not open the provider's OS keychain entry".to_string())?;
        let key =
            match entry.get_password() {
                Ok(key) => Some(key),
                Err(keyring_core::Error::NoEntry) if self.name == "openai_compatible" => None,
                Err(_) => return Err(
                    "No readable provider key in the OS keychain; configure it in Qiyas Settings"
                        .into(),
                ),
            };
        if self.name == "anthropic" {
            Ok(Box::new(
                qiyas_ai::anthropic::Anthropic::new(
                    key.ok_or("No Anthropic key in the OS keychain")?,
                )
                .with_strict_budget(),
            ))
        } else {
            Ok(Box::new(
                qiyas_ai::openai::OpenAiCompatible::new(
                    self.base_url
                        .as_deref()
                        .ok_or("No saved provider address")?,
                    key,
                )
                .with_strict_budget(),
            ))
        }
    }
}

fn saved_provider() -> Result<SavedProvider, String> {
    // Tauri's app_config_dir is the platform config directory plus the bundle identifier.
    // Only this explicitly approved runtime path reads the file; it never rewrites settings.
    let home = || {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("No home directory".to_string())
    };
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .ok_or("No app config directory")?
    } else if cfg!(target_os = "macos") {
        home()?.join("Library/Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(|| home().map(|p| p.join(".config")))?
    };
    let value: Value = read_json(&base.join("com.qsmind.qiyas/settings.json"))?;
    SavedProvider::from_value(&value)
}

struct PreparedCase {
    session: Session,
    drawing: PathBuf,
    note: String,
}

/// Preparation sees the drawing alone. Ground truth never supplies PDF calibration or units.
fn prepare_case(source: &Path, output: &Path) -> Result<PreparedCase, String> {
    validate_path(source)?;
    validate_path(output)?;
    let extension = source
        .extension()
        .and_then(|s| s.to_str())
        .ok_or("Case drawing has no extension")?;
    if extension.eq_ignore_ascii_case("pdf") {
        let pdf = output.join("source.pdf");
        fs::copy(source, &pdf).map_err(|e| e.to_string())?;
        let drawing = output.join("drawing.dxf");
        let mut session = Session::new_blank(&drawing, 6);
        let scale = 0.0254 / 72.0;
        session
            .attach_pdf(&pdf, 1, [0.0, 0.0], scale, qiyas_core::AttachMode::Underlay)
            .map_err(|e| {
                format!(
                    "PDF preparation failed before provider settings or credentials were read: {e}"
                )
            })?;
        let imported =
            session.attach_pdf(&pdf, 1, [0.0, 0.0], scale, qiyas_core::AttachMode::Import);
        let vectors = match imported {
            Ok(count) => format!("{count} vector/text entities were imported at the same paper scale."),
            Err(_) => "Vector import was unavailable; the PDF underlay is available for visual inspection.".into(),
        };
        let note = format!("This is a PDF case. Only page 1 is prepared; do not claim coverage of other pages. The underlay and any imported entities are UNCALIBRATED: their current metres describe paper size (1 PDF point = 0.0254/72 metres), not real project lengths. {vectors} Inspect the page with qiyas_capture, calibrate from visible dimensions or a legible scale bar before measuring, and apply the same calibration to any imported vectors. If no reliable scale is visible, report that quantities cannot be established. Do not infer scale from filenames or benchmark metadata.");
        Ok(PreparedCase {
            session,
            drawing,
            note,
        })
    } else if extension.eq_ignore_ascii_case("dwg") || extension.eq_ignore_ascii_case("dxf") {
        let drawing = output.join(format!("drawing.{}", extension.to_ascii_lowercase()));
        fs::copy(source, &drawing).map_err(|e| e.to_string())?;
        let session = Session::open_standalone(&drawing).map_err(|e| e.to_string())?;
        Ok(PreparedCase {
            session,
            drawing,
            note: String::new(),
        })
    } else {
        Err("Evaluation supports DWG, DXF and PDF drawings; no provider settings or credentials were read".into())
    }
}

pub fn run(args: &EvalArgs) -> Result<Value, String> {
    if !args.approve_live_run {
        return Err("Live evaluation needs Kareem's OK for this case; after approval use --approve-live-run (no settings or keychain were read)".into());
    }
    if args.max_cost_usd.is_some_and(|n| !n.is_finite() || n < 0.0) {
        return Err("--max-cost-usd must be finite and non-negative".into());
    }
    crate::bench::refuse_holdout_path(&args.root)?;
    let (truth, case_dir) = crate::bench::load_case(&args.root, &args.case)?;
    let root = args.root.canonicalize().map_err(|e| e.to_string())?;
    let run_id = format!(
        "eval-{}-{}",
        Utc::now().format("%Y%m%dT%H%M%S%.9fZ"),
        std::process::id()
    );
    validate_path(&root.join("runs"))?;
    fs::create_dir_all(root.join("runs")).map_err(|e| e.to_string())?;
    let output = crate::bench::safe_file(&root, Path::new("runs"))?.join(&run_id);
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    let PreparedCase {
        mut session,
        drawing,
        note,
    } = prepare_case(&case_dir.join(&truth.drawing), &output)?;
    visible_inputs(&case_dir, &output, &mut session)?;
    let session = Arc::new(Mutex::new(Some(session)));
    let local = root
        .parent()
        .ok_or("Benchmark root needs a parent directory")?;
    validate_path(&local.join("autopilot"))?;
    fs::create_dir_all(local.join("autopilot")).map_err(|e| e.to_string())?;
    let autopilot = crate::bench::safe_file(local, Path::new("autopilot"))?;
    let budget = Budget::load(&autopilot)?;
    let saved = saved_provider()?;
    let identity = format!("{}:{}", saved.name, saved.model);
    let prices = budget.prices.get(&identity).cloned().ok_or_else(|| format!("No prices for {identity}; set its input/output/cache-read/cache-write rates per million tokens in {} before a billable run", autopilot.join("budget.json").display()))?;
    if saved.name == "openai_compatible"
        && prices.endpoint.as_deref().map(|s| s.trim_end_matches('/'))
            != saved.base_url.as_deref().map(|s| s.trim_end_matches('/'))
    {
        return Err("The configured price endpoint must match the saved provider address".into());
    }
    let provider = saved.live_provider()?;
    let totals = Arc::new(Mutex::new(RunUsage::default()));
    let metered = Metered {
        inner: provider,
        dir: autopilot.clone(),
        run: run_id.clone(),
        prices,
        daily: budget.daily_usd,
        max_cost: args.max_cost_usd.unwrap_or(budget.daily_usd),
        max_tokens: args.max_tokens,
        max_requests: args.max_requests,
        totals: totals.clone(),
    };
    let started = Instant::now();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let status = runtime.block_on(run_agent(
        Box::new(metered),
        session.clone(),
        saved.settings(args.max_tokens),
        &note,
    ));
    let elapsed_ms = started.elapsed().as_millis();
    let submission_path = {
        let mut guard = session.lock().map_err(|_| "Session lock failed")?;
        let session = guard.as_mut().ok_or("Mizan closed the benchmark drawing")?;
        save_outputs(session, &output, &drawing, elapsed_ms, &truth)?
    };
    let summary = totals.lock().map_err(|_| "Usage lock failed")?;
    append_costs(
        &autopilot, &run_id, &args.case, &identity, &summary, &status,
    )?;
    write_json(&output.join("usage.json"), &*summary)?;
    drop(summary);
    let score = crate::bench::score_submission(&root, &args.case, &submission_path)?;
    let report = json!({ "run": run_id, "case": args.case, "status": status, "provider": identity, "elapsed_ms": elapsed_ms, "drawing": drawing, "takeoff": submission_path, "score": score, "preparation": note, "traceability_basis": "Original case geometry when available; the resulting drawing is also retained for review", "autonomy": "autonomous, explicitly approved for this evaluation", "max_tokens_per_request": args.max_tokens, "max_requests": args.max_requests });
    write_json(&output.join("report.json"), &report)?;
    if status != "done" {
        return Err(format!(
            "Live evaluation {status}; partial output was scored in {}",
            output.join("report.json").display()
        ));
    }
    Ok(report)
}

async fn run_agent(
    provider: Box<dyn Provider>,
    session: Arc<Mutex<Option<Session>>>,
    settings: AgentSettings,
    preparation: &str,
) -> String {
    let mut agent = Agent::new(provider, session, settings);
    let (sender, mut receiver) = mpsc::channel(64);
    let events = tokio::spawn(async move {
        let mut status = "incomplete".to_string();
        while let Some(event) = receiver.recv().await {
            match event {
                AgentEvent::Done { .. } => status = "done".into(),
                AgentEvent::Stopped { reason } => status = format!("stopped: {reason}"),
                AgentEvent::AwaitingApproval { .. } => status = "awaiting_approval".into(),
                AgentEvent::Refused { .. } => status = "refused".into(),
                _ => {}
            }
        }
        status
    });
    let prompt = format!("Complete a quantity take-off of this drawing. Inspect its layers, blocks, rooms and geometry. Add measurements with meaningful descriptions, physical units and source entity handles for every measurable item you can justify. If a BOQ is loaded, link measurements to its item codes, compare it against the drawing and preserve the resulting conflicts. Check drawing and schedule consistency and preserve those conflicts too. Report any limitations. This benchmark run is approved; work autonomously within the drawing. The harness saves and exports your result after you finish.\n\n{preparation}");
    let result = agent
        .run_turn(prompt, sender, CancellationToken::new())
        .await;
    let status = events
        .await
        .unwrap_or_else(|_| "event collection failed".into());
    match result {
        Ok(()) => status,
        Err(e) => format!("failed: {e}"),
    }
}

fn append_costs(
    dir: &Path,
    run: &str,
    case: &str,
    provider: &str,
    totals: &RunUsage,
    status: &str,
) -> Result<(), String> {
    let _lock = LedgerLock::acquire(dir)?;
    let path = dir.join("costs.md");
    validate_path(&path)?;
    let fresh = !path.exists();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    if fresh {
        writeln!(file, "# Mizan benchmark costs\n\nEstimates use the configured per-model rates. Unknown usage retains the full request reservation. Dates are UTC.\n\n| UTC | Run | Case | Provider/model | Requests | Input | Output | Cache read | Cache write | Reasoning (part of output) | Estimated USD | Unknown usage | Status |\n|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---|").map_err(|e| e.to_string())?;
    }
    let cell = |s: &str| s.replace(['\n', '\r', '|'], " ");
    let u = totals.usage;
    writeln!(
        file,
        "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.6} | {} | {} |",
        Utc::now().to_rfc3339(),
        cell(run),
        cell(case),
        cell(provider),
        totals.requests,
        u.input_tokens,
        u.output_tokens,
        u.cache_read_input_tokens,
        u.cache_creation_input_tokens,
        u.reasoning_tokens,
        totals.estimated_usd,
        totals.unknown_usage_requests,
        cell(status)
    )
    .map_err(|e| e.to_string())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    validate_path(path)?;
    serde_json::from_slice(
        &fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?,
    )
    .map_err(|_| format!("Invalid JSON in {}", path.display()))
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    validate_path(path)?;
    fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Check missing output paths before creation as well as existing files before opening them.
fn validate_path(path: &Path) -> Result<(), String> {
    crate::bench::refuse_holdout_path(path)?;
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err("Evaluation paths cannot be symlinks".into())
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use qiyas_ai::{ContentBlock, StopReason};
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Scripted {
        calls: Arc<AtomicU32>,
        usage: Usage,
        command: Option<Value>,
    }

    impl Provider for Scripted {
        fn stream_turn<'a>(
            &'a self,
            req: TurnRequest,
            _sink: Sink<'a>,
        ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
            Box::pin(async move {
                assert!(req.max_tokens <= 4096);
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                let (content, stop_reason) = if self.command.is_some() && call == 0 {
                    (
                        vec![ContentBlock::ToolUse {
                            id: "measure-1".into(),
                            name: "qiyas_execute".into(),
                            input: json!({"op":"run", "args":{"command":self.command}}),
                        }],
                        StopReason::ToolUse,
                    )
                } else {
                    (
                        vec![ContentBlock::Text {
                            text: "Measured the available geometry.".into(),
                        }],
                        StopReason::EndTurn,
                    )
                };
                Ok(TurnResult {
                    content,
                    stop_reason,
                    usage: self.usage,
                })
            })
        }
    }

    fn prices() -> Prices {
        Prices {
            endpoint: None,
            input_per_million: 1.0,
            output_per_million: 2.0,
            cache_read_per_million: 0.1,
            cache_write_per_million: 1.25,
        }
    }

    #[test]
    fn no_approval_stops_before_settings_keychain_or_files() {
        let args = EvalArgs {
            case: "anything".into(),
            root: PathBuf::from("does-not-exist"),
            provider_from_settings: true,
            approve_live_run: false,
            max_tokens: 4096,
            max_requests: 16,
            max_cost_usd: None,
        };
        assert!(run(&args).unwrap_err().contains("Kareem's OK"));
    }

    #[test]
    fn approved_eval_still_refuses_a_holdout_root_before_io() {
        let args = EvalArgs {
            case: "anything".into(),
            root: PathBuf::from("does-not-exist/holdout"),
            provider_from_settings: true,
            approve_live_run: true,
            max_tokens: 4096,
            max_requests: 16,
            max_cost_usd: None,
        };
        assert!(run(&args).unwrap_err().contains("holdout is sealed"));
    }

    #[cfg(unix)]
    #[test]
    fn budget_and_output_symlinks_are_refused_before_opening_targets() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("budget.json");
        std::os::unix::fs::symlink(dir.path().join("never-open/holdout"), &link).unwrap();
        assert!(Budget::load(dir.path()).is_err());
        assert!(validate_path(&dir.path().join("budget.json/new-output")).is_err());
    }

    #[test]
    fn concurrent_reservations_and_unknown_usage_cannot_free_budget() {
        let dir = tempfile::tempdir().unwrap();
        reserve(dir.path(), "a", "2026-10-21", 4.0, 5.0).unwrap();
        assert!(reserve(dir.path(), "b", "2026-10-21", 2.0, 5.0).is_err());
        settle(dir.path(), "a", Usage::default(), None).unwrap();
        assert!(reserve(dir.path(), "c", "2026-10-21", 2.0, 5.0).is_err());
        settle(
            dir.path(),
            "a",
            Usage {
                input_tokens: 100,
                ..Usage::default()
            },
            Some(0.1),
        )
        .unwrap();
        reserve(dir.path(), "d", "2026-10-21", 2.0, 5.0).unwrap();
        reserve(dir.path(), "e", "2026-10-22", 5.0, 5.0).unwrap();
        let lock = LedgerLock::acquire(dir.path()).unwrap();
        assert!(reserve(dir.path(), "f", "2026-10-23", 1.0, 5.0).is_err());
        drop(lock);
    }

    #[tokio::test]
    async fn real_agent_uses_scripted_provider_and_accounts_for_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicU32::new(0));
        let totals = Arc::new(Mutex::new(RunUsage::default()));
        let case_dir = dir.path().join("cases/test");
        let output = dir.path().join("runs/scripted");
        fs::create_dir_all(&case_dir).unwrap();
        fs::create_dir_all(&output).unwrap();
        let original = case_dir.join("drawing.dxf");
        let drawing = output.join("drawing.dxf");
        let mut opened = Session::new_blank(&original, 6);
        let outcome = opened.run_input(&json!({"type":"draw", "kind":"polyline", "closed":true, "points":[[0,0],[3,0],[3,2],[0,2]]}), qiyas_core::Source::User).unwrap().outcome;
        let qiyas_core::Outcome::Added { handles } = outcome else {
            panic!("Expected rectangle");
        };
        let handle = handles[0];
        opened.save_as_headless(&original).unwrap();
        write_json(&case_dir.join("truth.json"), &json!({"schema_version":1,"id":"test","project_type":"office","drawing":"drawing.dxf","items":[{"id":"floor","description":"Test floor","kind":"area","unit":"m2","quantity":6.0,"source_objects":[format!("{handle:X}")],"basis":"geometric","tolerance_class":"geometry"}]})).unwrap();
        fs::copy(&original, &drawing).unwrap();
        let provider = Metered {
            inner: Box::new(Scripted {
                calls: calls.clone(),
                usage: Usage {
                    input_tokens: 100,
                    output_tokens: 20,
                    reasoning_tokens: 5,
                    ..Usage::default()
                },
                command: Some(json!({"type":"measure_entity","handle":handle})),
            }),
            dir: dir.path().into(),
            run: "test".into(),
            prices: prices(),
            daily: 5.0,
            max_cost: 5.0,
            max_tokens: 4096,
            max_requests: 2,
            totals: totals.clone(),
        };
        let session = Arc::new(Mutex::new(Some(
            Session::open_standalone(&drawing).unwrap(),
        )));
        let status = run_agent(
            Box::new(provider),
            session.clone(),
            AgentSettings {
                autonomy: Autonomy::Autonomous,
                ..AgentSettings::default()
            },
            "",
        )
        .await;
        assert_eq!(status, "done");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let mut session = session.lock().unwrap();
        let measurement = &session.as_ref().unwrap().takeoff.items[0];
        assert_eq!(measurement.shown, 6.0);
        assert_eq!(measurement.source, "ai");
        let (truth, _) = crate::bench::load_case(dir.path(), "test").unwrap();
        let submission =
            save_outputs(session.as_mut().unwrap(), &output, &drawing, 1, &truth).unwrap();
        let score = crate::bench::score_submission(dir.path(), "test", &submission).unwrap();
        assert_eq!(score["items"][0]["passed"], true);
        assert_eq!(score["items"][0]["traceable"], true);
        assert!(output.join("takeoff.xlsx").exists());
        assert!(output.join("conflicts.json").exists());
        let totals = totals.lock().unwrap();
        assert_eq!(totals.usage.output_tokens, 40);
        assert!((totals.estimated_usd - 0.00028).abs() < 1e-9);
        assert_eq!(
            ledger(dir.path()).unwrap()[0].status,
            "estimated_from_usage"
        );
    }

    #[tokio::test]
    async fn daily_budget_refuses_before_scripted_provider_is_called() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicU32::new(0));
        let provider = Metered {
            inner: Box::new(Scripted {
                calls: calls.clone(),
                usage: Usage::default(),
                command: None,
            }),
            dir: dir.path().into(),
            run: "test".into(),
            prices: prices(),
            daily: 0.0,
            max_cost: 5.0,
            max_tokens: 4096,
            max_requests: 1,
            totals: Arc::new(Mutex::new(RunUsage::default())),
        };
        let session = Arc::new(Mutex::new(Some(Session::new_blank(
            &dir.path().join("drawing.dxf"),
            6,
        ))));
        let status = run_agent(Box::new(provider), session, AgentSettings::default(), "").await;
        assert!(status.contains("Daily budget refused"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn saved_provider_parser_uses_only_the_selected_profile() {
        let parsed = SavedProvider::from_value(&json!({ "provider":"openai_compatible", "providers": { "anthropic": { "model":"unused" }, "openai_compatible": { "model":"priced-model", "base_url":"http://localhost:1234/v1" } }, "effort":"medium", "language":"ar" })).unwrap();
        assert_eq!(parsed.model, "priced-model");
        assert_eq!(parsed.settings(4096).language, qiyas_ai::prompt::Lang::Ar);
    }

    #[test]
    fn pdf_preparation_keeps_the_visible_page_and_requires_calibration() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("plan.pdf");
        let output = dir.path().join("output");
        fs::create_dir(&output).unwrap();
        let mut session = Session::new_blank(&dir.path().join("plan.dxf"), 6);
        session.run_input(&json!({"type":"draw","kind":"polyline","closed":true,"points":[[0,0],[3,0],[3,2],[0,2]]}), qiyas_core::Source::User).unwrap();
        let job = qiyas_plot::PlotJob {
            sheet: session.sheet.clone(),
            area: qiyas_plot::Area::Extents,
            paper: qiyas_plot::Paper {
                name: "A4".into(),
                w_mm: 210.0,
                h_mm: 297.0,
            },
            orientation: qiyas_plot::Orientation::Portrait,
            scale: qiyas_plot::Scale::Fit,
            centre: true,
            style: None,
            lineweights: false,
            output: None,
        };
        qiyas_plot::plot_pdf_to(&session, &job, &source).unwrap();
        let prepared = match prepare_case(&source, &output) {
            Ok(prepared) => prepared,
            Err(error) if error.contains("PDF support is not installed") => {
                eprintln!("PDF preparation unavailable: bundled PDFium is not installed; live PDF evaluation will refuse before reading settings or credentials");
                return;
            }
            Err(error) => panic!("{error}"),
        };
        assert_eq!(prepared.drawing, output.join("drawing.dxf"));
        assert!(output.join("source.pdf").exists());
        assert!(prepared.note.contains("UNCALIBRATED"));
        assert!(prepared.note.contains("Only page 1"));
        assert!(
            !prepared.session.batch().images.is_empty(),
            "The PDF page must be visible to capture"
        );
        assert!(prepared.session.takeoff.items.is_empty());
    }

    #[test]
    fn malformed_pdf_is_refused_during_preparation() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("broken.pdf");
        let output = dir.path().join("output");
        fs::create_dir(&output).unwrap();
        fs::write(&source, b"not a PDF").unwrap();
        let result = prepare_case(&source, &output);
        assert!(
            matches!(result, Err(error) if error.contains("before provider settings or credentials were read"))
        );
    }

    #[test]
    fn conflict_identity_requires_kind_exact_sources_and_a_unique_match() {
        let mut session = Session::new_blank(Path::new("unused.dxf"), 6);
        session
            .run_input(
                &json!({"type":"measure_area","points":[[0,0],[3,0],[3,2],[0,2]]}),
                qiyas_core::Source::Ai,
            )
            .unwrap();
        let measured = &mut session.takeoff.items[0];
        measured.kind = qiyas_core::Kind::Volume;
        measured.unit = "m3".into();
        measured.handles = vec![0xabc];
        session.conflicts.push(serde_json::from_value(json!({"id":"unmapped-conflict","kind":"missing_from_boq","boq_code":null,"measurements":[measured.id],"schedule":null,"sheets":[],"expected":null,"found":6.0,"unit":"m3","reason":"Missing measured work"})).unwrap());
        let mut truth: crate::bench::Truth = serde_json::from_value(json!({"schema_version":1,"id":"case","project_type":"office","drawing":"drawing.dxf","items":[
            {"id":"area","description":"Area","kind":"area","unit":"m2","quantity":6.0,"source_objects":["ABC"],"basis":"geometric","tolerance_class":"geometry"},
            {"id":"volume","description":"Volume","kind":"volume","unit":"m3","quantity":6.0,"source_objects":["ABC"],"basis":"geometric","tolerance_class":"geometry"}
        ]})).unwrap();
        assert_eq!(findings(&session, &truth)[0]["item_id"], "volume");
        truth.items[1].source_objects.push("DEF".into());
        assert_eq!(
            findings(&session, &truth)[0]["item_id"],
            "unmapped-conflict"
        );
        truth.items[1].source_objects.pop();
        let mut duplicate = truth.items[1].clone();
        duplicate.id = "other-volume".into();
        truth.items.push(duplicate);
        assert_eq!(
            findings(&session, &truth)[0]["item_id"],
            "unmapped-conflict"
        );
    }
}
