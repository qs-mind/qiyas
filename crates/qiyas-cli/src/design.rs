//! Headless design evidence from the production agent, with no benchmark recipes.
use super::*;
use opencadcodec::EntityType;
use qiyas_ai::{ContentBlock, DrawingHost, StreamEvent, ToolOutput};
use qiyas_core::Source;

#[derive(Args)]
pub struct DesignArgs {
    /// Design brief text, or the path of a UTF-8 file containing it.
    #[arg(long)]
    brief: String,
    /// A new or empty output folder.
    #[arg(long)]
    out: PathBuf,
    #[arg(long, value_parser = ["chatgpt_plan", "claude_code", "anthropic", "openai_compatible"])]
    provider: Option<String>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long, default_value_t = 40, value_parser = clap::value_parser!(u32).range(1..=100))]
    max_requests: u32,
    #[arg(long, value_parser = ["en", "ar"])]
    language: Option<String>,
}

fn brief(text: &str) -> Result<String, String> {
    let path = Path::new(text);
    if path.is_dir() {
        return Err("The brief path is a directory".into());
    }
    let text = if path.is_file() {
        validate_path(path)?;
        fs::read_to_string(path).map_err(|e| e.to_string())?
    } else {
        text.to_owned()
    };
    if text.trim().is_empty() {
        return Err("The design brief must not be empty".into());
    }
    Ok(text)
}

fn prepare_output(path: &Path) -> Result<PathBuf, String> {
    validate_path(path)?;
    fs::create_dir_all(path).map_err(|e| e.to_string())?;
    if fs::read_dir(path)
        .map_err(|e| e.to_string())?
        .next()
        .is_some()
    {
        return Err("Design output folder must be empty; use a new folder for each run".into());
    }
    path.canonicalize().map_err(|e| e.to_string())
}

fn selected(mut value: Value, args: &DesignArgs) -> Result<SavedProvider, String> {
    if let Some(name) = &args.provider {
        value["provider"] = json!(name);
    }
    if let Some(model) = &args.model {
        let name = value["provider"]
            .as_str()
            .ok_or("Choose a provider")?
            .to_owned();
        value["providers"][name]["model"] = json!(model);
    }
    if let Some(language) = &args.language {
        value["language"] = json!(language);
    }
    SavedProvider::from_value(&value)
}

