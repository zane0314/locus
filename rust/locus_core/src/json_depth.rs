use serde_json::{Map, Number, Value};

/// Matches Android JSONTokener's quotes, comments, and unquoted literals before recursive parsing.
pub fn require_json_depth(text: &str) -> Result<(), String> {
    let chars: Vec<char> = text.chars().collect();
    let mut stack = ['\0'; 128];
    let mut depth: usize = 0;
    let mut quote = '\0';
    let mut escaped = false;
    let mut literal = false;
    let mut i: usize = if chars.first() == Some(&'\u{FEFF}') { 1 } else { 0 };

    while i < chars.len() {
        let c = chars[i];
        if quote != '\0' {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                quote = '\0';
            }
            i += 1;
            continue;
        }
        if literal && !matches!(c, '{' | '}' | '[' | ']' | '/' | '\\' | ':' | ',' | '=' | ';' | '#' | ' ' | '\t' | '\u{000c}' | '\r' | '\n') {
            i += 1;
            continue;
        }
        literal = false;
        match c {
            '\'' | '"' => quote = c,
            '#' => {
                while i < chars.len() && chars[i] != '\n' && chars[i] != '\r' {
                    i += 1;
                }
                continue;
            }
            '/' => {
                if i + 1 < chars.len() {
                    match chars[i + 1] {
                        '/' => {
                            i += 2;
                            while i < chars.len() && chars[i] != '\n' && chars[i] != '\r' {
                                i += 1;
                            }
                            continue;
                        }
                        '*' => {
                            let mut end = None;
                            let mut j = i + 2;
                            while j + 1 < chars.len() {
                                if chars[j] == '*' && chars[j + 1] == '/' {
                                    end = Some(j);
                                    break;
                                }
                                j += 1;
                            }
                            let Some(pos) = end else {
                                return Err("JSON注释不完整".to_string());
                            };
                            i = pos + 2;
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            '[' | '{' => {
                if depth >= stack.len() {
                    return Err("JSON嵌套超过128层".to_string());
                }
                stack[depth] = c;
                depth += 1;
            }
            ']' | '}' => {
                let expected = if c == ']' { '[' } else { '{' };
                if depth == 0 || stack[depth - 1] != expected {
                    return Err("JSON结构无效".to_string());
                }
                depth -= 1;
            }
            _ => {
                let is_delim = matches!(c, ' ' | '\t' | '\r' | '\n' | ':' | ',' | ';' | '=' | '\\');
                let is_arrow = c == '>' && i > 0 && matches!(chars[i - 1], ':' | '=');
                if !is_delim && !is_arrow {
                    literal = true;
                }
            }
        }
        i += 1;
    }
    if depth != 0 || quote != '\0' {
        return Err("JSON不完整".to_string());
    }
    Ok(())
}

/// Parses strict RFC 8259 JSON first, falling back to Android `org.json.JSONTokener` lenient rules
/// (single-quoted strings, `//` / `#` / `/* */` comments, `=>` / `:>` / `=` key-value separators,
/// `;` array/object separators, and unquoted literals) after `require_json_depth` has bounded nesting.
pub fn parse_lenient_json(text: &str) -> Result<Value, String> {
    require_json_depth(text)?;
    if let Ok(val) = serde_json::from_str::<Value>(text) {
        return Ok(val);
    }
    let mut tok = AndroidJsonTokener::new(text);
    let val = tok.next_value()?;
    if tok.next_clean()?.is_some() {
        return Err("JSON 存在多余尾部内容".to_string());
    }
    Ok(val)
}

struct AndroidJsonTokener {
    chars: Vec<char>,
    pos: usize,
}

impl AndroidJsonTokener {
    fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let pos = if chars.first() == Some(&'\u{FEFF}') { 1 } else { 0 };
        Self { chars, pos }
    }

    fn next_clean(&mut self) -> Result<Option<char>, String> {
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            self.pos += 1;
            match c {
                '\0'..=' ' => continue,
                '/' => {
                    if self.pos == self.chars.len() {
                        return Ok(Some(c));
                    }
                    match self.chars[self.pos] {
                        '*' => {
                            self.pos += 1;
                            let mut closed = false;
                            while self.pos + 1 < self.chars.len() {
                                if self.chars[self.pos] == '*' && self.chars[self.pos + 1] == '/' {
                                    self.pos += 2;
                                    closed = true;
                                    break;
                                }
                                self.pos += 1;
                            }
                            if !closed {
                                return Err("JSON注释不完整".to_string());
                            }
                        }
                        '/' => {
                            self.pos += 1;
                            self.skip_to_eol();
                        }
                        _ => return Ok(Some(c)),
                    }
                }
                '#' => self.skip_to_eol(),
                _ => return Ok(Some(c)),
            }
        }
        Ok(None)
    }

    fn skip_to_eol(&mut self) {
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            self.pos += 1;
            if c == '\r' || c == '\n' {
                break;
            }
        }
    }

    fn next_value(&mut self) -> Result<Value, String> {
        let Some(c) = self.next_clean()? else {
            return Err("JSON不完整".to_string());
        };
        match c {
            '"' | '\'' => Ok(Value::String(self.next_string(c)?)),
            '{' => self.read_object(),
            '[' => self.read_array(),
            _ => {
                self.pos -= 1;
                self.read_literal()
            }
        }
    }

    fn next_string(&mut self, quote: char) -> Result<String, String> {
        let mut out = String::new();
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            self.pos += 1;
            if c == quote {
                return Ok(out);
            }
            if c == '\\' {
                if self.pos == self.chars.len() {
                    return Err("JSON转义不完整".to_string());
                }
                let esc = self.chars[self.pos];
                self.pos += 1;
                match esc {
                    'u' => {
                        if self.pos + 4 > self.chars.len() {
                            return Err("JSON Unicode转义无效".to_string());
                        }
                        let hex: String = self.chars[self.pos..self.pos + 4].iter().collect();
                        self.pos += 4;
                        let code = u16::from_str_radix(&hex, 16)
                            .map_err(|_| "JSON Unicode转义无效".to_string())?;
                        if (0xD800..=0xDBFF).contains(&code)
                            && self.pos + 6 <= self.chars.len()
                            && self.chars[self.pos] == '\\'
                            && self.chars[self.pos + 1] == 'u'
                        {
                            let low_hex: String =
                                self.chars[self.pos + 2..self.pos + 6].iter().collect();
                            if let Ok(low) = u16::from_str_radix(&low_hex, 16) {
                                if (0xDC00..=0xDFFF).contains(&low) {
                                    self.pos += 6;
                                    let combined = 0x10000
                                        + (((code as u32 - 0xD800) << 10)
                                            | (low as u32 - 0xDC00));
                                    if let Some(ch) = char::from_u32(combined) {
                                        out.push(ch);
                                        continue;
                                    }
                                }
                            }
                        }
                        out.push(char::from_u32(code as u32).unwrap_or('\u{FFFD}'));
                    }
                    't' => out.push('\t'),
                    'b' => out.push('\u{0008}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    'f' => out.push('\u{000c}'),
                    other => out.push(other),
                }
            } else {
                out.push(c);
            }
        }
        Err("JSON字符串未闭合".to_string())
    }

    fn read_literal(&mut self) -> Result<Value, String> {
        let start = self.pos;
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            if matches!(
                c,
                '{' | '}' | '[' | ']' | '/' | '\\' | ':' | ',' | '=' | ';' | '#' | ' ' | '\t'
                    | '\u{000c}' | '\r' | '\n'
            ) {
                break;
            }
            self.pos += 1;
        }
        let raw: String = self.chars[start..self.pos].iter().collect();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("JSON字面量为空".to_string());
        }
        if trimmed.eq_ignore_ascii_case("null") {
            return Ok(Value::Null);
        }
        if trimmed.eq_ignore_ascii_case("true") {
            return Ok(Value::Bool(true));
        }
        if trimmed.eq_ignore_ascii_case("false") {
            return Ok(Value::Bool(false));
        }
        if let Ok(i) = trimmed.parse::<i64>() {
            return Ok(Value::Number(i.into()));
        }
        if let Some(hex) = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
        {
            if let Ok(i) = i64::from_str_radix(hex, 16) {
                return Ok(Value::Number(i.into()));
            }
        }
        if let Ok(f) = trimmed.parse::<f64>() {
            if let Some(num) = Number::from_f64(f) {
                return Ok(Value::Number(num));
            }
        }
        Ok(Value::String(trimmed.to_string()))
    }

    fn value_to_key(val: Value) -> Result<String, String> {
        match val {
            Value::String(s) => Ok(s),
            Value::Number(n) => Ok(n.to_string()),
            Value::Bool(b) => Ok(b.to_string()),
            _ => Err("JSON对象键无效".to_string()),
        }
    }

    fn read_object(&mut self) -> Result<Value, String> {
        let mut map = Map::new();
        let Some(first) = self.next_clean()? else {
            return Err("JSON对象未闭合".to_string());
        };
        if first == '}' {
            return Ok(Value::Object(map));
        }
        self.pos -= 1;
        loop {
            let key_val = self.next_value()?;
            let key = Self::value_to_key(key_val)?;
            let Some(sep) = self.next_clean()? else {
                return Err("JSON对象缺少分隔符".to_string());
            };
            if sep != ':' && sep != '=' {
                return Err("JSON对象键值分隔符无效".to_string());
            }
            if self.pos < self.chars.len() && self.chars[self.pos] == '>' {
                self.pos += 1;
            }
            let val = self.next_value()?;
            map.insert(key, val);
            let Some(after) = self.next_clean()? else {
                return Err("JSON对象未闭合".to_string());
            };
            match after {
                '}' => return Ok(Value::Object(map)),
                ',' | ';' => {
                    let Some(peek) = self.next_clean()? else {
                        return Err("JSON对象未闭合".to_string());
                    };
                    if peek == '}' {
                        return Ok(Value::Object(map));
                    }
                    self.pos -= 1;
                }
                _ => return Err("JSON对象结构无效".to_string()),
            }
        }
    }

    fn read_array(&mut self) -> Result<Value, String> {
        let mut arr = Vec::new();
        let mut has_trailing_separator = false;
        loop {
            let Some(peek) = self.next_clean()? else {
                return Err("JSON数组未闭合".to_string());
            };
            match peek {
                ']' => {
                    if has_trailing_separator {
                        arr.push(Value::Null);
                    }
                    return Ok(Value::Array(arr));
                }
                ',' | ';' => {
                    arr.push(Value::Null);
                    has_trailing_separator = true;
                }
                _ => {
                    self.pos -= 1;
                    has_trailing_separator = false;
                    arr.push(self.next_value()?);
                    let Some(after) = self.next_clean()? else {
                        return Err("JSON数组未闭合".to_string());
                    };
                    match after {
                        ']' => return Ok(Value::Array(arr)),
                        ',' | ';' => has_trailing_separator = true,
                        _ => return Err("JSON数组结构无效".to_string()),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_128_and_rejects_129() {
        let ok = "[".repeat(128) + &"]".repeat(128);
        assert!(require_json_depth(&ok).is_ok());
        let bad = "[".repeat(129) + &"]".repeat(129);
        assert_eq!(require_json_depth(&bad).unwrap_err(), "JSON嵌套超过128层");
    }

    #[test]
    fn handles_comments_and_bom() {
        let text = "\u{FEFF}// comment\n/* block */ {\"a\": \"[not depth]\"}";
        assert!(require_json_depth(text).is_ok());
        assert_eq!(require_json_depth("/* unclosed").unwrap_err(), "JSON注释不完整");
    }

    #[test]
    fn lenient_json_preserves_arrows_single_quotes_and_comments() {
        let content = format!(",nested:{}0{},tail:foo", "[".repeat(129), "]".repeat(129));
        for sep in [":>", "=>"] {
            for quote in ['"', '\''] {
                let text = format!(
                    "{{\"type\":\"socks\",\"server\":\"x\",\"x\"{sep}{quote}{content}{quote}}}"
                );
                let val = parse_lenient_json(&text).unwrap();
                assert_eq!(val["type"], "socks");
                assert_eq!(val["x"], content);
                assert!(val.get("nested").is_none());
            }
        }
        let brackets = format!("{}{}", "[".repeat(129), "]".repeat(129));
        let quoted = format!(
            "{{'type':'socks','server':'x','note':'{brackets}',/*comment[}}*/'path'=>\"{brackets}\"}}"
        );
        let parsed = parse_lenient_json(&quoted).unwrap();
        assert_eq!(parsed["type"], "socks");
        assert_eq!(parsed["note"], brackets);
        assert_eq!(parsed["path"], brackets);
        assert_eq!(parse_lenient_json("[]").unwrap(), serde_json::json!([]));
        assert_eq!(parse_lenient_json("[1,]").unwrap(), serde_json::json!([1, null]));
        assert_eq!(parse_lenient_json("[,1]").unwrap(), serde_json::json!([null, 1]));
        assert_eq!(parse_lenient_json("[1,,2]").unwrap(), serde_json::json!([1, null, 2]));
    }
}
