//! Go keeps its own settings in a file managed by `go env -w`.

use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{bail, Context, Result};

pub struct Go;

impl super::App for Go {
    fn name(&self) -> &'static str {
        "go"
    }

    fn available(&self) -> bool {
        super::on_path("go")
    }

    fn list(&self) -> Result<BTreeMap<String, String>> {
        parse_flat_json(&go(&["env", "-json", "-changed"])?)
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        // `-changed` only knows non-default values; `go env KEY` also has defaults
        if let Some(value) = self.list()?.remove(key) {
            return Ok(Some(value));
        }
        let value = go(&["env", key])?;
        let value = value.strip_suffix('\n').unwrap_or(&value);
        Ok((!value.is_empty()).then(|| value.to_string()))
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        go(&["env", "-w", &format!("{key}={value}")]).map(drop)
    }

    fn unset(&self, key: &str) -> Result<()> {
        self.set(key, "")
    }

    fn delete(&self, key: &str) -> Result<()> {
        go(&["env", "-u", key]).map(drop)
    }
}

fn go(args: &[&str]) -> Result<String> {
    let out = Command::new("go")
        .args(args)
        .output()
        .context("could not run go")?;
    if !out.status.success() {
        bail!("go {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    String::from_utf8(out.stdout).context("go printed text that is not UTF-8")
}

/// Parse `{"KEY": "value", ...}`: a flat object of strings, which is all
/// `go env -json` prints.
fn parse_flat_json(text: &str) -> Result<BTreeMap<String, String>> {
    let mut chars = text.chars().peekable();
    let mut map = BTreeMap::new();
    expect(&mut chars, '{')?;
    skip_space(&mut chars);
    if chars.peek() == Some(&'}') {
        return Ok(map);
    }
    loop {
        let key = string(&mut chars)?;
        expect(&mut chars, ':')?;
        let value = string(&mut chars)?;
        map.insert(key, value);
        skip_space(&mut chars);
        match chars.next() {
            Some(',') => continue,
            Some('}') => return Ok(map),
            _ => bail!("unexpected go output: expected ',' or '}}'"),
        }
    }
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

fn skip_space(chars: &mut Chars) {
    while chars.peek().is_some_and(|c| c.is_whitespace()) {
        chars.next();
    }
}

fn expect(chars: &mut Chars, want: char) -> Result<()> {
    skip_space(chars);
    match chars.next() {
        Some(c) if c == want => Ok(()),
        _ => bail!("unexpected go output: expected '{want}'"),
    }
}

fn string(chars: &mut Chars) -> Result<String> {
    expect(chars, '"')?;
    let mut out = String::new();
    loop {
        match chars.next() {
            Some('"') => return Ok(out),
            Some('\\') => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('b') => out.push('\u{8}'),
                Some('f') => out.push('\u{c}'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('u') => {
                    let first = hex4(chars)?;
                    if (0xD800..0xDC00).contains(&first) {
                        // a high surrogate needs its low half
                        if chars.next() != Some('\\') || chars.next() != Some('u') {
                            bail!("unexpected go output: broken surrogate pair");
                        }
                        let second = hex4(chars)?;
                        if !(0xDC00..0xE000).contains(&second) {
                            bail!("unexpected go output: broken surrogate pair");
                        }
                        let code = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                        out.push(char::from_u32(code).context("bad unicode escape")?);
                    } else {
                        out.push(char::from_u32(first).context("bad unicode escape")?);
                    }
                }
                _ => bail!("unexpected go output: bad escape"),
            },
            Some(c) => out.push(c),
            None => bail!("unexpected go output: unterminated string"),
        }
    }
}

fn hex4(chars: &mut Chars) -> Result<u32> {
    let digits: String = chars.take(4).collect();
    if digits.len() != 4 {
        bail!("unexpected go output: short unicode escape");
    }
    u32::from_str_radix(&digits, 16).context("unexpected go output: bad unicode escape")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_maps() {
        let map = parse_flat_json("{\n\t\"A\": \"1\",\n\t\"B\": \"\"\n}\n").unwrap();
        assert_eq!(map["A"], "1");
        assert_eq!(map["B"], "");
        assert!(parse_flat_json("{}").unwrap().is_empty());
        assert!(parse_flat_json("{ }\n").unwrap().is_empty());
    }

    #[test]
    fn parses_escapes() {
        let map = parse_flat_json(r#"{"A":"q\"\\\/\n\t\u00fc\ud83d\ude00"}"#).unwrap();
        assert_eq!(map["A"], "q\"\\/\n\tü😀");
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in ["", "[]", "{\"A\"}", "{\"A\":\"x\"", "{\"A\":\"\\ud83d\"}", "{\"A\":1}"] {
            assert!(parse_flat_json(bad).is_err(), "{bad}");
        }
    }
}