pub fn run(args: &DesignArgs) -> Result<Value, String> {
    let brief = brief(&args.brief)?;
    let output = prepare_output(&args.out)?;
    let value = saved_settings()?;
    let saved = selected(value.clone(), args)?;
    let mut drawing = Session::new_blank(&output.join("drawing.dwg"), 6);
    apply_selected_method(saved.method.as_deref(), &mut drawing)?;
    let session = Arc::new(Mutex::new(Some(drawing)));
    let provider: Box<dyn Provider> = if saved.name == "claude_code" {
        Box::new(
            qiyas_ai::claude_code::ClaudeCode::new(
                value["claude_executable"].as_str(),
                qiyas_core::data_home::qiyas_dir()?,
                session.clone(),
            )
            .map_err(|e| e.to_string())?
            .with_request_limit(args.max_requests),
        )
    } else {
        saved.live_provider()?
    };
    let subscription = matches!(saved.name.as_str(), "chatgpt_plan" | "claude_code");
    let budget = if subscription {
        None
    } else {
        // Share eval's daily ledger, including reservations held after unknown usage.
        let autopilot = PathBuf::from(".local/autopilot");
        let budget = Budget::load(&autopilot)?;
        let identity = format!("{}:{}", saved.name, saved.model);
        let prices = budget.prices.get(&identity).cloned().ok_or_else(||
            format!("No prices for {identity}; configure .local/autopilot/budget.json before a billable run"))?;
        if saved.name == "openai_compatible"
            && prices.endpoint.as_deref().map(|s| s.trim_end_matches('/'))
                != saved.base_url.as_deref().map(|s| s.trim_end_matches('/'))
        {
            return Err(
                "The configured price endpoint must match the saved provider address".into(),
            );
        }
        Some(PaidBudget {
            dir: autopilot,
            prices,
            daily: budget.daily_usd,
        })
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(run_with_provider(Run {
        provider,
        session,
        settings: saved.settings(4096),
        brief,
        output,
        max_requests: args.max_requests,
        country: value["country"].as_str().map(str::to_owned),
        budget,
    }))
}

#[derive(Default, Serialize)]
struct Trace {
    requests: Vec<Request>,
    #[serde(skip)]
    delegated: BTreeMap<String, (usize, Usage)>,
    calls: Vec<Call>,
    status: String,
    failure: Option<qiyas_ai::provider_error::PublicError>,
    final_answer: String,
    resolved_model: Option<String>,
    /// The run used its whole request allowance: a normal stop, not a provider failure.
    #[serde(skip)]
    limit_reached: bool,
}
#[derive(Serialize)]
struct Request {
    number: usize,
    usage: Option<Usage>,
    elapsed_ms: Option<u128>,
    error: Option<String>,
}
#[derive(Serialize)]
struct Call {
    id: String,
    tool: String,
    operation: String,
    kinds: Vec<String>,
    error: Option<String>,
    completed: bool,
}

struct Recorded {
    inner: Box<dyn Provider>,
    trace: Arc<Mutex<Trace>>,
    max_requests: u32,
    steps: PathBuf,
}
impl Provider for Recorded {
    fn handles_temporary_retries(&self) -> bool {
        self.inner.handles_temporary_retries()
    }
    fn recovery_output_tokens(&self, model: &str) -> u32 {
        self.inner.recovery_output_tokens(model)
    }
    fn image_input(&self, model: &str) -> Option<bool> {
        self.inner.image_input(model)
    }
    fn set_image_input(&self, supported: Option<bool>) {
        self.inner.set_image_input(supported);
    }
    fn owns_turn(&self) -> bool {
        self.inner.owns_turn()
    }
    fn set_host(&mut self, host: Arc<dyn DrawingHost>) {
        self.inner.set_host(host);
    }
    fn begin_turn(&self, text: &str) {
        self.inner.begin_turn(text);
    }
    fn resolved_model(&self) -> Option<String> {
        self.inner.resolved_model()
    }
    fn budget_notice(&self) -> Option<String> {
        let used = self.trace.lock().ok()?.requests.len();
        Some(format!(
            "Design run budget: {used} of {} provider requests used.",
            self.max_requests
        ))
    }
    fn stream_turn<'a>(
        &'a self,
        req: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            let number = {
                let mut trace = self
                    .trace
                    .lock()
                    .map_err(|_| AiError::Stream("Trace lock failed".into()))?;
                if trace.requests.len() >= self.max_requests as usize {
                    trace.limit_reached = true;
                    return Err(AiError::Stream("Design request limit reached".into()));
                }
                let number = trace.requests.len() + 1;
                trace.requests.push(Request {
                    number,
                    usage: None,
                    elapsed_ms: None,
                    error: None,
                });
                number
            };
            step(
                &Some(self.steps.clone()),
                &json!({"event":"design_request","number":number,"model":req.model}),
            )
            .map_err(AiError::Stream)?;
            let started = Instant::now();
            let mut log_error = None;
            let result = self
                .inner
                .stream_turn(req, &mut |event| {
                    if let StreamEvent::Diagnostic { kind, text } = &event {
                        if kind == "RESPONSE INCOMPLETE" || kind == "CLAUDE RESUME" {
                            if let Err(error) = step(&Some(self.steps.clone()), &json!({"event":"provider_diagnostic","number":number,"kind":kind,"detail":text})) { log_error = Some(error); }
                        }
                        if kind == "CLAUDE REQUEST USAGE" {
                            if let Ok(value) = serde_json::from_str::<Value>(text) {
                                if let (Some(id), Ok(usage)) = (
                                    value["id"].as_str(),
                                    serde_json::from_value(value["usage"].clone()),
                                ) {
                                    if let Ok(mut trace) = self.trace.lock() {
                                        let number = trace.delegated.len() + 1;
                                        let request = trace.delegated.entry(id.to_owned()).or_insert((number, usage));
                                        request.1 = usage;
                                        if let Err(error) = step(&Some(self.steps.clone()), &json!({"event":"delegated_request_usage","number":request.0,"usage":usage})) {
                                            log_error = Some(error);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    sink(event);
                })
                .await;
            if let Some(error) = log_error {
                return Err(AiError::Stream(error));
            }
            let usage = result
                .as_ref()
                .ok()
                .map(|r| r.usage)
                .filter(|usage| usage.total_input() + usage.output_tokens > 0);
            let error = result.as_ref().err().map(ToString::to_string);
            let failure = result.as_ref().err().map(AiError::public_error);
            let stop_reason = result.as_ref().ok().map(|r| format!("{:?}", r.stop_reason));
            let elapsed_ms = started.elapsed().as_millis();
            {
                let mut trace = self
                    .trace
                    .lock()
                    .map_err(|_| AiError::Stream("Trace lock failed".into()))?;
                trace.resolved_model = self.inner.resolved_model();
                let request = &mut trace.requests[number - 1];
                request.usage = usage;
                request.error.clone_from(&error);
                request.elapsed_ms = Some(elapsed_ms);
            }
            step(&Some(self.steps.clone()), &json!({"event":"design_response","number":number,"usage":usage,"error":error,"failure":failure,"stop_reason":stop_reason,"elapsed_ms":elapsed_ms})).map_err(AiError::Stream)?;
            result
        })
    }
}

struct PaidBudget {
    dir: PathBuf,
    prices: Prices,
    daily: f64,
}

struct Run {
    provider: Box<dyn Provider>,
    session: Arc<Mutex<Option<Session>>>,
    settings: AgentSettings,
    brief: String,
    output: PathBuf,
    max_requests: u32,
    country: Option<String>,
    budget: Option<PaidBudget>,
}

async fn run_with_provider(run: Run) -> Result<Value, String> {
    let started = Instant::now();
    step(
        &Some(run.output.join("steps.jsonl")),
        &json!({"event":"design_start","provider":run.settings.provider,"model":run.settings.model,"max_requests":run.max_requests}),
    )?;
    run.session
        .lock()
        .map_err(|_| "Session lock failed")?
        .as_mut()
        .ok_or("No drawing")?
        .set_export_directory(run.output.clone());
    let trace = Arc::new(Mutex::new(Trace::default()));
    let owns_turn = run.provider.owns_turn();
    let provider = Recorded {
        inner: run.provider,
        trace: trace.clone(),
        max_requests: run.max_requests,
        steps: run.output.join("steps.jsonl"),
    };
    let cost = run
        .budget
        .as_ref()
        .map(|_| Arc::new(Mutex::new(RunUsage::default())));
    let provider: Box<dyn Provider> = match (run.budget, &cost) {
        (Some(budget), Some(totals)) => Box::new(Metered {
            inner: Box::new(provider),
            dir: budget.dir,
            run: format!(
                "design-{}-{}",
                Utc::now().format("%Y%m%dT%H%M%S%f"),
                std::process::id()
            ),
            prices: budget.prices,
            daily: budget.daily,
            max_cost: budget.daily,
            max_tokens: run.settings.max_tokens,
            max_requests: run.max_requests,
            totals: totals.clone(),
            steps: Some(run.output.join("steps.jsonl")),
        }),
        _ => Box::new(provider),
    };
    let host = Arc::new(DesignHost {
        session: run.session.clone(),
        root: run.output.clone(),
    });
    let mut agent = Agent::new(provider, run.session.clone(), run.settings.clone())
        .with_country(run.country)
        .with_host(host)
        .with_log(qiyas_ai::AgentLog::evaluation(run.output.join("agent.log")));
    let (sender, mut receiver) = mpsc::channel(64);
    let collected = trace.clone();
    let steps = Some(run.output.join("steps.jsonl"));
    let events = tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            step(
                &steps,
                &json!({"event":"agent","data":trim_log(json!(event))}),
            )?;
            let mut trace = collected.lock().map_err(|_| "Trace lock failed")?;
            match event {
                AgentEvent::ToolCall {
                    id, name, input, ..
                } => {
                    let args = qiyas_ai::surface::args_of(&input).unwrap_or(Value::Null);
                    if input["op"] == "finish" {
                        if let Some(answer) = args["answer"].as_str() {
                            trace.final_answer = answer.into();
                        }
                    }
                    trace.calls.push(Call {
                        id,
                        tool: name,
                        operation: input["op"]
                            .as_str()
                            .or(input["source"].as_str())
                            .unwrap_or("unknown")
                            .into(),
                        kinds: command_kinds(&args),
                        error: None,
                        completed: false,
                    });
                }
                AgentEvent::ToolResult {
                    id,
                    is_error,
                    summary,
                    ..
                } => {
                    if let Some(call) = trace.calls.iter_mut().rev().find(|call| call.id == id) {
                        call.completed = true;
                        call.error = is_error.then_some(summary);
                    }
                }
                AgentEvent::Done { .. } if trace.status.is_empty() => trace.status = "done".into(),
                AgentEvent::Stopped { reason } => trace.status = format!("stopped: {reason}"),
                AgentEvent::Question { .. } => trace.status = "asked_question".into(),
                AgentEvent::AwaitingApproval { .. } => trace.status = "awaiting_approval".into(),
                AgentEvent::Refused { .. } => trace.status = "refused".into(),
                _ => {}
            }
        }
        Ok::<_, String>(())
    });
    let result = agent
        .run_turn(run.brief.clone(), sender, CancellationToken::new())
        .await;
    events.await.map_err(|e| e.to_string())??;
    let elapsed_ms = started.elapsed().as_millis();
    let mut trace = trace.lock().map_err(|_| "Trace lock failed")?;
    if let Err(error) = result {
        if trace.limit_reached {
            trace.status = format!(
                "stopped: request limit reached ({} of {} requests used)",
                trace.requests.len(),
                run.max_requests
            );
        } else {
            trace.status = format!("failed: {error}");
            trace.failure = Some(error.public_error_for(&run.settings.provider));
        }
    }
    if trace.status.is_empty() {
        trace.status = "incomplete".into();
    }
    // History retains the actual final response, including non-streaming scripted providers.
    if let Some(message) = agent
        .history()
        .last()
        .filter(|m| m.role == qiyas_ai::Role::Assistant)
    {
        let answer = message
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !answer.is_empty() {
            trace.final_answer = answer;
        }
    }
    let mut guard = run.session.lock().map_err(|_| "Session lock failed")?;
    let session = guard.as_mut().ok_or("No drawing")?;
    let mut artifacts = Vec::new();
    if let Err(error) = session.save_as_headless(&run.output.join("drawing.dwg")) {
        artifacts.push(format!("DWG save: {error}"));
    }
    let drawing = drawing_metrics(session, &run.output, &mut artifacts)?;
    let mut by_operation = BTreeMap::<String, usize>::new();
    let mut by_kind = BTreeMap::<String, usize>::new();
    for call in &trace.calls {
        *by_operation
            .entry(format!("{}.{}", call.tool, call.operation))
            .or_default() += 1;
        for kind in &call.kinds {
            *by_kind.entry(kind.clone()).or_default() += 1;
        }
    }
    let failed: Vec<_> = trace
        .calls
        .iter()
        .filter(|call| call.error.is_some())
        .collect();
    let requests: Vec<Value> = if owns_turn && !trace.delegated.is_empty() {
        let mut requests: Vec<_> = trace.delegated.values().collect();
        requests.sort_by_key(|(number, _)| *number);
        requests
            .into_iter()
            .map(|(number, usage)| json!({"number":number,"usage":usage}))
            .collect()
    } else {
        trace.requests.iter().map(|r| json!(r)).collect()
    };
    let token_stats = token_stats(&requests);
    let total_elapsed_ms = started.elapsed().as_millis();
    let report = json!({
        "schema_version":1, "kind":"design_run", "provider":run.settings.provider,
        "model":run.settings.model, "resolved_model":trace.resolved_model, "language":run.settings.language, "brief":run.brief,
        "status":trace.status, "failure":trace.failure, "artifacts_complete":artifacts.is_empty(), "elapsed_ms":elapsed_ms, "total_elapsed_ms":total_elapsed_ms, "max_requests":run.max_requests,
        "requests":requests.len(), "request_usage":requests, "tokens":token_stats,
        "request_scope": if owns_turn && trace.delegated.is_empty() { "delegated turn aggregate; per-request usage unavailable" } else { "provider requests" },
        "request_limit_scope": if owns_turn { "Claude Code --max-turns" } else { "provider requests" },
        "tool_calls":trace.calls.len(), "tool_calls_by_operation":by_operation,
        "tool_calls_by_kind":by_kind, "failed_calls":failed, "calls":trace.calls,
        "drawing":drawing, "artifact_errors":artifacts, "final_answer":trace.final_answer,
        "cost":cost.as_ref().and_then(|cost| cost.lock().ok().map(|v| json!(*v))),
        "count_scope":"Stored entities on model space and layouts; definitions counted separately. Inserts are stored references, including hidden layers, not expanded instances.",
        "title_block_detection":"Qiyas title layer A-ANNO-TTLB or insert names containing TITLE or TTLB; presence is not a content-quality check."
    });
    step(
        &Some(run.output.join("steps.jsonl")),
        &json!({"event":"design_end","status":report["status"],"failure":report["failure"],"requests":report["requests"],"artifact_errors":report["artifact_errors"]}),
    )?;
    write_json(&run.output.join("report.json"), &report)?;
    fs::write(run.output.join("report.md"), markdown(&report)).map_err(|e| e.to_string())?;
    Ok(report)
}

fn command_kinds(args: &Value) -> Vec<String> {
    fn kind(command: &Value) -> Option<String> {
        command["type"]
            .as_str()
            .map(|name| match command["kind"].as_str() {
                Some(kind) => format!("{name}.{kind}"),
                None => name.into(),
            })
    }
    args["commands"]
        .as_array()
        .map(|items| items.iter().filter_map(kind).collect())
        .unwrap_or_else(|| kind(&args["command"]).into_iter().collect())
}

fn token_stats(requests: &[Value]) -> Value {
    let stats = |key: &str| {
        let values: Vec<u64> = requests
            .iter()
            .filter_map(|r| r["usage"][key].as_u64())
            .collect();
        let total: u64 = values.iter().sum();
        json!({"total":total,"max":values.iter().max(),"mean":(!values.is_empty()).then(|| total as f64 / values.len() as f64),"known_requests":values.len()})
    };
    json!({"input":stats("input_tokens"),"output":stats("output_tokens"),"cache_read_input":stats("cache_read_input_tokens"),"cache_creation_input":stats("cache_creation_input_tokens")})
}

struct DesignHost {
    session: Arc<Mutex<Option<Session>>>,
    root: PathBuf,
}
impl DrawingHost for DesignHost {
    fn before_tool(&self, _name: &str, input: &Value) -> Result<(), String> {
        fn check(root: &Path, value: &Value, record_paths: bool) -> Result<(), String> {
            match value {
                Value::Object(fields) => {
                    for (key, value) in fields {
                        if (key == "path" || key.ends_with("_path"))
                            && !(record_paths && key == "path")
                            && value.is_string()
                        {
                            crate::headless::confined(root, Path::new(value.as_str().unwrap()))?;
                        }
                        check(root, value, key == "updates" || key == "where")?;
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        check(root, value, record_paths)?;
                    }
                }
                Value::String(text) => {
                    if let Ok(value @ (Value::Object(_) | Value::Array(_))) =
                        serde_json::from_str(text)
                    {
                        check(root, &value, record_paths)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        check(&self.root, input, false)
    }
    fn list(&self) -> Result<Value, String> {
        Ok(json!({"drawings":[{"id":1,"path":self.root.join("drawing.dwg"),"active":true}]}))
    }
    fn new_drawing(&self, _units: &str) -> Result<Value, String> {
        Err("Design runs use the existing new drawing".into())
    }
    fn open(&self, _path: &str, _named: bool) -> Result<Value, String> {
        Err("Design runs use one new drawing".into())
    }
    fn switch(&self, _id: u32) -> Result<Value, String> {
        Err("Design runs use one new drawing".into())
    }
    fn close(&self, _id: Option<u32>) -> Result<Value, String> {
        Err("The design drawing stays open until the report is saved".into())
    }
    fn save(&self) -> Result<Value, String> {
        self.session
            .lock()
            .map_err(|_| "Session lock failed")?
            .as_mut()
            .ok_or("No drawing")?
            .save_as_headless(&self.root.join("drawing.dwg"))
            .map_err(|e| e.to_string())?;
        Ok(json!({"saved":true}))
    }
}

fn entity_kind(entity: &EntityType) -> String {
    // Stop Debug at the variant name without formatting large entity payloads.
    struct Name(String);
    impl std::fmt::Write for Name {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            if let Some(end) = text.find('(') {
                self.0.push_str(&text[..end]);
                Err(std::fmt::Error)
            } else {
                self.0.push_str(text);
                Ok(())
            }
        }
    }
    let mut name = Name(String::new());
    let _ = std::fmt::write(&mut name, format_args!("{entity:?}"));
    name.0
}

fn drawing_metrics(
    session: &mut Session,
    output: &Path,
    errors: &mut Vec<String>,
) -> Result<Value, String> {
    let mut kinds = BTreeMap::<String, usize>::new();
    let mut layers = BTreeMap::<String, usize>::new();
    let mut inserts = BTreeMap::<String, usize>::new();
    let mut sheets = Vec::new();
    let mut texts = 0;
    let mut dimensions = 0;
    let mut definitions = Vec::new();
    let sheet_list = session.drawing.sheets();
    for record in session.drawing.doc.block_records.iter() {
        if sheet_list.iter().any(|sheet| sheet.block == record.name)
            || record.name.to_ascii_uppercase().starts_with("*PAPER_SPACE")
        {
            continue;
        }
        let mut kinds = BTreeMap::<String, usize>::new();
        for entity in session.drawing.entities_in(&record.name) {
            *kinds.entry(entity_kind(entity)).or_default() += 1;
        }
        definitions.push(
            json!({"name":record.name,"entities_by_kind":kinds,"inserts":0,"nested_inserts":0}),
        );
    }
    for (index, sheet) in sheet_list.iter().enumerate() {
        let mut count = 0;
        let mut title = false;
        let mut sheet_texts = 0;
        let mut sheet_dimensions = 0;
        for entity in session.drawing.entities_in(&sheet.block) {
            count += 1;
            *kinds.entry(entity_kind(entity)).or_default() += 1;
            *layers.entry(entity.common().layer.clone()).or_default() += 1;
            if matches!(entity, EntityType::Text(_) | EntityType::MText(_)) {
                sheet_texts += 1;
            }
            if matches!(entity, EntityType::Dimension(_)) {
                sheet_dimensions += 1;
            }
            if let EntityType::Insert(insert) = entity {
                *inserts.entry(insert.block_name.clone()).or_default() += 1;
                let name = insert.block_name.to_ascii_uppercase();
                title |= name.contains("TITLE") || name.contains("TTLB");
            }
            title |= entity.common().layer.eq_ignore_ascii_case("A-ANNO-TTLB")
                && matches!(entity, EntityType::Text(_) | EntityType::MText(_));
        }
        texts += sheet_texts;
        dimensions += sheet_dimensions;
        let filename = if sheet.is_model {
            "model.png".into()
        } else {
            format!("layout-{index:03}.png")
        };
        let mut args = json!({"sheet":sheet.block,"width":1600,"marks":false});
        if count == 0 {
            args["bounds"] = json!([[0, 0], [1, 1]]);
        }
        let mut render = qiyas_ai::surface::run_session(
            session,
            qiyas_ai::surface::CAPTURE,
            "picture",
            &args,
            Source::Ai,
        );
        if render
            .as_ref()
            .is_err_and(|error| error.to_string() == "Nothing is drawn on that sheet")
        {
            args["bounds"] = json!([[0, 0], [1, 1]]);
            render = qiyas_ai::surface::run_session(
                session,
                qiyas_ai::surface::CAPTURE,
                "picture",
                &args,
                Source::Ai,
            );
        }
        let mut caption = String::new();
        match render {
            Ok(ToolOutput::Image {
                png,
                caption: rendered,
            }) => {
                fs::write(output.join(&filename), png).map_err(|e| e.to_string())?;
                caption = rendered;
            }
            Ok(_) => errors.push(format!("{}: renderer returned no image", sheet.name)),
            Err(error) => errors.push(format!("{}: {error}", sheet.name)),
        }
        sheets.push(json!({"name":sheet.name,"block":sheet.block,"is_model":sheet.is_model,"entities":count,"texts":sheet_texts,"dimensions":sheet_dimensions,"title_block":!sheet.is_model && title,"png":filename,"render_caption":caption}));
    }
    let mut nested = BTreeMap::<String, usize>::new();
    for definition in &definitions {
        for entity in session
            .drawing
            .entities_in(definition["name"].as_str().unwrap())
        {
            if let EntityType::Insert(insert) = entity {
                *nested.entry(insert.block_name.clone()).or_default() += 1;
            }
        }
    }
    for definition in &mut definitions {
        let name = definition["name"].as_str().unwrap().to_owned();
        definition["inserts"] = json!(inserts.get(&name).copied().unwrap_or(0));
        definition["nested_inserts"] = json!(nested.get(&name).copied().unwrap_or(0));
    }
    definitions.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(
        json!({"file":"drawing.dwg","entities":kinds.values().sum::<usize>(),"entities_by_kind":kinds,"entities_by_layer":layers,"texts":texts,"dimensions":dimensions,"blocks_defined":definitions.len(),"named_blocks_defined":session.blocks().len(),"blocks":definitions,"inserts_by_block":inserts,"sheets":sheets,"layouts":sheets.iter().filter(|s|s["is_model"]==false).count(),"sheets_with_title_blocks":sheets.iter().filter(|s|s["title_block"]==true).count()}),
    )
}

fn cell(value: &Value) -> String {
    let text = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string());
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
        .replace(['\n', '\r'], "<br>")
}

fn metric_rows(report: &Value) -> BTreeMap<String, Value> {
    let mut rows = BTreeMap::new();
    for key in [
        "provider",
        "model",
        "resolved_model",
        "language",
        "status",
        "artifacts_complete",
        "requests",
        "request_scope",
        "request_limit_scope",
        "max_requests",
        "tool_calls",
        "elapsed_ms",
        "total_elapsed_ms",
        "brief",
        "final_answer",
        "artifact_errors",
        "count_scope",
        "title_block_detection",
    ] {
        rows.insert(key.into(), report[key].clone());
    }
    rows.insert(
        "failed_calls".into(),
        json!(report["failed_calls"].as_array().map_or(0, Vec::len)),
    );
    for key in [
        "entities",
        "texts",
        "dimensions",
        "blocks_defined",
        "named_blocks_defined",
        "layouts",
        "sheets_with_title_blocks",
    ] {
        rows.insert(format!("drawing.{key}"), report["drawing"][key].clone());
    }
    for key in ["tool_calls_by_operation", "tool_calls_by_kind"] {
        if let Some(values) = report[key].as_object() {
            for (name, value) in values {
                rows.insert(format!("{key}.{name}"), value.clone());
            }
        }
    }
    for key in ["entities_by_kind", "entities_by_layer", "inserts_by_block"] {
        if let Some(values) = report["drawing"][key].as_object() {
            for (name, value) in values {
                rows.insert(format!("drawing.{key}.{name}"), value.clone());
            }
        }
    }
    for kind in [
        "input",
        "output",
        "cache_read_input",
        "cache_creation_input",
    ] {
        for statistic in ["total", "max", "mean", "known_requests"] {
            rows.insert(
                format!("tokens.{kind}.{statistic}"),
                report["tokens"][kind][statistic].clone(),
            );
        }
    }
    for request in report["request_usage"].as_array().into_iter().flatten() {
        rows.insert(format!("request.{}", request["number"]), request.clone());
    }
    for (index, failed) in report["failed_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        rows.insert(format!("failed_call.{}", index + 1), failed.clone());
    }
    for block in report["drawing"]["blocks"].as_array().into_iter().flatten() {
        rows.insert(
            format!("block.{}", block["name"].as_str().unwrap_or("")),
            block.clone(),
        );
    }
    for sheet in report["drawing"]["sheets"].as_array().into_iter().flatten() {
        rows.insert(
            format!("sheet.{}", sheet["name"].as_str().unwrap_or("")),
            sheet.clone(),
        );
    }
    rows.insert("cost".into(), report["cost"].clone());
    rows
}

pub fn markdown(report: &Value) -> String {
    let mut text = String::from("# Mizan design run\n\nFor review. Counts describe the drawing; they do not score creativity or engineering correctness.\n\n| Metric | Value |\n| --- | --- |\n");
    for (key, value) in metric_rows(report) {
        text.push_str(&format!("| {} | {} |\n", cell(&json!(key)), cell(&value)));
    }
    text
}

pub fn compare(a: &Path, b: &Path) -> Result<String, String> {
    let read = |path: &Path| -> Result<Value, String> {
        let report: Value = read_json(&path.join("report.json"))?;
        if report["schema_version"] != 1 || report["kind"] != "design_run" {
            return Err("Expected a design-run report with schema_version 1".into());
        }
        Ok(report)
    };
    let a_rows = metric_rows(&read(a)?);
    let b_rows = metric_rows(&read(b)?);
    let keys: BTreeSet<_> = a_rows.keys().chain(b_rows.keys()).collect();
    let mut text = format!(
        "# Mizan design comparison\n\nA: {}<br>B: {}\n\n| Metric | A | B |\n| --- | --- | --- |\n",
        cell(&json!(a)),
        cell(&json!(b))
    );
    for key in keys {
        text.push_str(&format!(
            "| {} | {} | {} |\n",
            cell(&json!(key)),
            a_rows.get(key).map(cell).unwrap_or_else(|| "—".into()),
            b_rows.get(key).map(cell).unwrap_or_else(|| "—".into())
        ));
    }
    Ok(text)
}

#[cfg(test)]
#[path = "design_tests.rs"]
mod tests;
