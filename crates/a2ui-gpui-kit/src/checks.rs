//! Dynamic values and the supported subset of basic-catalog functions: `required`, `length`,
//! `regex`, and the `and`/`or`/`not` combinators that compose them.

use regex::{Regex, RegexBuilder};
use serde_json::{Map, Value, json};

use crate::error::A2uiError;
use crate::pointer;
use crate::protocol::BASIC_CATALOG_ID;

const MAX_PATTERN_CHARS: usize = 256;
const MAX_REGEX_VALUE_CHARS: usize = 4_096;

/// A `Dynamic*` property: a literal, a `{"@path"}` binding, or a `{"@call"}` function call.
#[derive(Debug, Clone)]
pub enum Dynamic {
    Literal(Value),
    Path(String),
    Call(Box<Call>),
}

#[derive(Debug, Clone)]
pub enum Call {
    Required(Dynamic),
    Length {
        value: Dynamic,
        min: Option<u64>,
        max: Option<u64>,
    },
    Regex {
        value: Dynamic,
        regex: Regex,
    },
    And(Vec<Dynamic>),
    Or(Vec<Dynamic>),
    Not(Dynamic),
}

/// One `@checks` entry.
#[derive(Debug, Clone)]
pub struct CheckRule {
    pub condition: Dynamic,
    pub message: Option<String>,
}

/// Where a binding is evaluated: the surface data model and, inside a `List` template, the
/// pointer of the current item that relative paths resolve against.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub model: &'a Value,
    pub base: Option<&'a str>,
}

impl Dynamic {
    pub fn parse(value: &Value, path: &str) -> Result<Self, A2uiError> {
        let Value::Object(map) = value else {
            return Ok(Self::Literal(value.clone()));
        };
        if let Some(binding) = map.get("@path") {
            return match (binding, map.len()) {
                (Value::String(p), 1) => Ok(Self::Path(p.clone())),
                _ => Err(A2uiError::validation(
                    path,
                    "A binding is exactly {\"@path\": \"<JSON Pointer>\"}.",
                )),
            };
        }
        if map.contains_key("@call") {
            return Call::parse(map, path).map(|call| Self::Call(Box::new(call)));
        }
        Ok(Self::Literal(literal_object(map, path)?))
    }

    pub fn eval(&self, scope: Scope) -> Value {
        match self {
            Self::Literal(value) => value.clone(),
            Self::Path(path) => pointer::get(scope.model, &pointer::resolve(scope.base, path))
                .cloned()
                .unwrap_or(Value::Null),
            Self::Call(call) => call.eval(scope),
        }
    }

    pub fn eval_text(&self, scope: Scope) -> String {
        to_text(&self.eval(scope))
    }

    pub fn eval_bool(&self, scope: Scope) -> bool {
        truthy(&self.eval(scope))
    }
}

