//! A bounded scalar subset of Wallpaper Engine's JavaScript-compatible conditions.
//! Unsupported syntax or non-scalar values fail open; no code is executed.
use serde_json::Value;
use std::{borrow::Cow, collections::BTreeMap};

const MAX_BYTES: usize = 16 * 1024;
const MAX_TOKENS: usize = 512;
const MAX_DEPTH: usize = 32;

type Parsed<T> = Result<T, ()>;

/// Evaluate using the caller's effective values (defaults with current overrides).
/// Missing properties behave as `undefined`; invalid conditions keep controls visible.
pub(crate) fn evaluate(condition: Option<&str>, values: &BTreeMap<String, Value>) -> bool {
    let Some(source) = condition else { return true };
    if source.len() > MAX_BYTES {
        return true;
    }
    if source.trim_matches(js_space).is_empty() {
        return true;
    }
    let result = (|| {
        let tokens = tokenize(source)?;
        let mut parser = Parser { tokens: &tokens, position: 0, values };
        let value = parser.expression(1, 0)?;
        if parser.position != tokens.len() {
            return Err(());
        }
        Ok(value.truthy())
    })();
    result.unwrap_or(true)
}

#[derive(Clone, PartialEq)]
enum Scalar<'a> {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(Cow<'a, str>),
}

impl Scalar<'_> {
    fn truthy(&self) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Bool(v) => *v,
            Self::Number(v) => *v != 0.0 && !v.is_nan(),
            Self::String(v) => !v.is_empty(),
        }
    }
    fn number(&self) -> f64 {
        match self {
            Self::Undefined => f64::NAN,
            Self::Null => 0.0,
            Self::Bool(v) => u8::from(*v) as f64,
            Self::Number(v) => *v,
            Self::String(v) => {
                let s = v.trim_matches(js_space);
                match s {
                    "" => 0.0,
                    "Infinity" | "+Infinity" => f64::INFINITY,
                    "-Infinity" => f64::NEG_INFINITY,
                    _ if ["0x", "0X", "0b", "0B", "0o", "0O"].iter().any(|p| s.starts_with(p)) => {
                        let radix = match s.as_bytes()[1] {
                            b'x' | b'X' => 16,
                            b'b' | b'B' => 2,
                            _ => 8,
                        };
                        if s.len() == 2 {
                            return f64::NAN;
                        }
                        s[2..]
                            .chars()
                            .try_fold(0.0, |n, c| {
                                c.to_digit(radix).map(|d| n * radix as f64 + d as f64)
                            })
                            .unwrap_or(f64::NAN)
                    }
                    _ if decimal_len(s) == Some(s.len()) => s.parse().unwrap_or(f64::NAN),
                    _ => f64::NAN,
                }
            }
        }
    }
    fn loose_eq(&self, other: &Self) -> bool {
        use Scalar::*;
        match (self, other) {
            (Null | Undefined, Null | Undefined) => true,
            (Null | Undefined, _) | (_, Null | Undefined) => false,
            (String(_), String(_)) | (Bool(_), Bool(_)) | (Number(_), Number(_)) => self == other,
            _ => self.number() == other.number(),
        }
    }
}

#[derive(Clone)]
enum Token<'a> {
    Atom(Scalar<'a>),
    Reference(&'a str),
    Symbol(&'a str),
}

fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn decimal_len(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let start = i;
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    let mut digits = i - start;
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        digits += i - start;
    }
    if digits == 0 {
        return None;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return None;
        }
    }
    Some(i)
}

