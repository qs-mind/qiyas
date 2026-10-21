//! Expressions for dimensional constraints and data-defined quantities. The language has
//! arithmetic, powers, comparisons, logic, strings, lazy conditionals and resolver-backed
//! lookups. It has no drawing or trade knowledge and no access to time, files or randomness.

use std::collections::{BTreeMap, BTreeSet};

/// A value in a quantity expression. Text and booleans are never silently converted to numbers.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
}

impl Value {
    pub fn as_number(&self) -> Result<f64, ExprError> {
        match self {
            Self::Number(n) if n.is_finite() => Ok(*n),
            Self::Number(_) => Err(ExprError::NotANumber("Value".into())),
            _ => Err(ExprError::Type("Expected a number".into())),
        }
    }

    /// Conditions accept booleans and finite numbers (zero is false); never text.
    pub fn as_bool(&self) -> Result<bool, ExprError> {
        match self {
            Self::Bool(b) => Ok(*b),
            Self::Number(_) => Ok(self.as_number()? != 0.0),
            Self::Text(_) => Err(ExprError::Type("Expected a boolean or number".into())),
        }
    }

    pub fn as_text(&self) -> Result<&str, ExprError> {
        match self {
            Self::Text(s) => Ok(s),
            _ => Err(ExprError::Type("Expected text".into())),
        }
    }
}

/// The caller supplies properties, geometry facts and exact table lookups. The evaluator
/// neither picks a nearest row nor supplies an invented value for a missing key or column.
pub trait Resolver {
    fn resolve(&self, name: &str) -> Option<Value>;

    fn lookup(&self, _table: &str, _key: &Value, _column: &str) -> Option<Value> {
        None
    }
}

impl<F: Fn(&str) -> Option<Value>> Resolver for F {
    fn resolve(&self, name: &str) -> Option<Value> {
        self(name)
    }
}

/// A parsed expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Text(String),
    Bool(bool),
    Name(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    Sin,
    Cos,
    Tan,
    Sqrt,
    Abs,
    If,
    Min,
    Max,
    Round,
    Ceil,
    Floor,
    Lookup,
}

impl Func {
    fn named(name: &str) -> Option<Func> {
        Some(match name.to_ascii_lowercase().as_str() {
            "sin" => Func::Sin,
            "cos" => Func::Cos,
            "tan" => Func::Tan,
            "sqrt" => Func::Sqrt,
            "abs" => Func::Abs,
            "if" => Func::If,
            "min" => Func::Min,
            "max" => Func::Max,
            "round" => Func::Round,
            "ceil" => Func::Ceil,
            "floor" => Func::Floor,
            "lookup" => Func::Lookup,
            _ => return None,
        })
    }

