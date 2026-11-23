use serde_json::Value;
use sha2::{Digest, Sha256};

/// Canonical SHA-256 identity key for a node outbound (ignores key order, `tag`, and `name`).
pub fn node_identity_key(outbound: &str) -> Result<String, String> {
    let mut value: Value =
        serde_json::from_str(outbound).map_err(|e| format!("节点 JSON 无效: {e}"))?;
    if let Value::Object(ref mut map) = value {
        map.shift_remove("tag");
        map.shift_remove("name");
    } else {
        return Err("节点 JSON 必须是对象".to_string());
    }
    let mut buf = String::new();
    write_canonical(&value, &mut buf);
    let digest = Sha256::digest(buf.as_bytes());
    let mut hex = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{b:02x}");
    }
    Ok(hex)
}

fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&canonical_number(n)),
        Value::String(s) => out.push_str(&quote_java_json_string(s)),
        Value::Array(arr) => {
            out.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&quote_java_json_string(k));
                out.push(':');
                write_canonical(&map[*k], out);
            }
            out.push('}');
        }
    }
}

fn canonical_number(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    if let Some(f) = n.as_f64() {
        if f.fract() == 0.0 && f.abs() < (i64::MAX as f64) {
            return (f as i64).to_string();
        }
        let s = f.to_string();
        if s.contains('.') {
            return s.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        return s;
    }
    n.to_string()
}

/// Match org.json.JSONObject.quote() escaping behavior.
pub fn quote_java_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    let mut prev = '\0';
    for c in s.chars() {
        match c {
            '\\' | '"' => {
                out.push('\\');
                out.push(c);
            }
            '/' => {
                if prev == '<' {
                    out.push('\\');
                }
                out.push('/');
            }
            '\u{0008}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{000c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write;
                let _ = write!(&mut out, "\\u{:04x}", c as u32);
            }
            other => out.push(other),
        }
        prev = c;
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_order_and_tag_ignored() {
        let a = r#"{"tag":"a","server":"1.1.1.1","server_port":443,"type":"vless"}"#;
        let b = r#"{"type":"vless","server_port":443.0,"server":"1.1.1.1","name":"x"}"#;
        assert_eq!(node_identity_key(a).unwrap(), node_identity_key(b).unwrap());
    }
}