fn quoted(source: &mut &str) -> Parsed<String> {
    let quote = source.chars().next().ok_or(())?;
    *source = &source[1..];
    let mut output = String::new();
    while let Some(c) = source.chars().next() {
        *source = &source[c.len_utf8()..];
        match c {
            c if c == quote => return Ok(output),
            '\n' | '\r' | '\u{2028}' | '\u{2029}' => return Err(()),
            '\\' => {
                let escape = source.chars().next().ok_or(())?;
                *source = &source[escape.len_utf8()..];
                output.push(match escape {
                    '\'' | '"' | '\\' | '/' => escape,
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    'v' => '\u{b}',
                    '0' if !source.starts_with(|c: char| c.is_ascii_digit()) => '\0',
                    'x' | 'u' => {
                        let digits = if escape == 'x' { 2 } else { 4 };
                        let hex = source.get(..digits).ok_or(())?;
                        if !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
                            return Err(());
                        }
                        let code = u32::from_str_radix(hex, 16).map_err(|_| ())?;
                        *source = &source[digits..];
                        char::from_u32(code).ok_or(())?
                    }
                    _ => return Err(()),
                });
            }
            _ => output.push(c),
        }
    }
    Err(())
}

fn tokenize(mut source: &str) -> Parsed<Vec<Token<'_>>> {
    let mut tokens = Vec::new();
    loop {
        source = source.trim_start_matches(js_space);
        if source.is_empty() {
            return Ok(tokens);
        }
        if tokens.len() == MAX_TOKENS {
            return Err(());
        }
        let first = source.as_bytes()[0];
        let token = if matches!(first, b'\'' | b'"') {
            Token::Atom(Scalar::String(Cow::Owned(quoted(&mut source)?)))
        } else if first.is_ascii_alphabetic() || matches!(first, b'_' | b'$') {
            let end = source
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))
                .count();
            let name = &source[..end];
            source = source[end..].trim_start_matches(js_space);
            match name {
                "true" => Token::Atom(Scalar::Bool(true)),
                "false" => Token::Atom(Scalar::Bool(false)),
                "null" => Token::Atom(Scalar::Null),
                "undefined" => Token::Atom(Scalar::Undefined),
                _ => {
                    source = source.strip_prefix('.').ok_or(())?.trim_start_matches(js_space);
                    source = source.strip_prefix("value").ok_or(())?;
                    Token::Reference(name)
                }
            }
        } else if first.is_ascii_digit() || matches!(first, b'.' | b'+' | b'-') {
            let end = decimal_len(source).ok_or(())?;
            let value = source[..end].parse().map_err(|_| ())?;
            source = &source[end..];
            Token::Atom(Scalar::Number(value))
        } else {
            let symbol =
                ["===", "!==", "==", "!=", "<=", ">=", "&&", "||", "!", "<", ">", "(", ")"]
                    .into_iter()
                    .find(|op| source.starts_with(op))
                    .ok_or(())?;
            source = &source[symbol.len()..];
            Token::Symbol(symbol)
        };
        tokens.push(token);
    }
}

struct Parser<'a> {
    tokens: &'a [Token<'a>],
    position: usize,
    values: &'a BTreeMap<String, Value>,
}

