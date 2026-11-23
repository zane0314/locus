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
}
