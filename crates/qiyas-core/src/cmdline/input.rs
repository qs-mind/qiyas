use crate::Ucs;

/// Western digits and punctuation for either Arabic digit set.
fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '٠'..='٩' => char::from(b'0' + (c as u32 - 0x0660) as u8),
            '۰'..='۹' => char::from(b'0' + (c as u32 - 0x06f0) as u8),
            '٫' => '.',
            '،' => ',',
            c => c,
        })
        .collect()
}

/// The same plain decimals the window and script grammar accept, without exponents or NaN.
fn decimal(text: &str) -> Option<String> {
    let text = normalize(text.trim());
    let body = text.strip_prefix(['-', '+']).unwrap_or(&text);
    let plain = body.chars().any(|c| c.is_ascii_digit())
        && body.chars().all(|c| c.is_ascii_digit() || c == '.')
        && body.matches('.').count() <= 1;
    plain.then_some(text)
}

pub(super) fn number(text: &str) -> Option<f64> {
    decimal(text)?.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Integer fields include entity handles: keep every digit, including above f64's exact range.
pub(super) fn integer(text: &str) -> Option<serde_json::Number> {
    let text = decimal(text)?;
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    if !fraction.bytes().all(|digit| digit == b'0') {
        return None;
    }
    let digits = whole.trim_start_matches(['-', '+']).trim_start_matches('0');
    if digits.is_empty() {
        return Some(0.into());
    }
    if whole.starts_with('-') {
        format!("-{digits}").parse::<i64>().ok().map(Into::into)
    } else {
        digits.parse::<u64>().ok().map(Into::into)
    }
}

pub(super) fn point(text: &str, last: Option<[f64; 2]>, ucs: Ucs) -> Result<[f64; 2], String> {
    let text = normalize(text.trim());
    let relative = text.starts_with('@');
    let body = text.strip_prefix('@').unwrap_or(&text).trim();
    let bad = || format!("{text} is not a point");
    let origin = if relative {
        last.ok_or_else(|| format!("{text}: no point before it for @"))?
    } else {
        ucs.origin
    };
    let delta = if relative && body.contains('<') {
        let (distance, angle) = body.split_once('<').ok_or_else(bad)?;
        let distance = number(distance).ok_or_else(bad)?;
        let angle = number(angle).ok_or_else(bad)?.to_radians();
        [distance * angle.cos(), distance * angle.sin()]
    } else {
        let (x, y) = body.split_once(',').ok_or_else(bad)?;
        [number(x).ok_or_else(bad)?, number(y).ok_or_else(bad)?]
    };
    let [x, y] = [ucs.x_axis, ucs.y_axis()];
    let p = [
        origin[0] + delta[0] * x[0] + delta[1] * y[0],
        origin[1] + delta[0] * x[1] + delta[1] * y[1],
    ];
    if p.iter().all(|n| n.is_finite()) {
        Ok(p)
    } else {
        Err(bad())
    }
}

/// Double quotes keep a name together, matching the window and script tokenizers.
pub(super) fn tokenize(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut word = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                word.push(c);
            }
            words.push(word);
        } else {
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                word.push(c);
                chars.next();
            }
            words.push(word);
        }
    }
    words
}