    fn check_arity(self, count: usize) -> Result<(), ExprError> {
        let expected = match self {
            Self::If | Self::Lookup if count != 3 => Some("three arguments"),
            Self::Min | Self::Max if count == 0 => Some("at least one argument"),
            Self::Round if !(1..=2).contains(&count) => Some("one or two arguments"),
            Self::Sin
            | Self::Cos
            | Self::Tan
            | Self::Sqrt
            | Self::Abs
            | Self::Ceil
            | Self::Floor
                if count != 1 =>
            {
                Some("one argument")
            }
            _ => None,
        };
        match expected {
            Some(expected) => Err(ExprError::Syntax(format!("{self:?} needs {expected}"))),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ExprError {
    #[error("{0}")]
    Syntax(String),
    #[error("There is no parameter named {0}")]
    Unknown(String),
    #[error("These parameters depend on each other in a circle: {}", .0.join(" > "))]
    Circular(Vec<String>),
    #[error("{0} is not a number (a division by 0?)")]
    NotANumber(String),
    #[error("It uses {0}, which can't be worked out")]
    Uses(String),
    #[error("{0}")]
    Type(String),
    #[error("There is no lookup value for {0}")]
    Lookup(String),
}

fn reserved(name: &str) -> bool {
    Func::named(name).is_some()
        || ["pi", "true", "false", "and", "or", "not"]
            .iter()
            .any(|word| name.eq_ignore_ascii_case(word))
}

/// Parameter names are a letter or `_`, then letters, digits or `_`; reserved words are refused.
pub fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !reserved(text)
}

/// Trim and normalize Arabic-Indic/Persian digits and the Arabic decimal separator outside
/// string literals. A string is data: a quoted table key must keep its exact spelling.
pub fn normalize(text: &str) -> String {
    ascii_digits(text.trim())
}

fn ascii_digits(text: &str) -> String {
    let mut quote = None;
    let mut escaped = false;
    text.chars()
        .map(|c| {
            if let Some(q) = quote {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                }
                return c;
            }
            match c {
                '\'' | '"' => {
                    quote = Some(c);
                    c
                }
                '\u{0660}'..='\u{0669}' => char::from(b'0' + (c as u32 - 0x0660) as u8),
                '\u{06F0}'..='\u{06F9}' => char::from(b'0' + (c as u32 - 0x06F0) as u8),
                '\u{066B}' => '.',
                _ => c,
            }
        })
        .collect()
}

// Bound nested input before recursing so a malformed knowledge record cannot exhaust the stack.
const MAX_DEPTH: usize = 128;
const MAX_NODES: usize = 4096;

struct Parser {
    chars: Vec<char>,
    at: usize,
    depth: usize,
    nodes: usize,
}

impl Parser {
    fn skip(&mut self) {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.skip();
        self.chars.get(self.at).copied()
    }

    fn eat(&mut self, token: &str) -> bool {
        self.skip();
        if self.chars[self.at..]
            .iter()
            .copied()
            .zip(token.chars())
            .all(|(a, b)| a == b)
            && self.chars.len() - self.at >= token.len()
        {
            self.at += token.len();
            true
        } else {
            false
        }
    }

    fn word(&mut self, word: &str) -> bool {
        self.skip();
        let end = self.at + word.len();
        if self
            .chars
            .get(self.at..end)
            .is_some_and(|chars| chars.iter().collect::<String>().eq_ignore_ascii_case(word))
            && !self
                .chars
                .get(end)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        {
            self.at = end;
            true
        } else {
            false
        }
    }

    fn nested(
        &mut self,
        parse: fn(&mut Self) -> Result<Expr, ExprError>,
    ) -> Result<Expr, ExprError> {
        if self.depth >= MAX_DEPTH {
            return Err(ExprError::Syntax(
                "The expression is nested too deeply".into(),
            ));
        }
        self.depth += 1;
        let result = parse(self);
        self.depth -= 1;
        result
    }

