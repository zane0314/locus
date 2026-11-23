use crate::models::{AppData, RouteRule};
use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use url::Url;

pub const BUILTIN_SMART_KEYS: &[&str] = &[
    "speed", "youtube", "telegram", "netflix", "disney", "tiktok", "x", "meta", "spotify",
    "google", "ai",
];

pub const UNSUPPORTED_SMART_RULE_TYPES: &[&str] = &["USER-AGENT", "IP-ASN", "OR"];

pub fn factory_route_rules(country: &str) -> Vec<RouteRule> {
    let mut rules = vec![
        RouteRule {
            id: 1,
            name: "屏蔽 QUIC".to_string(),
            domains: String::new(),
            packages: String::new(),
            ip_cidrs: String::new(),
            outbound: "block".to_string(),
            enabled: false,
            order: 0,
            advanced: r#"{"port":[443],"network":["udp"]}"#.to_string(),
            prioritize: true,
        },
        RouteRule {
            id: 2,
            name: "屏蔽广告".to_string(),
            domains: "geosite:category-ads-all".to_string(),
            packages: String::new(),
            ip_cidrs: String::new(),
            outbound: "block".to_string(),
            enabled: false,
            order: 1,
            advanced: "{}".to_string(),
            prioritize: true,
        },
        RouteRule {
            id: 3,
            name: "中国 Play 服务".to_string(),
            domains: "googleapis.cn".to_string(),
            packages: String::new(),
            ip_cidrs: String::new(),
            outbound: "proxy".to_string(),
            enabled: false,
            order: 2,
            advanced: "{}".to_string(),
            prioritize: true,
        },
    ];
    let mut countries = vec![("cn", "中国")];
    if country != "CN" {
        countries.push(("ir", "伊朗"));
        countries.push(("ru", "俄罗斯"));
    }
    for (code, name) in countries {
        let next_id = (rules.len() + 1) as i64;
        rules.push(RouteRule {
            id: next_id,
            name: format!("绕过{name}域名"),
            domains: format!("geosite:{code}"),
            packages: String::new(),
            ip_cidrs: String::new(),
            outbound: "direct".to_string(),
            enabled: false,
            order: 0,
            advanced: "{}".to_string(),
            prioritize: false,
        });
        let next_id = (rules.len() + 1) as i64;
        rules.push(RouteRule {
            id: next_id,
            name: format!("绕过{name} IP"),
            domains: String::new(),
            packages: String::new(),
            ip_cidrs: String::new(),
            outbound: "direct".to_string(),
            enabled: false,
            order: 0,
            advanced: format!(r#"{{"rule_set":["geoip:{code}"]}}"#),
            prioritize: false,
        });
    }
    for (idx, r) in rules.iter_mut().enumerate() {
        r.order = idx as i32;
    }
    rules
}

pub fn with_factory_route_defaults(data: &AppData, country: &str) -> Result<AppData, String> {
    if data.setting("factoryRouteDefaultsVersion") == "1" {
        return Ok(data.clone());
    }
    let mut out = data.clone();
    out.settings
        .insert("factoryRouteDefaultsVersion".into(), "1".into());
    if !out.rules.is_empty() {
        return Ok(out);
    }
    let templates = factory_route_rules(country);
    let last = data
        .nodes
        .iter()
        .map(|n| n.id)
        .chain(data.groups.iter().map(|g| g.id))
        .chain(data.merges.iter().map(|m| m.id))
        .max()
        .unwrap_or(0);
    if last > i64::MAX - templates.len() as i64 {
        return Err("记录 ID 已耗尽".to_string());
    }
    out.rules = templates
        .into_iter()
        .enumerate()
        .map(|(i, mut r)| {
            r.id = last + i as i64 + 1;
            r
        })
        .collect();
    Ok(out)
}

pub fn normalize_smart_target(value: &str) -> String {
    if value.trim().is_empty() || value.starts_with("region:") {
        "proxy".to_string()
    } else {
        value.to_string()
    }
}

pub fn smart_target(data: &AppData, key: &str) -> String {
    let setting_key = format!("smart.{key}.target");
    normalize_smart_target(data.setting_or(&setting_key, "proxy"))
}

pub fn smart_policy_keys(data: &AppData) -> Vec<String> {
    let mut available: Vec<String> = BUILTIN_SMART_KEYS.iter().map(|s| s.to_string()).collect();
    if data.settings.contains_key("smartRules.custom")
        || data.settings.contains_key("smart.custom.target")
    {
        available.push("custom".to_string());
    }
    let mut custom_keys: Vec<String> = data
        .settings
        .keys()
        .filter_map(|k| {
            k.strip_prefix("smartCustom.")
                .and_then(|r| r.strip_suffix(".name"))
                .map(str::to_string)
        })
        .collect();
    custom_keys.sort();
    available.extend(custom_keys);

    let avail_set: HashSet<&str> = available.iter().map(String::as_str).collect();
    let mut out = IndexSet::new();
    for line in data.setting("smartPolicyOrder").lines() {
        if avail_set.contains(line) {
            out.insert(line.to_string());
        }
    }
    for key in available {
        out.insert(key);
    }
    out.into_iter().collect()
}

pub fn reorder_smart_policies(data: &AppData, requested: &[String]) -> Result<AppData, String> {
    let current = smart_policy_keys(data);
    let req_set: HashSet<&String> = requested.iter().collect();
    let cur_set: HashSet<&String> = current.iter().collect();
    if req_set.len() != requested.len() || req_set != cur_set {
        return Err("分流组已变化，请重试排序".to_string());
    }
    let mut out = data.clone();
    out.settings
        .insert("smartPolicyOrder".into(), requested.join("\n"));
    Ok(out)
}

pub fn supported_builtin_smart_rules(raw: &str) -> String {
    raw.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| {
            let head = line.split(',').next().unwrap_or("").trim().to_ascii_uppercase();
            !UNSUPPORTED_SMART_RULE_TYPES.contains(&head.as_str())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn is_builtin_smart_rule_text(value: &str, bundled: &str) -> bool {
    let norm = |s: &str| s.replace("\r\n", "\n").trim().to_string();
    let nv = norm(value);
    nv == norm(bundled) || nv == norm(&supported_builtin_smart_rules(bundled))
}

pub fn rule_set_format(url: &str) -> &'static str {
    let path = Url::parse(url)
        .ok()
        .map(|u| u.path().to_string())
        .or_else(|| {
            // Mirror Java URI(url).path for relative or non-scheme URLs
            let without_frag = url.split('#').next().unwrap_or(url);
            let without_query = without_frag.split('?').next().unwrap_or(without_frag);
            Some(without_query.to_string())
        })
        .unwrap_or_default();
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".srs") {
        "binary"
    } else if lower.ends_with(".json") {
        "source"
    } else {
        ""
    }
}

pub fn rule_compatibility_warning_types(rules: &str) -> Vec<String> {
    let mut out = IndexSet::new();
    for line in rules.lines() {
        let head = line.split(',').next().unwrap_or("").trim().to_ascii_uppercase();
        if UNSUPPORTED_SMART_RULE_TYPES.contains(&head.as_str()) {
            out.insert(head);
        }
    }
    out.into_iter().collect()
}

pub fn app_packages(text: &str) -> IndexSet<String> {
    text.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn is_valid_package_name(pkg: &str) -> bool {
    if pkg.is_empty() {
        return false;
    }
    for segment in pkg.split('.') {
        let mut chars = segment.chars();
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
            _ => return false,
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
    }
    true
}

pub fn filter_vpn_packages<'a>(
    data: &AppData,
    packages: impl IntoIterator<Item = &'a str>,
) -> IndexSet<String> {
    if !data.bool_setting("perAppEnabled", None) {
        return packages.into_iter().map(str::to_string).collect();
    }
    let selected = app_packages(data.setting("perAppPackages"));
    let include = data.setting_or("perAppMode", "exclude") == "include";
    packages
        .into_iter()
        .filter(|p| selected.contains(*p) == include)
        .map(str::to_string)
        .collect()
}

pub fn effective_smart_packages(
    data: &AppData,
    key: &str,
    installed: Option<&HashSet<String>>,
) -> IndexSet<String> {
    let raw_key = format!("smartCustom.{key}.packages");
    let raw = app_packages(data.setting(&raw_key));
    let filtered = filter_vpn_packages(data, raw.iter().map(String::as_str));
    match installed {
        Some(inst) => filtered
            .into_iter()
            .filter(|p| inst.contains(p))
            .collect(),
        None => filtered,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppPaste {
    pub accepted: Vec<String>,
    pub skipped: usize,
}

pub fn parse_app_paste(
    text: &str,
    installed: &HashSet<String>,
    scope: Option<&AppData>,
) -> AppPaste {
    let entries = app_packages(text);
    let valid: IndexSet<String> = entries
        .iter()
        .filter(|p| is_valid_package_name(p) && installed.contains(*p))
        .cloned()
        .collect();
    let accepted: IndexSet<String> = match scope {
        Some(d) => filter_vpn_packages(d, valid.iter().map(String::as_str)),
        None => valid,
    };
    let skipped = entries.len().saturating_sub(accepted.len());
    AppPaste {
        accepted: accepted.into_iter().collect(),
        skipped,
    }
}

pub fn geo_asset_code(kind: &str, bytes: &[u8]) -> Result<String, String> {
    if !(16..=64 * 1024 * 1024).contains(&bytes.len()) {
        return Err("资源为空或超过 64 MiB".to_string());
    }
    if kind == "geoip" {
        let marker = b"\xab\xcd\xefMaxMind.com";
        let start = bytes.len().saturating_sub(131_072);
        let found = bytes[start..]
            .windows(marker.len())
            .any(|w| w == marker);
        if !found {
            return Err("文件不是 sing-box GeoIP 数据库".to_string());
        }
        return Ok("cn".to_string());
    }
    if kind != "geosite" || bytes[0] != 0 {
        return Err("文件不是 sing-box Geosite 数据库".to_string());
    }
    let mut pos = 1usize;
    let read_uvarint = |pos: &mut usize| -> Result<u64, String> {
        let mut res = 0u64;
        for shift in (0..=63).step_by(7) {
            if *pos >= bytes.len() {
                return Err("Geosite 元数据不完整".to_string());
            }
            let b = bytes[*pos];
            *pos += 1;
            if shift == 63 && b > 1 {
                return Err("Geosite 元数据溢出".to_string());
            }
            res |= ((b & 0x7f) as u64) << shift;
            if b < 0x80 {
                return Ok(res);
            }
        }
        Err("Geosite 元数据无效".to_string())
    };
    let count = read_uvarint(&mut pos)?;
    if !(1..=100_000).contains(&count) {
        return Err("Geosite 没有有效规则".to_string());
    }
    let len = read_uvarint(&mut pos)?;
    if !(1..=256).contains(&len) || pos + (len as usize) > bytes.len() {
        return Err("Geosite 规则名无效".to_string());
    }
    let name = std::str::from_utf8(&bytes[pos..pos + len as usize])
        .map_err(|_| "Geosite 规则名无效".to_string())?;
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'@' | b'.' | b'-'))
    {
        return Err("Geosite 规则名无效".to_string());
    }
    Ok(name.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulePreview {
    pub title: String,
    pub subtitle: String,
    pub count: Option<i32>,
    pub status: String,
    pub sample: Vec<String>,
}

pub fn build_rule_previews(
    data: &AppData,
    geo_present_geoip: bool,
    geo_present_geosite: bool,
    builtin_texts: &std::collections::HashMap<String, String>,
    installed: Option<&HashSet<String>>,
) -> Vec<RulePreview> {
    let mut out = Vec::new();
    let mut sorted_rules = data.rules.clone();
    sorted_rules.sort_by_key(|r| r.order);

    for rule in &sorted_rules {
        let mut refs = IndexSet::new();
        if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&rule.advanced) {
            match obj.get("rule_set") {
                Some(Value::Array(arr)) => {
                    for v in arr {
                        if let Some(s) = v.as_str() {
                            if !s.trim().is_empty() {
                                refs.insert(s.to_string());
                            }
                        }
                    }
                }
                Some(Value::Null) | None => {}
                Some(Value::String(s)) => {
                    if !s.trim().is_empty() {
                        refs.insert(s.clone());
                    }
                }
                Some(other) => {
                    let s = other.to_string();
                    if !s.trim().is_empty() {
                        refs.insert(s);
                    }
                }
            }
        }
        for token in rule
            .domains
            .split(|c: char| c.is_whitespace() || c == ',')
        {
            if (token.starts_with("geosite:") || token.starts_with("geoip:"))
                && !token.trim().is_empty()
            {
                refs.insert(token.to_string());
            }
        }
        for r in refs {
            let geo = r.split_once(':').map(|(k, _)| k).filter(|k| *k == "geoip" || *k == "geosite");
            let asset = match geo {
                None => "规则集引用，运行加载未验证",
                Some("geoip") => {
                    if geo_present_geoip {
                        "本地数据库存在，运行加载未验证"
                    } else {
                        "本地数据库尚未就绪"
                    }
                }
                Some(_) => {
                    if geo_present_geosite {
                        "本地数据库存在，运行加载未验证"
                    } else {
                        "本地数据库尚未就绪"
                    }
                }
            };
            out.push(RulePreview {
                title: r,
                subtitle: format!("{} · 目标：{}", rule.name, rule.outbound),
                count: None,
                status: format!(
                    "{} · {asset}",
                    if rule.enabled { "已启用" } else { "已停用" }
                ),
                sample: Vec::new(),
            });
        }
    }

    for key in smart_policy_keys(data) {
        let stored = data.settings.get(&format!("smartRules.{key}"));
        let url = data.setting(&format!("smartUrl.{key}"));
        let bundled = if url.trim().is_empty() {
            builtin_texts.get(&key).map(String::as_str).unwrap_or("")
        } else {
            ""
        };
        let text = stored.map(String::as_str).unwrap_or(bundled);
        let builtin = !bundled.trim().is_empty()
            && (stored.is_none() || stored.is_some_and(|s| is_builtin_smart_rule_text(s, bundled)));
        let lines: Vec<String> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"))
            .map(str::to_string)
            .collect();
        let source = if builtin {
            "内置规则文本已就绪"
        } else if !text.trim().is_empty() && !url.trim().is_empty() {
            "已保存远程规则正文"
        } else if !text.trim().is_empty() {
            "已保存自定义规则正文"
        } else if !url.trim().is_empty() {
            "仅配置 URL，正文未预览"
        } else if stored.is_some() {
            "规则文本为空"
        } else {
            "未配置规则文本"
        };
        let state = if smart_target(data, &key) == "off" {
            "策略关闭"
        } else if data.setting_or("routeMode", "rule") != "rule" {
            "当前路由模式不使用此策略"
        } else {
            "策略已配置，运行加载未验证"
        };
        let count = if !text.trim().is_empty() && rule_set_format(url) == "source" {
            serde_json::from_str::<Value>(text)
                .ok()
                .and_then(|v| v.get("rules").and_then(Value::as_array).map(|a| a.len() as i32))
        } else if !url.trim().is_empty() && text.trim().is_empty() {
            None
        } else {
            Some(lines.len() as i32)
        };
        let packages = effective_smart_packages(data, &key, installed);
        out.push(RulePreview {
            title: key,
            subtitle: format!(
                "智能应用规则{} · {} 个有效应用",
                if url.trim().is_empty() {
                    String::new()
                } else {
                    format!(" · {url}")
                },
                packages.len()
            ),
            count,
            status: format!("{source} · {state}"),
            sample: lines.into_iter().take(5).collect(),
        });
    }
    out
}