impl Call {
    fn parse(map: &Map<String, Value>, path: &str) -> Result<Self, A2uiError> {
        let name = map.get("@call").and_then(Value::as_str).unwrap_or_default();
        let call_path = format!("{path}/@call");
        for key in map.keys() {
            match key.as_str() {
                "@call" | "args" => {}
                "catalogId" if map[key].as_str() == Some(BASIC_CATALOG_ID) => {}
                "catalogId" => {
                    return Err(A2uiError::validation(
                        format!("{path}/catalogId"),
                        "Only the basic catalog is supported.",
                    ));
                }
                other => {
                    return Err(A2uiError::validation(
                        format!("{path}/{}", pointer::escape(other)),
                        format!("Unexpected property \"{other}\" on a function call."),
                    ));
                }
            }
        }
        let empty = Map::new();
        let args = match map.get("args") {
            Some(Value::Object(args)) => args,
            None => &empty,
            Some(_) => {
                return Err(A2uiError::validation(
                    format!("{path}/args"),
                    "args must be an object.",
                ));
            }
        };
        let args_path = format!("{path}/args");
        let allow = |allowed: &[&str]| -> Result<(), A2uiError> {
            match args.keys().find(|k| !allowed.contains(&k.as_str())) {
                Some(k) => Err(A2uiError::validation(
                    format!("{args_path}/{}", pointer::escape(k)),
                    format!("{name} has no argument \"{k}\"."),
                )),
                None => Ok(()),
            }
        };
        let arg = |key: &str| -> Result<Dynamic, A2uiError> {
            let at = format!("{args_path}/{key}");
            match args.get(key) {
                Some(v) => Dynamic::parse(v, &at),
                None => Err(A2uiError::validation(
                    at,
                    format!("{name} requires \"{key}\"."),
                )),
            }
        };
        let list = |key: &str| -> Result<Vec<Dynamic>, A2uiError> {
            let at = format!("{args_path}/{key}");
            match args.get(key) {
                Some(Value::Array(items)) if items.len() >= 2 => items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| Dynamic::parse(v, &format!("{at}/{i}")))
                    .collect(),
                _ => Err(A2uiError::validation(
                    at,
                    format!("{name} takes at least two values."),
                )),
            }
        };
        let bound = |key: &str| -> Result<Option<u64>, A2uiError> {
            match args.get(key) {
                None => Ok(None),
                Some(v) => v.as_u64().map(Some).ok_or_else(|| {
                    A2uiError::validation(
                        format!("{args_path}/{key}"),
                        "Expected a non-negative integer.",
                    )
                }),
            }
        };
        match name {
            "required" => allow(&["value"]).and_then(|_| arg("value").map(Self::Required)),
            "length" => {
                allow(&["value", "min", "max"])?;
                let (min, max) = (bound("min")?, bound("max")?);
                if min.is_none() && max.is_none() {
                    return Err(A2uiError::validation(
                        args_path,
                        "length requires min or max.",
                    ));
                }
                Ok(Self::Length {
                    value: arg("value")?,
                    min,
                    max,
                })
            }
            "regex" => {
                allow(&["value", "pattern"])?;
                let pattern = args.get("pattern").and_then(Value::as_str).ok_or_else(|| {
                    A2uiError::validation(
                        format!("{args_path}/pattern"),
                        "regex requires a string pattern.",
                    )
                })?;
                let regex = compile(pattern)
                    .map_err(|e| A2uiError::validation(format!("{args_path}/pattern"), e))?;
                Ok(Self::Regex {
                    value: arg("value")?,
                    regex,
                })
            }
            "and" => allow(&["values"]).and_then(|_| list("values").map(Self::And)),
            "or" => allow(&["values"]).and_then(|_| list("values").map(Self::Or)),
            "not" => allow(&["value"]).and_then(|_| arg("value").map(Self::Not)),
            other => Err(A2uiError::validation(
                call_path,
                format!("Function \"{other}\" is not supported by this renderer."),
            )),
        }
    }

    fn eval(&self, scope: Scope) -> Value {
        let valid = |valid: bool| json!({ "valid": valid });
        match self {
            Self::Required(value) => valid(match value.eval(scope) {
                Value::Null => false,
                Value::String(s) => !s.is_empty(),
                Value::Array(a) => !a.is_empty(),
                Value::Object(o) => !o.is_empty(),
                _ => true,
            }),
            Self::Length { value, min, max } => {
                let len = value.eval_text(scope).chars().count() as u64;
                valid(min.is_none_or(|m| len >= m) && max.is_none_or(|m| len <= m))
            }
            Self::Regex { value, regex } => {
                let text = value.eval_text(scope);
                valid(text.chars().count() <= MAX_REGEX_VALUE_CHARS && regex.is_match(&text))
            }
            Self::And(values) => Value::Bool(values.iter().all(|v| v.eval_bool(scope))),
            Self::Or(values) => Value::Bool(values.iter().any(|v| v.eval_bool(scope))),
            Self::Not(value) => Value::Bool(!value.eval_bool(scope)),
        }
    }
}