    fn node(&mut self) -> Result<(), ExprError> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(ExprError::Syntax("The expression is too large".into()));
        }
        Ok(())
    }

    fn expr(&mut self) -> Result<Expr, ExprError> {
        self.chain(Self::and, |p| {
            (p.word("or") || p.eat("||")).then_some(Op::Or)
        })
    }

    fn and(&mut self) -> Result<Expr, ExprError> {
        self.chain(Self::not, |p| {
            (p.word("and") || p.eat("&&")).then_some(Op::And)
        })
    }

    fn not(&mut self) -> Result<Expr, ExprError> {
        if self.word("not") || self.eat("!") {
            self.node()?;
            return Ok(Expr::Not(Box::new(self.nested(Self::not)?)));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Expr, ExprError> {
        let left = self.sum()?;
        let op = [
            ("<=", Op::Le),
            (">=", Op::Ge),
            ("==", Op::Eq),
            ("!=", Op::Ne),
            ("<>", Op::Ne),
            ("<", Op::Lt),
            (">", Op::Gt),
            ("=", Op::Eq),
        ]
        .into_iter()
        .find_map(|(token, op)| self.eat(token).then_some(op));
        if let Some(op) = op {
            self.node()?;
            Ok(Expr::Binary(op, Box::new(left), Box::new(self.sum()?)))
        } else {
            Ok(left)
        }
    }

    fn sum(&mut self) -> Result<Expr, ExprError> {
        self.chain(Self::term, |p| {
            if p.eat("+") {
                Some(Op::Add)
            } else if p.eat("-") {
                Some(Op::Sub)
            } else {
                None
            }
        })
    }

    fn term(&mut self) -> Result<Expr, ExprError> {
        self.chain(Self::unary, |p| {
            if p.eat("*") {
                Some(Op::Mul)
            } else if p.eat("/") {
                Some(Op::Div)
            } else {
                None
            }
        })
    }

    fn chain(
        &mut self,
        parse: fn(&mut Self) -> Result<Expr, ExprError>,
        op: impl Fn(&mut Self) -> Option<Op>,
    ) -> Result<Expr, ExprError> {
        let mut left = parse(self)?;
        while let Some(op) = op(self) {
            self.node()?;
            left = Expr::Binary(op, Box::new(left), Box::new(parse(self)?));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, ExprError> {
        if self.eat("-") {
            self.node()?;
            return Ok(Expr::Neg(Box::new(self.nested(Self::unary)?)));
        }
        if self.eat("+") {
            return self.nested(Self::unary);
        }
        self.power()
    }

    fn power(&mut self) -> Result<Expr, ExprError> {
        let left = self.primary()?;
        if self.eat("^") || self.eat("**") {
            self.node()?;
            Ok(Expr::Binary(
                Op::Pow,
                Box::new(left),
                Box::new(self.nested(Self::unary)?),
            ))
        } else {
            Ok(left)
        }
    }

    fn primary(&mut self) -> Result<Expr, ExprError> {
        self.node()?;
        match self.peek() {
            Some('(') => {
                self.at += 1;
                let inner = self.nested(Self::expr)?;
                if !self.eat(")") {
                    return Err(ExprError::Syntax("A bracket is not closed".into()));
                }
                Ok(inner)
            }
            Some('\'' | '"') => self.string(),
            Some(c) if c.is_ascii_digit() || c == '.' => self.number(),
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                let start = self.at;
                loop {
                    while self
                        .chars
                        .get(self.at)
                        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                    {
                        self.at += 1;
                    }
                    if self.chars.get(self.at) == Some(&'.')
                        && self
                            .chars
                            .get(self.at + 1)
                            .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')
                    {
                        self.at += 1;
                    } else {
                        break;
                    }
                }
                let name: String = self.chars[start..self.at].iter().collect();
                if self.eat("(") {
                    let func = Func::named(&name)
                        .ok_or_else(|| ExprError::Syntax(format!("There is no function {name}")))?;
                    let mut args = Vec::new();
                    if !self.eat(")") {
                        loop {
                            args.push(self.nested(Self::expr)?);
                            if self.eat(")") {
                                break;
                            }
                            if !self.eat(",") {
                                return Err(ExprError::Syntax(
                                    "Expected a comma or closing bracket".into(),
                                ));
                            }
                        }
                    }
                    func.check_arity(args.len())?;
                    return Ok(Expr::Call(func, args));
                }
                match name.to_ascii_lowercase().as_str() {
                    "pi" => Ok(Expr::Number(std::f64::consts::PI)),
                    "true" => Ok(Expr::Bool(true)),
                    "false" => Ok(Expr::Bool(false)),
                    "and" | "or" | "not" => Err(ExprError::Syntax(format!("Unexpected {name}"))),
                    _ => Ok(Expr::Name(name)),
                }
            }
            Some(c) => Err(ExprError::Syntax(format!("Unexpected \"{c}\""))),
            None => Err(ExprError::Syntax("The expression ends too soon".into())),
        }
    }

    fn string(&mut self) -> Result<Expr, ExprError> {
        let quote = self.chars[self.at];
        self.at += 1;
        let mut text = String::new();
        while let Some(c) = self.chars.get(self.at).copied() {
            self.at += 1;
            if c == quote {
                return Ok(Expr::Text(text));
            }
            if c == '\\' {
                let escaped = self
                    .chars
                    .get(self.at)
                    .copied()
                    .ok_or_else(|| ExprError::Syntax("A string is not closed".into()))?;
                self.at += 1;
                text.push(match escaped {
                    '\\' | '\'' | '"' => escaped,
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    _ => {
                        return Err(ExprError::Syntax(format!(
                            "Unknown string escape \\{escaped}"
                        )))
                    }
                });
            } else {
                text.push(c);
            }
        }
        Err(ExprError::Syntax("A string is not closed".into()))
    }

    fn number(&mut self) -> Result<Expr, ExprError> {
        let start = self.at;
        let digits = |p: &mut Parser| {
            while p.chars.get(p.at).is_some_and(|c| c.is_ascii_digit()) {
                p.at += 1;
            }
        };
        digits(self);
        if self.chars.get(self.at) == Some(&'.') {
            self.at += 1;
            digits(self);
        }
        if matches!(self.chars.get(self.at), Some('e' | 'E')) {
            let back = self.at;
            self.at += 1;
            if matches!(self.chars.get(self.at), Some('+' | '-')) {
                self.at += 1;
            }
            let exp = self.at;
            digits(self);
            if self.at == exp {
                self.at = back;
            }
        }
        let text: String = self.chars[start..self.at].iter().collect();
        text.parse()
            .map(Expr::Number)
            .map_err(|_| ExprError::Syntax(format!("\"{text}\" is no number")))
    }
}

impl Expr {
    /// Parse the entire expression. Power is right-associative and binds more tightly than
    /// unary minus; comparisons bind more tightly than `not`, `and`, then `or`.
    pub fn parse(text: &str) -> Result<Expr, ExprError> {
        if text.len() > 65536 {
            return Err(ExprError::Syntax("The expression is too large".into()));
        }
        let mut p = Parser {
            chars: ascii_digits(text).chars().collect(),
            at: 0,
            depth: 0,
            nodes: 0,
        };
        let e = p.expr()?;
        // A long unparenthesized sum also builds a nested tree.
        let mut pending = vec![(&e, 0)];
        while let Some((node, depth)) = pending.pop() {
            if depth > MAX_DEPTH {
                return Err(ExprError::Syntax(
                    "The expression is nested too deeply".into(),
                ));
            }
            match node {
                Self::Neg(e) | Self::Not(e) => pending.push((e, depth + 1)),
                Self::Binary(_, a, b) => {
                    pending.push((a, depth + 1));
                    pending.push((b, depth + 1));
                }
                Self::Call(_, args) => pending.extend(args.iter().map(|arg| (arg, depth + 1))),
                _ => {}
            }
        }
        match p.peek() {
            None => Ok(e),
            Some(c) => Err(ExprError::Syntax(format!("Unexpected \"{c}\""))),
        }
    }

    /// The existing numeric interface for dimensional constraints and parameters.
    pub fn eval(&self, value: &dyn Fn(&str) -> Option<f64>) -> Result<f64, ExprError> {
        self.eval_with(&|name: &str| value(name).map(Value::Number))?
            .as_number()
    }

    /// Evaluate against caller-owned data. `if`, `and` and `or` only evaluate needed branches.
    pub fn eval_with(&self, resolver: &dyn Resolver) -> Result<Value, ExprError> {
        let result = match self {
            Self::Number(n) => Value::Number(*n),
            Self::Text(s) => Value::Text(s.clone()),
            Self::Bool(b) => Value::Bool(*b),
            Self::Name(n) => resolver
                .resolve(n)
                .ok_or_else(|| ExprError::Unknown(n.clone()))?,
            Self::Neg(e) => Value::Number(-e.eval_with(resolver)?.as_number()?),
            Self::Not(e) => Value::Bool(!e.eval_with(resolver)?.as_bool()?),
            Self::Binary(Op::And, a, b) => {
                Value::Bool(a.eval_with(resolver)?.as_bool()? && b.eval_with(resolver)?.as_bool()?)
            }
            Self::Binary(Op::Or, a, b) => {
                Value::Bool(a.eval_with(resolver)?.as_bool()? || b.eval_with(resolver)?.as_bool()?)
            }
            Self::Binary(op, a, b) => {
                Self::binary(*op, a.eval_with(resolver)?, b.eval_with(resolver)?)?
            }
            Self::Call(func, args) => {
                // Also guard expressions constructed directly through the public enum.
                func.check_arity(args.len())?;
                if *func == Func::If {
                    let branch = if args[0].eval_with(resolver)?.as_bool()? {
                        1
                    } else {
                        2
                    };
                    args[branch].eval_with(resolver)?
                } else if *func == Func::Lookup {
                    let table = args[0].eval_with(resolver)?;
                    let key = args[1].eval_with(resolver)?;
                    let column = args[2].eval_with(resolver)?;
                    let (table, column) = (table.as_text()?, column.as_text()?);
                    resolver
                        .lookup(table, &key, column)
                        .ok_or_else(|| ExprError::Lookup(format!("{table}[{key:?}].{column}")))?
                } else {
                    let numbers = args
                        .iter()
                        .map(|e| e.eval_with(resolver)?.as_number())
                        .collect::<Result<Vec<_>, _>>()?;
                    Value::Number(Self::function(*func, &numbers)?)
                }
            }
        };
        if matches!(result, Value::Number(n) if !n.is_finite()) {
            Err(ExprError::NotANumber(self.to_string()))
        } else {
            Ok(result)
        }
    }

    fn binary(op: Op, a: Value, b: Value) -> Result<Value, ExprError> {
        if matches!(op, Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge) {
            let ordering = match (&a, &b) {
                (Value::Number(a), Value::Number(b)) => a.partial_cmp(b),
                (Value::Text(a), Value::Text(b)) => Some(a.cmp(b)),
                (Value::Bool(a), Value::Bool(b)) if matches!(op, Op::Eq | Op::Ne) => Some(a.cmp(b)),
                _ => {
                    return Err(ExprError::Type(
                        "A comparison needs values of the same type".into(),
                    ))
                }
            }
            .ok_or_else(|| ExprError::NotANumber("Comparison".into()))?;
            return Ok(Value::Bool(match op {
                Op::Eq => ordering.is_eq(),
                Op::Ne => !ordering.is_eq(),
                Op::Lt => ordering.is_lt(),
                Op::Le => !ordering.is_gt(),
                Op::Gt => ordering.is_gt(),
                Op::Ge => !ordering.is_lt(),
                _ => unreachable!("comparison operators only"),
            }));
        }
        let (a, b) = (a.as_number()?, b.as_number()?);
        Ok(Value::Number(match op {
            Op::Add => a + b,
            Op::Sub => a - b,
            Op::Mul => a * b,
            Op::Div => a / b,
            Op::Pow => a.powf(b),
            _ => unreachable!("logic and comparison handled separately"),
        }))
    }

    fn function(func: Func, args: &[f64]) -> Result<f64, ExprError> {
        let x = args[0];
        Ok(match func {
            Func::Sin => x.to_radians().sin(),
            Func::Cos => x.to_radians().cos(),
            Func::Tan => x.to_radians().tan(),
            Func::Sqrt => x.sqrt(),
            Func::Abs => x.abs(),
            Func::Min => args.iter().copied().fold(x, f64::min),
            Func::Max => args.iter().copied().fold(x, f64::max),
            Func::Ceil => x.ceil(),
            Func::Floor => x.floor(),
            Func::Round => {
                // Decimal places, with halfway values rounded away from zero, independent of locale.
                let digits = args.get(1).copied().unwrap_or(0.0);
                if digits.fract() != 0.0 || !(-308.0..=308.0).contains(&digits) {
                    return Err(ExprError::Type(
                        "Round decimal places must be an integer from -308 to 308".into(),
                    ));
                }
                let factor = 10f64.powi(digits as i32);
                (x * factor).round() / factor
            }
            Func::If | Func::Lookup => unreachable!("handled before numeric functions"),
        })
    }

    /// All names, including names in unselected branches, each once in order of first use.
    /// Static dependency checking of dimensional parameters deliberately uses this full list.
    pub fn names(&self) -> Vec<String> {
        fn walk(e: &Expr, out: &mut Vec<String>) {
            match e {
                Expr::Name(n) => {
                    if !out.contains(n) {
                        out.push(n.clone());
                    }
                }
                Expr::Neg(e) | Expr::Not(e) => walk(e, out),
                Expr::Call(_, args) => {
                    for e in args {
                        walk(e, out);
                    }
                }
                Expr::Binary(_, a, b) => {
                    walk(a, out);
                    walk(b, out);
                }
                Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) => {}
            }
        }
        let mut out = Vec::new();
        walk(self, &mut out);
        out
    }
}

impl std::fmt::Display for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Expr::Number(n) => write!(f, "{n}"),
            Expr::Name(n) => write!(f, "{n}"),
            Expr::Bool(b) => write!(f, "{b}"),
            Expr::Text(text) => {
                write!(f, "\"")?;
                for c in text.chars() {
                    match c {
                        '\\' => write!(f, "\\\\")?,
                        '"' => write!(f, "\\\"")?,
                        '\n' => write!(f, "\\n")?,
                        '\r' => write!(f, "\\r")?,
                        '\t' => write!(f, "\\t")?,
                        _ => write!(f, "{c}")?,
                    }
                }
                write!(f, "\"")
            }
            Expr::Neg(e) => write!(f, "-({e})"),
            Expr::Not(e) => write!(f, "not ({e})"),
            Expr::Binary(op, a, b) => {
                let sign = match op {
                    Op::Add => "+",
                    Op::Sub => "-",
                    Op::Mul => "*",
                    Op::Div => "/",
                    Op::Pow => "^",
                    Op::Eq => "==",
                    Op::Ne => "!=",
                    Op::Lt => "<",
                    Op::Le => "<=",
                    Op::Gt => ">",
                    Op::Ge => ">=",
                    Op::And => "and",
                    Op::Or => "or",
                };
                write!(f, "({a} {sign} {b})")
            }
            Expr::Call(func, args) => {
                write!(f, "{}(", format!("{func:?}").to_lowercase())?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{arg}")?;
                }
                write!(f, ")")
            }
        }
    }
}

