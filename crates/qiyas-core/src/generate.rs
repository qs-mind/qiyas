//! Bounded recipes expanded into ordinary drawing commands and one batch transaction.
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

use opencadcodec::{EntityType, Handle};
use opencadkernel::geom2d::{Curve, Line};
use qiyas_param::expr::{Expr, ExprError, Resolver, Value as ExprValue};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{CoreError, Outcome, Session, Source};

const MAX_COMMANDS: usize = 20_000;
const MAX_VISITS: usize = 100_000;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_RECIPE_BYTES: usize = 1024 * 1024;
const MAX_EVAL_WORK: usize = 8 * 1024 * 1024;
const MAX_PATH: usize = 4096;
const MAX_ENTITY_WORK: usize = 100_000;

/// Work shared by all recipes and ordinary steps in the outer transaction.
#[derive(Clone, Default)]
pub(crate) struct Budget {
    pub active: bool,
    pub generating: bool,
    pub commands: usize,
    pub visits: usize,
    pub bytes: usize,
    pub recipe_bytes: usize,
    pub work: usize,
    pub entities: usize,
}

impl Budget {
    pub fn check(&self) -> Result<(), CoreError> {
        if self.active {
            for (used, limit, name) in [
                (self.commands, MAX_COMMANDS, "Expanded command"),
                (self.visits, MAX_VISITS, "Loop visit"),
                (self.bytes, MAX_BYTES, "Expanded JSON bytes"),
                (self.recipe_bytes, MAX_RECIPE_BYTES, "Recipe size bytes"),
                (self.work, MAX_EVAL_WORK, "Evaluation work"),
                (self.entities, MAX_ENTITY_WORK, "Entity change"),
            ] {
                if used > limit {
                    return Err(invalid(format!(
                        "{name} limit {limit} exceeded across the batch"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// A literal number or an expression, with an optional leading `=`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Scalar {
    Number(f64),
    Expression(String),
}

/// Loops nest in array order. Count starts at zero; ranges include both endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Loop {
    pub var: String,
    #[serde(default)]
    pub from: Option<Scalar>,
    #[serde(default)]
    pub to: Option<Scalar>,
    #[serde(default)]
    pub step: Option<Scalar>,
    #[serde(default)]
    pub count: Option<Scalar>,
    #[serde(default)]
    pub along: Option<Along>,
}

/// A drawing path handle or connected points, and exactly one of spacing or count.
/// Count includes both ends of an open path; closed paths do not repeat their start.
/// Spacing starts at the path start and never passes its end. Stations expose x, y and
/// angle (tangent in degrees), plus the loop variable as a zero-based station index.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Along {
    #[serde(default)]
    pub path: Option<u64>,
    #[serde(default)]
    pub points: Option<Vec<[Scalar; 2]>>,
    #[serde(default)]
    pub spacing: Option<Scalar>,
    #[serde(default)]
    pub count: Option<Scalar>,
}

/// A recipe contains data only. No code, file loading or nested generators are accepted.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Recipe {
    /// Names may refer to other vars and active outer loop variables. `unit` is drawing
    /// units per metre; multiply metre lengths by unit. Coordinates otherwise stay literal.
    #[serde(default)]
    pub vars: BTreeMap<String, Scalar>,
    /// At most eight loops, nested in order. Inner bounds can use outer variables.
    #[serde(default)]
    pub loops: Vec<Loop>,
    /// Run the iteration only when this expression is true (or nonzero).
    #[serde(default)]
    pub when: Option<String>,
    /// Ordinary wire commands. `=expr` returns a typed value; `{expr}` interpolates text.
    /// Double braces `{{` and `}}` escape literal braces. Optional template `when` skips only
    /// that command; it is removed before wire validation. At most 20000 expanded commands.
    pub commands: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub template: usize,
    /// One-based visit number, including skipped iterations.
    pub iteration: usize,
    pub values: BTreeMap<String, f64>,
    /// Handles added, changed or removed by this command, before later iterations.
    pub handles: Vec<u64>,
    pub defaults_used: Vec<crate::validation::DefaultUsed>,
    pub warnings: Vec<crate::validation::InputIssue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateResult {
    pub template: usize,
    pub count: usize,
    pub handles: Vec<u64>,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

fn at(path: &str, error: impl std::fmt::Display) -> CoreError {
    invalid(format!("{path}: {error}"))
}

fn expression_error(message: impl Into<String>) -> ExprError {
    ExprError::Syntax(message.into())
}

struct Variable {
    expr: Expr,
    cost: usize,
}

fn variable_order(vars: &BTreeMap<String, Variable>, path: &str) -> Result<Vec<String>, CoreError> {
    let mut pending: BTreeMap<_, _> = vars
        .iter()
        .map(|(name, var)| {
            (
                name.clone(),
                var.expr
                    .names()
                    .into_iter()
                    .filter(|n| vars.contains_key(n))
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect();
    let mut order = Vec::new();
    while !pending.is_empty() {
        let Some(name) = pending
            .iter()
            .find(|(_, used)| used.is_empty())
            .map(|(name, _)| name.clone())
        else {
            return Err(at(
                &format!("{path}/vars"),
                format!(
                    "Circular variable dependencies: {}",
                    pending.keys().cloned().collect::<Vec<_>>().join(", ")
                ),
            ));
        };
        pending.remove(&name);
        for used in pending.values_mut() {
            used.remove(&name);
        }
        order.push(name);
    }
    Ok(order)
}

struct Scope<'a> {
    values: &'a BTreeMap<String, f64>,
    unit: f64,
    work: &'a Cell<usize>,
    output: &'a Cell<usize>,
    cache: BTreeMap<String, Result<ExprValue, ExprError>>,
    error: RefCell<Option<ExprError>>,
}

impl Resolver for Scope<'_> {
    fn resolve(&self, name: &str) -> Option<ExprValue> {
        if name == "unit" {
            return Some(ExprValue::Number(self.unit));
        }
        if let Some(value) = self.values.get(name) {
            return Some(ExprValue::Number(*value));
        }
        match self.cache.get(name)? {
            Ok(value) => Some(value.clone()),
            Err(error) => {
                *self.error.borrow_mut() = Some(error.clone());
                None
            }
        }
    }
}

impl Scope<'_> {
    fn charge(&self, cost: usize) -> Result<(), ExprError> {
        let remaining = self.work.get().checked_sub(cost).ok_or_else(|| {
            expression_error(format!(
                "Recipe evaluation work limit {MAX_EVAL_WORK} exceeded"
            ))
        })?;
        self.work.set(remaining);
        Ok(())
    }

    fn eval(&self, text: &str) -> Result<ExprValue, ExprError> {
        self.charge(text.len().max(1))?;
        let result = Expr::parse(text.strip_prefix('=').unwrap_or(text))?.eval_with(self);
        self.error.borrow_mut().take().map_or(result, Err)
    }

    fn scalar(&self, scalar: &Scalar) -> Result<f64, ExprError> {
        match scalar {
            Scalar::Number(n) => ExprValue::Number(*n).as_number(),
            Scalar::Expression(text) => self.eval(text)?.as_number(),
        }
    }
}

struct Expanded {
    input: Value,
    template: usize,
    iteration: usize,
    values: BTreeMap<String, f64>,
}

struct Expansion<'a> {
    session: &'a Session,
    recipe: &'a Recipe,
    vars: BTreeMap<String, Variable>,
    order: Vec<String>,
    work: Cell<usize>,
    visits: usize,
    iterations: usize,
    skipped: usize,
    output: Cell<usize>,
    commands: Vec<Expanded>,
    prior_commands: usize,
    path: String,
}

impl Expansion<'_> {
    fn scope<'a>(&'a self, values: &'a BTreeMap<String, f64>) -> Scope<'a> {
        let mut scope = Scope {
            values,
            unit: 1.0 / self.session.drawing.units.to_metres(),
            work: &self.work,
            output: &self.output,
            cache: BTreeMap::new(),
            error: RefCell::new(None),
        };
        for name in &self.order {
            let var = &self.vars[name];
            let result = scope
                .charge(var.cost)
                .and_then(|()| var.expr.eval_with(&scope));
            let result = scope.error.borrow_mut().take().map_or(result, Err);
            scope.cache.insert(name.clone(), result);
        }
        scope
    }

    fn walk(&mut self, depth: usize, values: BTreeMap<String, f64>) -> Result<(), CoreError> {
        if depth == self.recipe.loops.len() {
            self.iterations += 1;
            let iteration = self.iterations;
            let context =
                |error| invalid(format!("iteration {iteration} {}: {error}", json!(values)));
            let scope = self.scope(&values);
            if let Some(when) = &self.recipe.when {
                if !scope
                    .eval(when)
                    .and_then(|v| v.as_bool())
                    .map_err(|e| context(at(&format!("{}/when", self.path), e)))?
                {
                    self.skipped += 1;
                    return Ok(());
                }
            }
            for name in self.vars.keys() {
                scope
                    .eval(name)
                    .map_err(|e| context(at(&format!("{}/vars/{name}", self.path), e)))?;
            }
            let mut inputs = Vec::new();
            for (template, command) in self.recipe.commands.iter().enumerate() {
                let prefix = format!("{}/commands/{template}", self.path);
                if let Some(when) = command.get("when") {
                    let when = when.as_str().ok_or_else(|| {
                        context(at(&format!("{prefix}/when"), "Expected an expression"))
                    })?;
                    if !scope
                        .eval(when)
                        .and_then(|v| v.as_bool())
                        .map_err(|e| context(at(&format!("{prefix}/when"), e)))?
                    {
                        continue;
                    }
                }
                if self.prior_commands + self.commands.len() + inputs.len() >= MAX_COMMANDS {
                    return Err(context(at(
                        &prefix,
                        format!("Expanded command limit {MAX_COMMANDS} exceeded"),
                    )));
                }
                let mut command = command.clone();
                if let Some(object) = command.as_object_mut() {
                    object.remove("when");
                }
                let mut input = expand_value(&command, &scope, &prefix, 0).map_err(context)?;
                crate::registry::canonicalize(&mut input);
                safe_command(&input).map_err(|e| context(at(&prefix, e)))?;
                inputs.push((template, input));
            }
            for (template, input) in inputs {
                self.commands.push(Expanded {
                    input,
                    template,
                    iteration,
                    values: values.clone(),
                });
            }
            return Ok(());
        }
        let item = &self.recipe.loops[depth];
        let prefix = format!("{}/loops/{depth}", self.path);
        let scope = self.scope(&values);
        let stations = loop_values(self.session, item, &scope)
            .map_err(|e| at(&prefix, format!("{}: {e}", json!(values))))?;
        let var = item.var.clone();
        for (value, station) in stations {
            self.visits += 1;
            if self.visits > MAX_VISITS {
                return Err(at(
                    &prefix,
                    format!("Loop visit limit {MAX_VISITS} exceeded"),
                ));
            }
            let mut next = values.clone();
            next.insert(var.clone(), value);
            if let Some([x, y, angle]) = station {
                next.insert("x".into(), x);
                next.insert("y".into(), y);
                next.insert("angle".into(), angle);
            }
            self.walk(depth + 1, next)?;
        }
        Ok(())
    }
}

fn expand_value(
    value: &Value,
    scope: &Scope<'_>,
    path: &str,
    depth: usize,
) -> Result<Value, CoreError> {
    if depth > 32 {
        return Err(at(path, "Template depth limit 32 exceeded"));
    }
    let cost = match value {
        Value::Array(items) => items.len() + 2,
        Value::Object(items) => items.keys().map(|key| key.len() * 6 + 4).sum::<usize>() + 2,
        _ => 0,
    };
    charge_output(scope, cost, path)?;
    let result: Value = match value {
        Value::String(text) if text.starts_with('=') => {
            let value = scope.eval(text).map_err(|e| at(path, e))?;
            Ok(match value {
                ExprValue::Number(n) => number(n),
                ExprValue::Text(s) => Value::String(s),
                ExprValue::Bool(b) => Value::Bool(b),
            })
        }
        Value::String(text) => interpolate(text, scope)
            .map(Value::String)
            .map_err(|e| at(path, e)),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| expand_value(v, scope, &format!("{path}/{i}"), depth + 1))
            .collect(),
        Value::Object(items) => items
            .iter()
            .map(|(key, v)| {
                let pointer = key.replace('~', "~0").replace('/', "~1");
                Ok((
                    key.clone(),
                    expand_value(v, scope, &format!("{path}/{pointer}"), depth + 1)?,
                ))
            })
            .collect(),
        _ => Ok(value.clone()),
    }?;
    if !result.is_array() && !result.is_object() {
        charge_output(scope, result.to_string().len(), path)?;
    }
    Ok(result)
}

fn charge_output(scope: &Scope<'_>, bytes: usize, path: &str) -> Result<(), CoreError> {
    let remaining = scope.output.get().checked_sub(bytes).ok_or_else(|| {
        at(
            path,
            format!("Expanded JSON limit {MAX_BYTES} bytes exceeded"),
        )
    })?;
    scope.output.set(remaining);
    Ok(())
}

fn number(n: f64) -> Value {
    // Integer command fields (counts, handles) must deserialize as integers.
    if n.fract() == 0.0 && n >= 0.0 && n < u64::MAX as f64 {
        json!(n as u64)
    } else if n.fract() == 0.0 && n >= i64::MIN as f64 && n < i64::MAX as f64 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

fn interpolate(text: &str, scope: &Scope<'_>) -> Result<String, ExprError> {
    let mut out = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        if rest.starts_with("{{") || rest.starts_with("}}") {
            out.push(rest.chars().next().expect("brace"));
            rest = &rest[2..];
        } else if let Some(expr) = rest.strip_prefix('{') {
            let end = expr
                .find('}')
                .ok_or_else(|| expression_error("Unclosed {expression} placeholder"))?;
            match scope.eval(&expr[..end])? {
                ExprValue::Number(n) => out.push_str(&n.to_string()),
                ExprValue::Text(s) => out.push_str(&s),
                ExprValue::Bool(b) => out.push_str(&b.to_string()),
            }
            rest = &expr[end + 1..];
        } else {
            let c = rest.chars().next().expect("nonempty text");
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
        if out.len() > 65536 {
            return Err(expression_error(
                "Generated string limit 65536 bytes exceeded",
            ));
        }
    }
    Ok(out)
}

fn safe_command(input: &Value) -> Result<(), CoreError> {
    let name = input["type"]
        .as_str()
        .ok_or_else(|| invalid("A command needs a literal type"))?;
    if name == "generate" {
        return Err(invalid("generate cannot run inside generate"));
    }
    if name == "batch" {
        return Err(invalid(
            "batch cannot run inside generate; put its commands directly in the recipe templates",
        ));
    }
    let registry = crate::registry::registry();
    let info = registry
        .get(name)
        .ok_or_else(|| invalid(format!("Unknown command {name}")))?;
    if !info.batchable
        || info.internal
        || !(matches!(
            info.category.as_str(),
            "draw" | "text" | "modify" | "dimensions" | "layers" | "blocks"
        ) && info.effect == crate::registry::Effect::Edit
            || name == "set_current")
        || input.get("from_path").is_some_and(|v| !v.is_null())
    {
        return Err(invalid(format!(
            "{name} is not a local drawing command allowed in generate"
        )));
    }
    Ok(())
}

fn count(n: f64) -> Result<usize, CoreError> {
    if n.fract() != 0.0 || n < 0.0 || n > MAX_VISITS as f64 {
        return Err(invalid(format!(
            "Loop count must be an integer from 0 to the visit limit {MAX_VISITS}"
        )));
    }
    Ok(n as usize)
}

type Stations = Vec<(f64, Option<[f64; 3]>)>;

fn loop_values(session: &Session, item: &Loop, scope: &Scope<'_>) -> Result<Stations, CoreError> {
    let scalar = |s: &Scalar| scope.scalar(s).map_err(|e| invalid(e.to_string()));
    if let Some(along) = &item.along {
        if item.count.is_some() || item.from.is_some() || item.to.is_some() || item.step.is_some() {
            return Err(invalid(
                "along cannot be combined with count or range bounds",
            ));
        }
        return along_values(session, along, scope);
    }
    if let Some(n) = &item.count {
        if item.from.is_some() || item.to.is_some() || item.step.is_some() {
            return Err(invalid("count cannot be combined with range bounds"));
        }
        return Ok((0..count(scalar(n)?)?).map(|i| (i as f64, None)).collect());
    }
    let from = scalar(
        item.from
            .as_ref()
            .ok_or_else(|| invalid("Give count, along, or from/to/step"))?,
    )?;
    let to = scalar(
        item.to
            .as_ref()
            .ok_or_else(|| invalid("A range needs to"))?,
    )?;
    let step = scalar(
        item.step
            .as_ref()
            .ok_or_else(|| invalid("A range needs step"))?,
    )?;
    if step == 0.0 || (to - from) * step < 0.0 {
        return Err(invalid(
            "Range step must be nonzero and move from from towards to",
        ));
    }
    let intervals = (to - from) / step;
    if !intervals.is_finite() || intervals >= MAX_VISITS as f64 {
        return Err(invalid(format!("Loop visit limit {MAX_VISITS} exceeded")));
    }
    let n = count((intervals + 1e-10).floor() + 1.0)?;
    (0..n)
        .map(|i| {
            let value = from + step * i as f64;
            if !value.is_finite() || i > 0 && value == from {
                Err(invalid("Range step cannot advance a finite coordinate"))
            } else {
                Ok((value, None))
            }
        })
        .collect()
}

fn along_values(
    session: &Session,
    along: &Along,
    scope: &Scope<'_>,
) -> Result<Stations, CoreError> {
    if along.path.is_some() == along.points.is_some()
        || along.spacing.is_some() == along.count.is_some()
    {
        return Err(invalid(
            "along needs exactly one of path/points and exactly one of spacing/count",
        ));
    }
    let pieces = if let Some(handle) = along.path {
        let entity = session
            .drawing
            .doc
            .get_entity(Handle::new(handle))
            .ok_or(CoreError::NoSuchEntity(handle))?;
        let vertices = match entity {
            EntityType::Line(_) | EntityType::Arc(_) | EntityType::Circle(_) => 2,
            EntityType::LwPolyline(p) => p.vertices.len(),
            _ => {
                return Err(invalid(
                    "along path must be a line, arc, circle or lightweight polyline",
                ))
            }
        };
        scope
            .charge(vertices.saturating_mul(32))
            .map_err(|e| invalid(e.to_string()))?;
        if vertices > MAX_PATH {
            return Err(invalid(format!("Path vertex limit {MAX_PATH} exceeded")));
        }
        let xrefs = session.xref_docs();
        let located = crate::session::locate(&session.drawing.doc, &xrefs, handle, &[])?;
        crate::inquiry::oriented_path(&located, session.measure_tol())
    } else {
        let points = along.points.as_ref().expect("checked points");
        scope
            .charge(points.len().saturating_mul(32))
            .map_err(|e| invalid(e.to_string()))?;
        if !(2..=MAX_PATH).contains(&points.len()) {
            return Err(invalid(format!(
                "Path needs 2 to {MAX_PATH} points (path vertex limit)"
            )));
        }
        let points = points
            .iter()
            .map(|p| {
                Ok([
                    scope.scalar(&p[0]).map_err(|e| invalid(e.to_string()))?,
                    scope.scalar(&p[1]).map_err(|e| invalid(e.to_string()))?,
                ])
            })
            .collect::<Result<Vec<_>, CoreError>>()?;
        points
            .windows(2)
            .map(|p| {
                (
                    Curve::Line(Line {
                        start: p[0],
                        end: p[1],
                    }),
                    false,
                )
            })
            .collect()
    };
    let mut ends = Vec::new();
    let mut total = 0.0;
    for (curve, _) in &pieces {
        let length = curve.length();
        if !length.is_finite() || length <= 0.0 {
            return Err(invalid("Path pieces must have positive finite lengths"));
        }
        total += length;
        ends.push(total);
    }
    if total <= 0.0 || !total.is_finite() || pieces.len() > MAX_PATH {
        return Err(invalid(format!(
            "Path must have positive finite length and at most {MAX_PATH} pieces"
        )));
    }
    let start = crate::inquiry::point_at(&pieces, 0.0);
    let end = crate::inquiry::point_at(&pieces, total);
    let closed = (start[0] - end[0]).hypot(start[1] - end[1]) <= total * 1e-10;
    let (n, spacing) = if let Some(n) = &along.count {
        let n = count(scope.scalar(n).map_err(|e| invalid(e.to_string()))?)?;
        let divisor = if closed { n } else { n.saturating_sub(1) }.max(1);
        (n, total / divisor as f64)
    } else {
        let spacing = scope
            .scalar(along.spacing.as_ref().expect("checked spacing"))
            .map_err(|e| invalid(e.to_string()))?;
        if spacing <= 0.0 {
            return Err(invalid("along spacing must be positive"));
        }
        let ratio = total / spacing;
        let n = if closed {
            ratio.ceil()
        } else {
            (ratio + 1e-10).floor() + 1.0
        };
        if !n.is_finite() {
            return Err(invalid(format!("Loop visit limit {MAX_VISITS} exceeded")));
        }
        (count(n)?, spacing)
    };
    scope
        .charge(n.saturating_mul(32))
        .map_err(|e| invalid(e.to_string()))?;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let distance = (i as f64 * spacing).min(total);
        let index = ends
            .partition_point(|end| *end < distance)
            .min(pieces.len() - 1);
        let (curve, reverse) = &pieces[index];
        let length = curve.length();
        let local = distance - if index == 0 { 0.0 } else { ends[index - 1] };
        let at = |d: f64| curve.point_at_distance(if *reverse { length - d } else { d });
        let position = at(local.clamp(0.0, length));
        let tangent = match curve {
            Curve::Line(line) => [line.end[0] - line.start[0], line.end[1] - line.start[1]],
            Curve::Arc(arc) => [arc.centre[1] - position[1], position[0] - arc.centre[0]],
            Curve::Circle(circle) => [
                circle.centre[1] - position[1],
                position[0] - circle.centre[0],
            ],
            _ => return Err(invalid("Path pieces must be lines or circular arcs")),
        };
        let direction = if *reverse { -1.0 } else { 1.0 };
        let angle = (direction * tangent[1])
            .atan2(direction * tangent[0])
            .to_degrees();
        if !position.iter().chain([angle].iter()).all(|n| n.is_finite()) {
            return Err(invalid("Path station must be finite"));
        }
        out.push((i as f64, Some([position[0], position[1], angle])));
    }
    Ok(out)
}

impl Session {
    pub(crate) fn generate(
        &mut self,
        recipe: Recipe,
        source: Source,
    ) -> Result<Outcome, CoreError> {
        if self.recipe_budget.generating {
            return Err(invalid("generate cannot run inside generate"));
        }
        self.batch_with(source, "generate", None, None, |session| {
            session.recipe_budget.active = true;
            session.recipe_budget.check()?;
            session.recipe_budget.generating = true;
            let result = session.generate_steps(recipe, source);
            session.recipe_budget.generating = false;
            result
        })
    }

    fn generate_steps(&mut self, recipe: Recipe, source: Source) -> Result<Outcome, CoreError> {
        let path = self.batch_step_path.clone();
        if recipe.loops.len() > 8 {
            return Err(invalid("Loop nesting limit 8 exceeded"));
        }
        if recipe.vars.len() > 128 {
            return Err(invalid("Variable count limit 128 exceeded"));
        }
        if recipe.commands.is_empty() {
            return Err(at(
                &format!("{path}/commands"),
                "Give at least one command template",
            ));
        }
        if recipe.commands.len() > MAX_COMMANDS {
            return Err(invalid(format!(
                "Expanded command limit {MAX_COMMANDS} exceeded"
            )));
        }
        self.recipe_budget.recipe_bytes += serde_json::to_vec(&recipe)
            .map_err(|e| invalid(e.to_string()))?
            .len();
        self.recipe_budget.check()?;
        let mut names = BTreeSet::from(["unit".to_owned(), "x".into(), "y".into(), "angle".into()]);
        for (index, item) in recipe.loops.iter().enumerate() {
            if !qiyas_param::expr::is_name(&item.var) || !names.insert(item.var.clone()) {
                return Err(at(
                    &format!("{path}/loops/{index}/var"),
                    "Use a unique, nonreserved variable name",
                ));
            }
        }
        let mut vars = BTreeMap::new();
        for (name, scalar) in &recipe.vars {
            if !qiyas_param::expr::is_name(name) || !names.insert(name.clone()) {
                return Err(at(
                    &format!("{path}/vars/{name}"),
                    "Use a unique, nonreserved variable name",
                ));
            }
            let text = match scalar {
                Scalar::Number(n) => n.to_string(),
                Scalar::Expression(s) => s.clone(),
            };
            let expr = Expr::parse(text.strip_prefix('=').unwrap_or(&text))
                .map_err(|e| at(&format!("{path}/vars/{name}"), e))?;
            vars.insert(
                name.clone(),
                Variable {
                    expr,
                    cost: text.len().max(1),
                },
            );
        }
        let order = variable_order(&vars, &path)?;
        let mut expansion = Expansion {
            session: self,
            recipe: &recipe,
            vars,
            order,
            work: Cell::new(MAX_EVAL_WORK - self.recipe_budget.work),
            visits: self.recipe_budget.visits,
            iterations: 0,
            skipped: 0,
            output: Cell::new(MAX_BYTES - self.recipe_budget.bytes),
            commands: Vec::new(),
            prior_commands: self.recipe_budget.commands,
            path: self.batch_step_path.clone(),
        };
        expansion.walk(0, BTreeMap::new())?;
        let Expansion {
            commands,
            iterations,
            skipped,
            visits,
            work,
            output,
            ..
        } = expansion;
        self.recipe_budget.visits = visits;
        self.recipe_budget.work = MAX_EVAL_WORK - work.get();
        self.recipe_budget.bytes = MAX_BYTES - output.get();
        let session = self;
        let mut steps = Vec::with_capacity(commands.len());
        let mut templates: Vec<_> = (0..recipe.commands.len())
            .map(|template| TemplateResult {
                template,
                count: 0,
                handles: Vec::new(),
            })
            .collect();
        let progress = crate::batch_progress::Batch::new(commands.len());
        for (index, expanded) in commands.into_iter().enumerate() {
            let Expanded {
                input,
                template,
                iteration,
                values,
            } = expanded;
            let prefix = format!("{}/commands/{template}", session.batch_step_path);
            let context = |error| {
                invalid(format!(
                    "{prefix}, iteration {iteration} {}: {error}",
                    json!(values)
                ))
            };
            let mut previous = std::mem::take(&mut session.last_change);
            // run_input resolves handles before validation and keeps each replacement warning.
            let report =
                session.with_batch_step(template, |session| session.run_input(&input, source));
            let change = std::mem::take(&mut session.last_change);
            let mut handles = change.added.clone();
            handles.extend(change.changed.iter().copied());
            handles.extend(change.removed.iter().copied());
            previous.merge(change);
            session.last_change = previous;
            let report = report.map_err(context)?;
            let result = &mut templates[template];
            result.count += 1;
            result.handles.extend(handles.iter().copied());
            steps.push(Step {
                template,
                iteration,
                values,
                handles,
                defaults_used: report.defaults_used,
                warnings: report.warnings,
            });
            progress.completed(index + 1, &input);
        }
        Ok(Outcome::Generated {
            iterations,
            skipped,
            templates,
            steps,
            added: session.last_change.added.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_recipes_share_every_cumulative_budget_and_roll_back() {
        let recipe = json!({"type":"generate","loops":[{"var":"i","count":1}],
            "commands":[{"type":"draw","kind":"point","p":["=i+1",0]}]});
        let mut probe = Session::new_blank(std::path::Path::new("/tmp/recipe-budget.dwg"), 6);
        let usage = probe
            .transaction(Source::Ai, "batch", |s| {
                s.run_input(&recipe, Source::Ai)?;
                Ok(s.recipe_budget.clone())
            })
            .unwrap();
        for limit in [
            "commands",
            "visits",
            "bytes",
            "recipe_bytes",
            "work",
            "entities",
        ] {
            let mut s = Session::new_blank(std::path::Path::new("/tmp/recipe-budget.dwg"), 6);
            let before = s.revision();
            let error = s
                .transaction(Source::Ai, "batch", |s| {
                    // Leave room for exactly one of the two column recipes.
                    match limit {
                        "commands" => s.recipe_budget.commands = MAX_COMMANDS - usage.commands,
                        "visits" => s.recipe_budget.visits = MAX_VISITS - usage.visits,
                        "bytes" => s.recipe_budget.bytes = MAX_BYTES - usage.bytes,
                        "recipe_bytes" => {
                            s.recipe_budget.recipe_bytes = MAX_RECIPE_BYTES - usage.recipe_bytes
                        }
                        "work" => s.recipe_budget.work = MAX_EVAL_WORK - usage.work,
                        "entities" => s.recipe_budget.entities = MAX_ENTITY_WORK - usage.entities,
                        _ => unreachable!(),
                    }
                    s.with_batch_step(0, |s| s.run_input(&recipe, Source::Ai))?;
                    s.with_batch_step(1, |s| s.run_input(&recipe, Source::Ai))
                })
                .unwrap_err()
                .to_string();
            assert!(error.contains("limit"), "{limit}: {error}");
            assert_eq!(s.revision(), before, "{limit}");
            assert_eq!(s.drawing.entities_in("*Model_Space").count(), 0, "{limit}");
            assert!(!s.undo());
            s.run_input(&recipe, Source::Ai).unwrap();
            assert_eq!(s.drawing.entities_in("*Model_Space").count(), 1);
        }
    }
}