impl CheckRule {
    pub fn parse(value: &Value, path: &str) -> Result<Self, A2uiError> {
        let Value::Object(map) = value else {
            return Err(A2uiError::validation(
                path,
                "A check rule must be an object.",
            ));
        };
        if let Some(key) = map.keys().find(|k| *k != "condition" && *k != "message") {
            return Err(A2uiError::validation(
                format!("{path}/{}", pointer::escape(key)),
                format!("Unexpected property \"{key}\" on a check rule."),
            ));
        }
        let condition = match map.get("condition") {
            Some(c @ Value::Object(_)) => Dynamic::parse(c, &format!("{path}/condition"))?,
            _ => {
                return Err(A2uiError::validation(
                    format!("{path}/condition"),
                    "A check condition is a binding or a function call.",
                ));
            }
        };
        let message = match map.get("message") {
            None => None,
            Some(Value::String(m)) => Some(m.clone()),
            Some(_) => {
                return Err(A2uiError::validation(
                    format!("{path}/message"),
                    "message must be a string.",
                ));
            }
        };
        Ok(Self { condition, message })
    }

    /// `None` when the check passes, otherwise the message to show.
    pub fn failure(&self, scope: Scope) -> Option<String> {
        let result = self.condition.eval(scope);
        let fallback = || {
            self.message
                .clone()
                .unwrap_or_else(|| "Invalid value.".into())
        };
        match result.get("valid").and_then(Value::as_bool) {
            Some(true) => None,
            Some(false) => Some(
                result
                    .get("message")
                    .and_then(Value::as_str)
                    .map_or_else(fallback, str::to_owned),
            ),
            None if truthy(&result) => None,
            None => Some(fallback()),
        }
    }
}

/// The failure messages of `checks`, in order; empty when all pass.
pub fn failures(checks: &[CheckRule], scope: Scope) -> Vec<String> {
    checks.iter().filter_map(|c| c.failure(scope)).collect()
}

/// A2UI's string conversion: `null` is empty, containers are JSON.
pub fn to_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The `DynamicBoolean` coercion table from the spec.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::Object(map) => map.get("valid").and_then(Value::as_bool).unwrap_or(false),
        Value::String(s) => s.eq_ignore_ascii_case("true"),
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::Null | Value::Array(_) => false,
    }
}

fn literal_object(map: &Map<String, Value>, path: &str) -> Result<Value, A2uiError> {
    map.iter()
        .map(|(key, value)| match key.strip_prefix('@') {
            Some(rest) if rest.starts_with('@') => Ok((rest.to_owned(), value.clone())),
            Some(_) => Err(A2uiError::validation(
                format!("{path}/{}", pointer::escape(key)),
                format!("\"{key}\" is not a recognised directive; escape a literal '@' as '@@'."),
            )),
            None => Ok((key.clone(), value.clone())),
        })
        .collect::<Result<Map<_, _>, _>>()
        .map(Value::Object)
}