/// Every parameter's value, from each one's expression (`name -> expression`). A parameter may
/// use any other; one that depends on itself, straight or through others, is refused
/// (`Circular`), as is one using a name not in `exprs` (`Unknown`).
pub fn evaluate_all(exprs: &BTreeMap<String, String>) -> Result<BTreeMap<String, f64>, ExprError> {
    let mut parsed = BTreeMap::new();
    for (name, text) in exprs {
        let e = Expr::parse(text).map_err(|e| match e {
            ExprError::Syntax(why) => ExprError::Syntax(format!("{name}: {why}")),
            other => other,
        })?;
        for used in e.names() {
            if !exprs.contains_key(&used) {
                return Err(ExprError::Unknown(used));
            }
        }
        parsed.insert(name.as_str(), e);
    }
    // Depth first, in name order: a name met again while still open closes a circle.
    let mut values: BTreeMap<String, f64> = BTreeMap::new();
    let mut open: Vec<&str> = Vec::new();
    let mut opened: BTreeSet<&str> = BTreeSet::new();
    fn visit<'a>(
        name: &'a str,
        parsed: &'a BTreeMap<&'a str, Expr>,
        values: &mut BTreeMap<String, f64>,
        open: &mut Vec<&'a str>,
        opened: &mut BTreeSet<&'a str>,
    ) -> Result<(), ExprError> {
        if values.contains_key(name) {
            return Ok(());
        }
        if opened.contains(name) {
            let from = open.iter().position(|n| *n == name).unwrap_or(0);
            let mut circle: Vec<String> = open[from..].iter().map(|n| n.to_string()).collect();
            circle.push(name.to_string());
            return Err(ExprError::Circular(circle));
        }
        let (key, e) = parsed.get_key_value(name).expect("checked known");
        opened.insert(*key);
        open.push(*key);
        for used in e.names() {
            let (used, _) = parsed.get_key_value(used.as_str()).expect("checked known");
            visit(used, parsed, values, open, opened)?;
        }
        open.pop();
        let v = e
            .eval(&|n| values.get(n).copied())
            .map_err(|err| match err {
                ExprError::NotANumber(_) => ExprError::NotANumber(name.to_string()),
                other => other,
            })?;
        values.insert(name.to_string(), v);
        Ok(())
    }
    for name in parsed.keys() {
        visit(name, &parsed, &mut values, &mut open, &mut opened)?;
    }
    Ok(values)
}

