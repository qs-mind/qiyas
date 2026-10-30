//! Source-only Arabic style checks. Never rewrite model output or user drawing text.
use qiyas_ai::prompt::{system_prompt, takeoff_workflow, DrawingFacts, Lang};
use serde::Deserialize;

// Run the existing Arabic instruction contracts in the permitted integration binary too.
#[path = "../src/prompt_tests_ar.rs"]
mod prompt_contracts;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Glossary {
    concepts: BTreeMap<String, Concept>,
    banned_patterns: Vec<Pattern>,
    arabic_letter_ranges: Vec<[u32; 2]>,
    diacritic_ranges: Vec<[u32; 2]>,
    digit_ranges: Vec<[u32; 2]>,
    allowed_diacritics: Vec<char>,
    rust_literal_exceptions: Vec<Exception>,
    examples: Examples,
}
#[derive(Deserialize)]
struct Concept {
    avoid: Vec<String>,
}
#[derive(Deserialize)]
struct Pattern {
    kind: String,
    value: String,
}
#[derive(Deserialize)]
struct Exception {
    path: String,
    text: String,
    reason: String,
}
#[derive(Deserialize)]
struct Examples {
    valid: Vec<String>,
    invalid: Vec<String>,
}
fn glossary() -> Glossary {
    serde_json::from_str(include_str!("../../../src/i18n/ar-glossary.json")).unwrap()
}
fn in_ranges(c: char, ranges: &[[u32; 2]]) -> bool {
    ranges.iter().any(|[a, b]| (*a..=*b).contains(&(c as u32)))
}
impl Glossary {
    fn arabic(&self, c: char) -> bool {
        in_ranges(c, &self.arabic_letter_ranges)
    }
    fn number_or_latin(&self, c: char) -> bool {
        c.is_ascii_alphabetic() || in_ranges(c, &self.digit_ranges)
    }
    fn word(&self, c: char) -> bool {
        self.arabic(c) || self.number_or_latin(c)
    }
    fn errors(&self, value: &str) -> Vec<String> {
        let mut errors = Vec::new();
        for term in
            self.concepts
                .values()
                .flat_map(|c| &c.avoid)
                .chain(self.banned_patterns.iter().map(|p| {
                    assert_eq!(p.kind, "word", "Implement new pattern kinds in both guards");
                    &p.value
                }))
        {
            for (i, _) in value.match_indices(term) {
                let before = value[..i].chars().next_back();
                let after = value[i + term.len()..].chars().next();
                if !before.is_some_and(|c| self.word(c)) && !after.is_some_and(|c| self.word(c)) {
                    errors.push(format!("banned term: {term}"));
                }
            }
        }
        for c in value.chars() {
            if in_ranges(c, &self.diacritic_ranges) && !self.allowed_diacritics.contains(&c) {
                errors.push(format!("diacritic: {c}"));
            }
        }
        // Rust format placeholders may include a precision, or be unnamed.
        // Replace them only for spacing checks, without touching source text.
        let mut rendered = String::new();
        let mut rest = value;
        while let Some(i) = rest.find('{') {
            rendered.push_str(&rest[..i]);
            if let Some(end) = rest[i..].find('}') {
                let field = &rest[i + 1..i + end];
                if field
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_: .?".contains(c))
                {
                    rendered.push_str("A1");
                    rest = &rest[i + end + 1..];
                    continue;
                }
            }
            rendered.push('{');
            rest = &rest[i + 1..];
        }
        rendered.push_str(rest);
        let chars: Vec<_> = rendered.chars().collect();
        for pair in chars.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            if (self.arabic(a) && self.number_or_latin(b))
                || (self.number_or_latin(a) && self.arabic(b))
            {
                errors.push(format!("missing space: {a}{b}"));
            }
        }
        errors.sort();
        errors.dedup();
        errors
    }
}