impl<'a> Parser<'a> {
    fn expression(&mut self, minimum: u8, depth: usize) -> Parsed<Scalar<'a>> {
        if depth >= MAX_DEPTH {
            return Err(());
        }
        let token = self.tokens.get(self.position).ok_or(())?.clone();
        self.position += 1;
        let mut left = match token {
            Token::Atom(value) => value,
            Token::Reference(name) => match self.values.get(name) {
                None => Scalar::Undefined,
                Some(Value::Null) => Scalar::Null,
                Some(Value::Bool(v)) => Scalar::Bool(*v),
                Some(Value::Number(v)) => Scalar::Number(v.as_f64().ok_or(())?),
                Some(Value::String(v)) => Scalar::String(Cow::Borrowed(v)),
                _ => return Err(()),
            },
            Token::Symbol("!") => Scalar::Bool(!self.expression(5, depth + 1)?.truthy()),
            Token::Symbol("(") => {
                let value = self.expression(1, depth + 1)?;
                if !matches!(self.tokens.get(self.position), Some(Token::Symbol(")"))) {
                    return Err(());
                }
                self.position += 1;
                value
            }
            _ => return Err(()),
        };
        while let Some(Token::Symbol(op)) = self.tokens.get(self.position) {
            let precedence = match *op {
                "||" => 1,
                "&&" => 2,
                "==" | "!=" | "===" | "!==" => 3,
                "<" | "<=" | ">" | ">=" => 4,
                _ => break,
            };
            if precedence < minimum {
                break;
            }
            self.position += 1;
            // Parse both sides, including unused logical operands, to validate all syntax.
            let right = self.expression(precedence + 1, depth + 1)?;
            left = match *op {
                "||" => {
                    if left.truthy() {
                        left
                    } else {
                        right
                    }
                }
                "&&" => {
                    if left.truthy() {
                        right
                    } else {
                        left
                    }
                }
                "==" => Scalar::Bool(left.loose_eq(&right)),
                "!=" => Scalar::Bool(!left.loose_eq(&right)),
                "===" => Scalar::Bool(left == right),
                "!==" => Scalar::Bool(left != right),
                _ => {
                    let order = match (&left, &right) {
                        (Scalar::String(a), Scalar::String(b)) => {
                            Some(a.encode_utf16().cmp(b.encode_utf16()))
                        }
                        _ => left.number().partial_cmp(&right.number()),
                    };
                    use std::cmp::Ordering::{Equal, Greater, Less};
                    Scalar::Bool(matches!(
                        (*op, order),
                        ("<", Some(Less))
                            | ("<=", Some(Less | Equal))
                            | (">", Some(Greater))
                            | (">=", Some(Greater | Equal))
                    ))
                }
            };
        }
        Ok(left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn check(cases: &[(&str, bool)]) {
        for &(condition, expected) in cases {
            assert_eq!(evaluate(Some(condition), &BTreeMap::new()), expected, "{condition}");
        }
    }

    #[test]
    fn missing_and_blank_conditions_show_controls() {
        assert!(evaluate(None, &BTreeMap::new()));
        check(&[("", true), (" \t\n", true)]);
    }

    #[test]
    fn scalar_literals_use_javascript_truthiness() {
        check(&[
            ("true", true),
            ("false", false),
            ("null", false),
            ("undefined", false),
            ("0", false),
            ("-0", false),
            ("0.0", false),
            ("-2.5", true),
            ("+2", true),
            (".5", true),
            ("1.", true),
            ("1e-2", true),
            ("''", false),
            ("\"\"", false),
            ("'false'", true),
            ("'0'", true),
            ("'你好'", true),
        ]);
    }

    #[test]
    fn logic_obeys_precedence_parentheses_and_operand_values() {
        check(&[
            ("!true", false),
            ("!!0", false),
            ("!!!false", true),
            ("false || true && false", false),
            ("(false || true) && false", false),
            ("true || false && false", true),
            ("!(true && (false || true))", false),
            ("(0 || 2) === 2", true),
            ("('x' && 0) === 0", true),
            ("(null || '') === ''", true),
            ("!1 == false", true),
        ]);
    }

    #[test]
    fn comparisons_distinguish_strict_and_loose_equality() {
        check(&[
            ("1 == '1'", true),
            ("1 === '1'", false),
            ("1 != '1'", false),
            ("1 !== '1'", true),
            ("false == 0", true),
            ("true == '1'", true),
            ("false == ''", true),
            ("0 == '  '", true),
            ("null == undefined", true),
            ("null === undefined", false),
            ("null == 0", false),
            ("undefined == 0", false),
            ("'01' == '1'", false),
            ("'no' == 0", false),
            ("'no' != 0", true),
            ("1 < 2", true),
            ("2 <= 2", true),
            ("3 > 2", true),
            ("2 >= 2", true),
            ("2 < 1", false),
            ("3 <= 2", false),
            ("2 > 3", false),
            ("1 >= 2", false),
            ("'10' < '2'", true),
            ("'10' < 2", false),
            ("null >= 0", true),
            ("undefined <= 0", false),
            ("'no' >= 0", false),
            ("1 < 2 == true", true),
            ("3 > 2 > 1", false),
            ("'0x10' == 16", true),
            ("'0b10' == 2", true),
            ("'0o10' == 8", true),
            ("'inf' == 'Infinity'", false),
        ]);
    }

    #[test]
    fn quoted_strings_support_escapes() {
        check(&[
            (r#"'it\'s' == "it's""#, true),
            (r#"'a\nb' === "a\nb""#, true),
            (r#"'\\' === "\\""#, true),
            (r#"'\u0041' === 'A'"#, true),
            (r#"'\t' == ' '"#, false),
            (r#"'\"' === '"'"#, true),
        ]);
    }

    #[test]
    fn caller_values_are_used_without_mutation_or_cached_defaults() {
        let mut defaults = BTreeMap::from([
            ("appdocksettings".into(), json!(true)),
            ("appdockenabled".into(), json!(true)),
            ("appdocksetup".into(), json!(false)),
            ("_flag$1".into(), json!(false)),
            ("mode".into(), json!("2")),
            ("nothing".into(), Value::Null),
        ]);
        let condition = "appdocksettings.value == true && appdockenabled.value == true && appdocksetup.value == true";
        let before = defaults.clone();
        assert!(!evaluate(Some(condition), &defaults));
        assert_eq!(defaults, before);
        defaults.insert("appdocksetup".into(), json!(true));
        let before = defaults.clone();
        assert!(evaluate(Some(condition), &defaults));
        for (condition, expected) in [
            ("!_flag$1.value", true),
            ("mode . value == 2", true),
            ("mode.value === 2", false),
            ("missing.value", false),
            ("missing.value === undefined", true),
            ("nothing.value === null", true),
            ("_flag$1.value || appdocksetup.value", true),
        ] {
            assert_eq!(evaluate(Some(condition), &defaults), expected, "{condition}");
        }
        assert_eq!(defaults, before);
    }

    #[test]
    fn malformed_and_unsupported_conditions_fail_open_even_in_dead_branches() {
        check(&[
            ("false && call()", true),
            ("false && (", true),
            ("false trailing", true),
            ("false)", true),
            ("(false", true),
            ("!", true),
            ("false &&", true),
            ("false & true", true),
            ("false = true", true),
            ("false + 1", true),
            ("false ? 1 : 0", true),
            ("false; true", true),
            ("false // hi", true),
            ("false || [1]", true),
            ("false || {}", true),
            ("false || /x/", true),
            ("foo", true),
            ("foo.other", true),
            ("foo.value.extra", true),
            ("foo['value']", true),
            ("é.value", true),
            ("'unclosed", true),
            ("'bad\\xZZ'", true),
            ("1e", true),
            ("1.2.3", true),
            ("0x", true),
        ]);
        let values =
            BTreeMap::from([("object".into(), json!({"x": 1})), ("array".into(), json!([]))]);
        assert!(evaluate(Some("!object.value"), &values));
        assert!(evaluate(Some("!array.value"), &values));
    }

    #[test]
    fn javascript_whitespace_is_accepted_without_accepting_non_js_whitespace() {
        check(&[
            ("\u{feff}false", false),
            ("true &&\u{feff}false", false),
            ("'\u{feff}' == 0", true),
            ("'\u{85}' == 0", false),
            ("false\u{85}", true),
        ]);
    }

    #[test]
    fn parsing_has_bounded_input_tokens_and_recursion() {
        for condition in [
            format!("{}false{}", "(".repeat(100), ")".repeat(100)),
            format!("{}true", "!".repeat(101)),
            format!("false{}", " || false".repeat(1000)),
            format!("false{}", " ".repeat(100_000)),
        ] {
            assert!(evaluate(Some(&condition), &BTreeMap::new()));
        }
        check(&[("((((false))))", false), ("!!!!!!!!false", false)]);
        for condition in [
            format!("{}false{}", "(".repeat(31), ")".repeat(31)),
            format!("false{}", " || false".repeat(255)),
        ] {
            assert!(!evaluate(Some(&condition), &BTreeMap::new()));
        }
        for condition in [
            format!("{}false{}", "(".repeat(32), ")".repeat(32)),
            format!("false{}", " || false".repeat(256)),
        ] {
            assert!(evaluate(Some(&condition), &BTreeMap::new()));
        }
    }
}