/// Every parameter's value, or why it has none, each on its own: one that can't be parsed, uses
/// a name not in `exprs`, sits in a circle or is no number has an error, and so does every one
/// using it (`Uses`); the others are worked out as usual.
pub fn evaluate_each(exprs: &BTreeMap<String, String>) -> BTreeMap<String, Result<f64, ExprError>> {
    let parsed: BTreeMap<&str, Result<Expr, ExprError>> = exprs
        .iter()
        .map(|(name, text)| (name.as_str(), Expr::parse(text)))
        .collect();
    let mut out: BTreeMap<String, Result<f64, ExprError>> = BTreeMap::new();
    let mut open: Vec<String> = Vec::new();
    fn visit(
        name: &str,
        parsed: &BTreeMap<&str, Result<Expr, ExprError>>,
        out: &mut BTreeMap<String, Result<f64, ExprError>>,
        open: &mut Vec<String>,
    ) -> Result<f64, ExprError> {
        if let Some(done) = out.get(name) {
            return done.clone();
        }
        if let Some(from) = open.iter().position(|n| n == name) {
            let mut circle = open[from..].to_vec();
            circle.push(name.to_string());
            return Err(ExprError::Circular(circle));
        }
        open.push(name.to_string());
        let result = match &parsed[name] {
            Err(e) => Err(e.clone()),
            Ok(e) => {
                let mut failed = None;
                for used in e.names() {
                    if !parsed.contains_key(used.as_str()) {
                        failed = Some(ExprError::Unknown(used));
                        break;
                    }
                    match visit(&used, parsed, out, open) {
                        Ok(_) => {}
                        Err(ExprError::Circular(c)) => {
                            failed = Some(ExprError::Circular(c));
                            break;
                        }
                        Err(_) => {
                            failed = Some(ExprError::Uses(used));
                            break;
                        }
                    }
                }
                match failed {
                    Some(e) => Err(e),
                    None => e
                        .eval(&|n| out.get(n).and_then(|r| r.as_ref().ok().copied()))
                        .map_err(|_| ExprError::NotANumber(name.to_string())),
                }
            }
        };
        open.pop();
        out.insert(name.to_string(), result.clone());
        result
    }
    for name in parsed.keys() {
        let _ = visit(name, &parsed, &mut out, &mut open);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arabic_digits_and_names() {
        let none = |_: &str| None;
        assert_eq!(Expr::parse("١٢٫٥").unwrap().eval(&none).unwrap(), 12.5);
        assert!(is_name("d1") && is_name("_w") && is_name("width2"));
        assert!(!is_name("1d") && !is_name("sin") && !is_name("PI") && !is_name("a-b"));
    }
}