/// A small lexer, not a quote regex: skip comments/characters, retain escaped,
/// raw and multiline strings. Offsets refer to the original source for diagnostics.
fn literals(source: &str) -> Vec<(usize, String)> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if source[i..].starts_with("//") {
            i += source[i..].find('\n').unwrap_or(bytes.len() - i);
        } else if source[i..].starts_with("/*") {
            let mut depth = 1;
            i += 2;
            while i < bytes.len() && depth > 0 {
                if source[i..].starts_with("/*") {
                    depth += 1;
                    i += 2;
                } else if source[i..].starts_with("*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += source[i..].chars().next().unwrap().len_utf8();
                }
            }
        } else if bytes[i] == b'r' {
            let start = i;
            let mut quote = i + 1;
            while bytes.get(quote) == Some(&b'#') {
                quote += 1;
            }
            if bytes.get(quote) == Some(&b'"') {
                let end_mark = format!("\"{}", "#".repeat(quote - start - 1));
                let end = quote + 1 + source[quote + 1..].find(&end_mark).unwrap();
                out.push((start, source[quote + 1..end].into()));
                i = end + end_mark.len();
            } else {
                i += 1;
            }
        } else if bytes[i] == b'"' {
            let start = i;
            let mut value = String::new();
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                let c = source[i..].chars().next().unwrap();
                i += c.len_utf8();
                if c != '\\' {
                    value.push(c);
                    continue;
                }
                let escaped = source[i..].chars().next().unwrap();
                i += escaped.len_utf8();
                match escaped {
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    '0' => value.push('\0'),
                    'u' => {
                        assert_eq!(bytes[i], b'{');
                        let end = i + source[i..].find('}').unwrap();
                        let hex = source[i + 1..end].replace('_', "");
                        value.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                        i = end + 1;
                    }
                    'x' => {
                        value.push(u8::from_str_radix(&source[i..i + 2], 16).unwrap() as char);
                        i += 2;
                    }
                    '\n' | '\r' => {
                        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                            i += 1;
                        }
                    }
                    c => value.push(c),
                }
            }
            out.push((start, value));
            i += 1;
        } else if bytes[i] == b'\'' {
            // Distinguish 'x', '\"', '\u{...}' from lifetimes such as 'static.
            let next = i + 1;
            let mut end = next;
            if bytes.get(next) == Some(&b'\\') {
                end = if bytes.get(next + 1) == Some(&b'u') {
                    next + source[next..].find('}').unwrap() + 1
                } else if bytes.get(next + 1) == Some(&b'x') {
                    next + 4
                } else {
                    next + 2
                };
            } else if let Some(c) = source[next..].chars().next() {
                end += c.len_utf8();
            }
            i = if bytes.get(end) == Some(&b'\'') {
                end + 1
            } else {
                next
            };
        } else {
            i += source[i..].chars().next().unwrap().len_utf8();
        }
    }
    out
}
fn scan(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(&path, files);
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn arabic_style_sources_follow_shared_glossary() {
    let glossary = glossary();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    scan(&root.join("src"), &mut files);
    files.extend([
        root.join("src/prompt_ar.txt"),
        root.join("src/takeoff_workflow_ar.txt"),
    ]);
    files.sort();
    let mut errors = Vec::new();
    let mut seen = vec![false; glossary.rust_literal_exceptions.len()];
    for file in files {
        let source = fs::read_to_string(&file).unwrap();
        let relative = file
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let texts = if file.extension().unwrap() == "txt" {
            vec![(0, source.clone())]
        } else {
            literals(&source)
        };
        for (offset, text) in texts {
            if let Some((index, exception)) = glossary
                .rust_literal_exceptions
                .iter()
                .enumerate()
                .find(|(_, e)| e.path == relative && e.text == text)
            {
                assert!(!exception.reason.trim().is_empty());
                assert!(
                    !glossary.errors(&text).is_empty(),
                    "Remove obsolete exception: {relative}: {text}"
                );
                seen[index] = true;
                continue;
            }
            let line = source[..offset].bytes().filter(|b| *b == b'\n').count() + 1;
            for error in glossary.errors(&text) {
                errors.push(format!("{relative}:{line}: {error} ({text})"));
            }
        }
    }
    for (seen, exception) in seen.iter().zip(&glossary.rust_literal_exceptions) {
        assert!(
            seen,
            "Exception no longer exists: {}: {}",
            exception.path, exception.text
        );
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn arabic_style_guard_catches_regressions_and_lexes_real_strings() {
    let glossary = glossary();
    for value in &glossary.examples.valid {
        assert!(glossary.errors(value).is_empty(), "{value}");
    }
    for value in &glossary.examples.invalid {
        assert!(!glossary.errors(value).is_empty(), "{value}");
    }
    let source = r###"// "ignored"
/* outer /* "nested" */ comment */
let lifetime: &'static str = "بند\u{64b}ا";
let quote = '\"'; let letter = 'ع';
let raw = r##"لوحتاA1 "inner""##;
let escaped = "نص \"مقتبس\"\nسطر";
let multiline = "نص
عربي";
"###;
    let values: Vec<_> = literals(source).into_iter().map(|(_, s)| s).collect();
    assert_eq!(
        values,
        [
            "بند\u{64b}ا",
            "لوحتاA1 \"inner\"",
            "نص \"مقتبس\"\nسطر",
            "نص\nعربي"
        ]
    );
    assert!(!glossary.errors(&values[0]).is_empty());
    assert!(!glossary.errors(&values[1]).is_empty());
    assert!(glossary.errors(&values[2]).is_empty());
    assert!(glossary.errors(&values[3]).is_empty());
}
