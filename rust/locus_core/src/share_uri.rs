use base64::Engine;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareUri {
    pub scheme: String,
    #[serde(rename = "rawUserInfo")]
    pub raw_user_info: Option<String>,
    pub host: String,
    pub port: i32,
    #[serde(rename = "rawPath")]
    pub raw_path: String,
    #[serde(rename = "rawQuery")]
    pub raw_query: Option<String>,
    #[serde(rename = "rawFragment")]
    pub raw_fragment: Option<String>,
}

impl ShareUri {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let Some(scheme_end) = raw.find("://") else {
            return Err("链接缺少协议".to_string());
        };
        if scheme_end == 0 {
            return Err("链接缺少协议".to_string());
        }
        let scheme = raw[..scheme_end].to_ascii_lowercase();
        let mut rest = &raw[scheme_end + 3..];
        let raw_fragment = if let Some(idx) = rest.find('#') {
            let frag = rest[idx + 1..].to_string();
            rest = &rest[..idx];
            Some(frag)
        } else {
            None
        };
        let raw_query = if let Some(idx) = rest.find('?') {
            let q = rest[idx + 1..].to_string();
            rest = &rest[..idx];
            Some(q)
        } else {
            None
        };
        let (raw_user_info, host_part) = if let Some(at) = rest.rfind('@') {
            (Some(rest[..at].to_string()), &rest[at + 1..])
        } else {
            (None, rest)
        };
        let (authority, raw_path) = if let Some(slash) = host_part.find('/') {
            (&host_part[..slash], host_part[slash..].to_string())
        } else {
            (host_part, String::new())
        };

        let (host, port_text) = if let Some(after_open) = authority.strip_prefix('[') {
            let Some(close) = after_open.find(']') else {
                return Err("IPv6 地址无效".to_string());
            };
            if close == 0 {
                return Err("IPv6 地址无效".to_string());
            }
            let h = &after_open[..close];
            let tail = &after_open[close + 1..];
            let p = if tail.is_empty() {
                None
            } else if let Some(stripped) = tail.strip_prefix(':') {
                Some(stripped)
            } else {
                return Err("端口无效".to_string());
            };
            (h.to_string(), p)
        } else if let Some(colon) = authority.rfind(':') {
            (authority[..colon].to_string(), Some(&authority[colon + 1..]))
        } else {
            (authority.to_string(), None)
        };

        if host.trim().is_empty() || host.chars().any(char::is_whitespace) {
            return Err("服务器无效".to_string());
        }
        let port = match port_text {
            Some(t) => match t.parse::<i32>() {
                Ok(n) if (1..=65535).contains(&n) => n,
                _ => return Err("端口无效".to_string()),
            },
            None => -1,
        };

        Ok(Self {
            scheme,
            raw_user_info,
            host,
            port,
            raw_path,
            raw_query,
            raw_fragment,
        })
    }

    pub fn query(&self) -> IndexMap<String, String> {
        parse_query(self.raw_query.as_deref())
    }
}

/// Exact match for Java `URLDecoder.decode(value.replace("+", "%2B"), "UTF-8")` with fallback on invalid escapes.
pub fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return value.to_string();
            }
            let Some(hi) = hex_val(bytes[i + 1]) else {
                return value.to_string();
            };
            let Some(lo) = hex_val(bytes[i + 2]) else {
                return value.to_string();
            };
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn parse_query(raw: Option<&str>) -> IndexMap<String, String> {
    let mut map = IndexMap::new();
    let Some(text) = raw else {
        return map;
    };
    for part in text.split('&') {
        if part.is_empty() {
            continue;
        }
        let (k, v) = match part.split_once('=') {
            Some((a, b)) => (a, b),
            None => (part, ""),
        };
        map.insert(percent_decode(k), percent_decode(v));
    }
    map
}

pub fn socks_user_info(raw: Option<&str>) -> String {
    let value = percent_decode(raw.unwrap_or(""));
    if value.contains(':') || value.is_empty() {
        return value;
    }
    if let Some(decoded) = try_decode_socks_b64(&value) {
        decoded
    } else {
        value
    }
}

fn try_decode_socks_b64(value: &str) -> Option<String> {
    let trimmed = value.trim_end_matches('=');
    if value.len() - trimmed.len() > 2 || trimmed.is_empty() {
        return None;
    }
    if !trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'_' | b'-'))
    {
        return None;
    }
    let normalized = value.replace('-', "+").replace('_', "/");
    let no_pad = normalized.trim_end_matches('=');
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(no_pad)
        .ok()?;
    let reencoded = base64::engine::general_purpose::STANDARD_NO_PAD.encode(&bytes);
    if reencoded != no_pad {
        return None;
    }
    let decoded = String::from_utf8(bytes).ok()?;
    if decoded.contains(':') && !decoded.chars().any(char::is_control) {
        Some(decoded)
    } else {
        None
    }
}