/// Compiles an A2UI-dialect pattern with Rust's linear-time engine, translating the ASCII-only
/// classes and rejecting constructs the dialect forbids or `regex` cannot express (lookaround).
pub fn compile(pattern: &str) -> Result<Regex, String> {
    if pattern.chars().count() > MAX_PATTERN_CHARS {
        return Err(format!(
            "Patterns are limited to {MAX_PATTERN_CHARS} characters."
        ));
    }
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut chars = pattern.chars().peekable();
    let mut in_class = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let Some(e) = chars.next() else {
                    return Err("Pattern ends with a lone backslash.".into());
                };
                match e {
                    '1'..='9' | 'k' => return Err("Backreferences are not allowed.".into()),
                    'p' | 'P' => return Err("Unicode property escapes are not allowed.".into()),
                    'c' => return Err("Control-letter escapes are not allowed.".into()),
                    '0' if chars.peek().is_some_and(char::is_ascii_digit) => {
                        return Err("Octal escapes are not allowed.".into());
                    }
                    '0' => out.push_str("\\x00"),
                    'd' if in_class => out.push_str("0-9"),
                    'd' => out.push_str("[0-9]"),
                    'w' if in_class => out.push_str("0-9A-Za-z_"),
                    'w' => out.push_str("[0-9A-Za-z_]"),
                    'D' | 'W' if in_class => {
                        return Err(
                            "Negated class escapes inside a class are not supported.".into()
                        );
                    }
                    'D' => out.push_str("[^0-9]"),
                    'W' => out.push_str("[^0-9A-Za-z_]"),
                    'b' if in_class => out.push_str("\\x08"),
                    'b' | 'B' => out.push_str(&format!("(?-u:\\{e})")),
                    'u' if chars.peek() == Some(&'{') => {
                        out.push_str("\\x");
                    }
                    'u' => {
                        let hex: String = chars.by_ref().take(4).collect();
                        if hex.len() != 4 || !hex.chars().all(|h| h.is_ascii_hexdigit()) {
                            return Err("\\u needs four hex digits.".into());
                        }
                        out.push_str(&format!("\\x{{{hex}}}"));
                    }
                    'x' | 's' | 'S' | 't' | 'n' | 'v' | 'f' | 'r' => {
                        out.push('\\');
                        out.push(e);
                    }
                    e if e.is_ascii_alphanumeric() => {
                        return Err(format!("\\{e} is not allowed in the A2UI regex dialect."));
                    }
                    e => {
                        out.push('\\');
                        out.push(e);
                    }
                }
            }
            '[' if in_class => out.push_str("\\["),
            '[' => {
                in_class = true;
                out.push('[');
                if chars.peek() == Some(&'^') {
                    chars.next();
                    out.push('^');
                }
                if chars.peek() == Some(&']') {
                    return Err("Empty character classes are not supported.".into());
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            '&' | '~' if in_class => {
                out.push('\\');
                out.push(c);
            }
            '(' if !in_class && chars.peek() == Some(&'?') => {
                chars.next();
                match chars.next() {
                    Some(':') => out.push_str("(?:"),
                    Some('=' | '!') => {
                        return Err("Lookahead is not supported by this renderer.".into());
                    }
                    Some('<') => return Err("Lookbehind and named groups are not allowed.".into()),
                    _ => return Err("Inline flags are not allowed.".into()),
                }
            }
            '.' if !in_class => out.push_str("[^\\n\\r\\x{2028}\\x{2029}]"),
            c => out.push(c),
        }
    }
    if in_class {
        return Err("Unterminated character class.".into());
    }
    RegexBuilder::new(&out)
        .size_limit(1 << 20)
        .build()
        .map_err(|e| format!("Invalid pattern: {e}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CheckRule, Dynamic, Scope, compile, truthy};

    fn rule(condition: serde_json::Value) -> CheckRule {
        CheckRule::parse(&json!({"condition": condition, "message": "bad"}), "/c").unwrap()
    }

    #[test]
    fn dialect_translation_keeps_ascii_classes_and_rejects_forbidden_constructs() {
        let digits = compile(r"^\d{3}$").unwrap();
        assert!(digits.is_match("123"));
        assert!(
            !digits.is_match("١٢٣"),
            "\\d is ASCII-only in the A2UI dialect"
        );
        assert!(
            compile(r"^[a-z]+@[a-z]+\.com$")
                .unwrap()
                .is_match("ab@cd.com")
        );
        assert!(compile(r"^[\w-]+$").unwrap().is_match("a_b-c"));
        assert!(!compile("^a.b$").unwrap().is_match("a\rb"));
        assert!(compile(r"^A\u{42}$").unwrap().is_match("AB"));
        for bad in [
            r"(\w)\1", "(?<=a)b", "(?i)a", r"\p{L}", "(?=a)", "[]", "abc\\", r"\q",
        ] {
            assert!(compile(bad).is_err(), "{bad} must be rejected");
        }
        assert!(compile(&"a".repeat(257)).is_err());
    }

    #[test]
    fn required_length_and_regex_report_failures() {
        let model = json!({"email": "", "code": "12345", "name": "Ada"});
        let scope = Scope {
            model: &model,
            base: None,
        };
        let required = rule(json!({"@call": "required", "args": {"value": {"@path": "/email"}}}));
        assert_eq!(required.failure(scope).as_deref(), Some("bad"));
        let length = rule(
            json!({"@call": "length", "args": {"value": {"@path": "/name"}, "min": 2, "max": 3}}),
        );
        assert_eq!(length.failure(scope), None);
        let regex = rule(
            json!({"@call": "regex", "args": {"value": {"@path": "/code"}, "pattern": "^\\d{4}$"}}),
        );
        assert!(regex.failure(scope).is_some());
        let bare_path = rule(json!({"@path": "/name"}));
        assert_eq!(
            bare_path.failure(scope).as_deref(),
            Some("bad"),
            "\"Ada\" coerces to false"
        );
    }

    #[test]
    fn malformed_function_arguments_are_rejected() {
        let parse = |v| Dynamic::parse(&v, "/x");
        assert!(parse(json!({"@call": "length", "args": {"value": "a"}})).is_err());
        assert!(parse(json!({"@call": "required", "args": {"value": "a", "extra": 1}})).is_err());
        assert!(parse(json!({"@call": "and", "args": {"values": [true]}})).is_err());
        assert!(parse(json!({"@call": "regex", "args": {"value": "a", "pattern": "(a"}})).is_err());
        assert!(
            parse(json!({"@call": "required", "catalogId": "elsewhere", "args": {"value": 1}}))
                .is_err()
        );
    }

    #[test]
    fn combinators_coerce_validation_results() {
        let model = json!({"a": "x", "b": ""});
        let scope = Scope {
            model: &model,
            base: None,
        };
        let req = |p: &str| json!({"@call": "required", "args": {"value": {"@path": p}}});
        let or = Dynamic::parse(
            &json!({"@call": "or", "args": {"values": [req("/a"), req("/b")]}}),
            "",
        )
        .unwrap();
        let and = Dynamic::parse(
            &json!({"@call": "and", "args": {"values": [req("/a"), req("/b")]}}),
            "",
        )
        .unwrap();
        let not =
            Dynamic::parse(&json!({"@call": "not", "args": {"value": req("/b")}}), "").unwrap();
        assert!(or.eval_bool(scope));
        assert!(!and.eval_bool(scope));
        assert!(not.eval_bool(scope));
    }

    #[test]
    fn unsupported_functions_and_bad_directives_are_rejected() {
        let e = Dynamic::parse(&json!({"@call": "openUrl", "args": {"url": "x"}}), "/text")
            .unwrap_err();
        assert_eq!(e.path, "/text/@call");
        assert!(
            Dynamic::parse(
                &json!({"@call": "formatString", "args": {"value": "x"}}),
                ""
            )
            .is_err()
        );
        assert!(Dynamic::parse(&json!({"@call": "@index"}), "").is_err());
        assert!(Dynamic::parse(&json!({"@bogus": 1}), "").is_err());
        assert!(Dynamic::parse(&json!({"@path": "/a", "extra": 1}), "").is_err());
        let escaped = Dynamic::parse(&json!({"@@type": "Thing"}), "").unwrap();
        let model = json!({});
        assert_eq!(
            escaped.eval(Scope {
                model: &model,
                base: None
            }),
            json!({"@type": "Thing"})
        );
    }

    #[test]
    fn coercion_follows_the_spec_table() {
        assert!(truthy(&json!("TRUE")));
        assert!(!truthy(&json!("hello")));
        assert!(!truthy(&json!(0)));
        assert!(truthy(&json!(2.5)));
        assert!(!truthy(&json!(null)));
        assert!(!truthy(&json!([1])));
        assert!(truthy(&json!({"valid": true})));
    }
}