pub fn decode_base64_utf8(s: &str) -> Result<String, String> {
    let clean: String = s
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    let no_pad = clean.trim_end_matches('=');
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(no_pad)
        .map_err(|e| format!("Base64 解码失败: {e}"))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Go `time.ParseDuration` grammar; enforces >= 5s and <= i64::MAX ns.
pub fn hysteria_hop_interval(raw: &str) -> Result<String, String> {
    let is_plain_num = is_unsigned_decimal(raw);
    let value = if is_plain_num {
        format!("{raw}s")
    } else {
        raw.to_string()
    };
    let bytes = value.as_bytes();
    let mut offset = if bytes.first() == Some(&b'+') { 1 } else { 0 };
    if offset >= bytes.len() {
        return Err("跳跃间隔格式无效".to_string());
    }
    let mut total_nanos: i128 = 0;
    while offset < bytes.len() {
        let num_start = offset;
        let mut has_digits = false;
        while offset < bytes.len() && bytes[offset].is_ascii_digit() {
            has_digits = true;
            offset += 1;
        }
        if offset < bytes.len() && bytes[offset] == b'.' {
            offset += 1;
            let frac_start = offset;
            while offset < bytes.len() && bytes[offset].is_ascii_digit() {
                has_digits = true;
                offset += 1;
            }
            if !has_digits || (num_start == frac_start - 1 && offset == frac_start) {
                return Err("跳跃间隔格式无效".to_string());
            }
        }
        if !has_digits {
            return Err("跳跃间隔格式无效".to_string());
        }
        let num_str = &value[num_start..offset];
        let rest = &value[offset..];
        let (unit_len, mult): (usize, i128) = if rest.starts_with("ns") {
            (2, 1)
        } else if rest.starts_with("us") {
            (2, 1_000)
        } else if rest.starts_with("µs") {
            ("µs".len(), 1_000)
        } else if rest.starts_with("μs") {
            ("μs".len(), 1_000)
        } else if rest.starts_with("ms") {
            (2, 1_000_000)
        } else if rest.starts_with('s') {
            (1, 1_000_000_000)
        } else if rest.starts_with('m') {
            (1, 60_000_000_000)
        } else if rest.starts_with('h') {
            (1, 3_600_000_000_000)
        } else {
            return Err("跳跃间隔格式无效".to_string());
        };
        offset += unit_len;
        let part_ns = decimal_mul_floor(num_str, mult)?;
        total_nanos = total_nanos
            .checked_add(part_ns)
            .ok_or_else(|| "跳跃间隔超出范围".to_string())?;
    }
    if total_nanos > i64::MAX as i128 {
        return Err("跳跃间隔超出范围".to_string());
    }
    if total_nanos < 5_000_000_000 {
        return Err("跳跃间隔不能小于5秒".to_string());
    }
    Ok(value)
}

fn is_unsigned_decimal(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut dot_seen = false;
    let mut digit_seen = false;
    for b in s.bytes() {
        if b.is_ascii_digit() {
            digit_seen = true;
        } else if b == b'.' && !dot_seen {
            dot_seen = true;
        } else {
            return false;
        }
    }
    digit_seen
}

fn decimal_mul_floor(num_str: &str, mult: i128) -> Result<i128, String> {
    let (int_part, frac_part) = match num_str.split_once('.') {
        Some((a, b)) => (a, b),
        None => (num_str, ""),
    };
    let int_val: i128 = if int_part.is_empty() {
        0
    } else {
        int_part.parse::<i128>().map_err(|_| "跳跃间隔超出范围".to_string())?
    };
    let mut total = int_val
        .checked_mul(mult)
        .ok_or_else(|| "跳跃间隔超出范围".to_string())?;
    if !frac_part.is_empty() {
        // Compute floor(0.frac * mult) using exact decimal digit scaling up to 15 digits.
        let take = frac_part.len().min(15);
        let frac_digits: i128 = frac_part[..take]
            .parse::<i128>()
            .map_err(|_| "跳跃间隔格式无效".to_string())?;
        let denom = 10i128.pow(take as u32);
        let frac_ns = frac_digits
            .checked_mul(mult)
            .ok_or_else(|| "跳跃间隔超出范围".to_string())?
            / denom;
        total = total
            .checked_add(frac_ns)
            .ok_or_else(|| "跳跃间隔超出范围".to_string())?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ipv6_and_at_in_password() {
        let uri = ShareUri::parse("vless://user@pass@[2001:db8::1]:443?sni=a.com#My+Node%201").unwrap();
        assert_eq!(uri.raw_user_info.as_deref(), Some("user@pass"));
        assert_eq!(uri.host, "2001:db8::1");
        assert_eq!(uri.port, 443);
        assert_eq!(percent_decode(uri.raw_fragment.as_deref().unwrap()), "My+Node 1");
    }

    #[test]
    fn validates_hop_interval() {
        assert_eq!(hysteria_hop_interval("30").unwrap(), "30s");
        assert_eq!(hysteria_hop_interval("1m30s").unwrap(), "1m30s");
        assert_eq!(hysteria_hop_interval("4s").unwrap_err(), "跳跃间隔不能小于5秒");
    }
}
