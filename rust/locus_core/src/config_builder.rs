use crate::chain_routing::{
    chain_target_nodes, chain_unavailable_reason, derived_group_source_id,
    effective_chain_targets, raw_chain_ids, raw_route_ids, validate_chain_node_settings,
    validate_chain_target_syntax, ChainTargets,
};
use crate::models::{AppData, Group, Node};
use crate::multicore_planner::{normalize_core_kind, CORE_SING_BOX, EXPERIMENTAL_CORE_KEY};
use crate::route_math::subtract_routes as subtract_cidrs;
use crate::route_rules::{
    effective_smart_packages, is_valid_package_name, rule_compatibility_warning_types,
    rule_set_format, smart_policy_keys, smart_target, UNSUPPORTED_SMART_RULE_TYPES,
};
use crate::runtime_logic::smart_target_node_ids;
use crate::subscription_ops::apply_subscription_dns_options;
use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Main,
    Test,
    Export,
}

impl Purpose {
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_uppercase().as_str() {
            "MAIN" => Ok(Self::Main),
            "TEST" => Ok(Self::Test),
            "EXPORT" => Ok(Self::Export),
            other => Err(format!("未知构建用途: {other}")),
        }
    }
}

pub fn normalize_transport_host(outbound: &mut Map<String, Value>) {
    let Some(Value::Object(transport)) = outbound.get_mut("transport") else {
        return;
    };
    let ttype = transport
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !matches!(ttype, "httpupgrade" | "xhttp") {
        return;
    }
    if transport
        .get("host")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
    {
        return;
    }
    let host = match transport.get("headers").and_then(Value::as_object) {
        Some(headers) => match headers.get("Host") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(a)) => a
                .first()
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            _ => String::new(),
        },
        None => return,
    };
    if !host.trim().is_empty() {
        transport.insert("host".into(), Value::String(host));
        let remove_headers = if let Some(Value::Object(headers)) = transport.get_mut("headers") {
            headers.shift_remove("Host");
            headers.is_empty()
        } else {
            false
        };
        if remove_headers {
            transport.shift_remove("headers");
        }
    }
}

pub fn merge_json(base: &mut Map<String, Value>, custom: &Map<String, Value>) {
    for (raw_key, value) in custom {
        if let Value::Array(arr) = value {
            if raw_key.starts_with('+') || raw_key.ends_with('+') {
                let stripped = raw_key.strip_prefix('+').unwrap_or(raw_key);
                let key = stripped.strip_suffix('+').unwrap_or(stripped);
                let orig = base
                    .get(key)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let mut result = Vec::with_capacity(arr.len() + orig.len());
                if raw_key.starts_with('+') {
                    result.extend(arr.iter().cloned());
                    result.extend(orig);
                } else {
                    result.extend(orig);
                    result.extend(arr.iter().cloned());
                }
                base.insert(key.to_string(), Value::Array(result));
                continue;
            }
        }
        if let Value::Object(custom_sub) = value {
            if let Some(Value::Object(base_sub)) = base.get_mut(raw_key) {
                merge_json(base_sub, custom_sub);
                continue;
            }
        }
        base.insert(raw_key.clone(), value.clone());
    }
}

pub fn normalize_advanced(rule: &mut Map<String, Value>) -> Result<(), String> {
    let split_tokens = |text: &str| -> Vec<String> {
        text.split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    };

    for key in ["port", "source_port"] {
        if let Some(text) = rule.get(key).and_then(Value::as_str).map(str::to_string) {
            let mut ports: Vec<Value> = Vec::new();
            let mut ranges: Vec<Value> = Vec::new();
            for value in split_tokens(&text) {
                if value.contains(':') || value.contains('-') {
                    let norm = value.replace('-', ":");
                    let parts: Vec<&str> = norm.split(':').collect();
                    if parts.len() != 2 {
                        return Err("端口范围无效".to_string());
                    }
                    let a = parts[0]
                        .parse::<i32>()
                        .map_err(|_| "端口范围无效".to_string())?;
                    let b = parts[1]
                        .parse::<i32>()
                        .map_err(|_| "端口范围无效".to_string())?;
                    if !(1..=65535).contains(&a) || !(1..=65535).contains(&b) || a > b {
                        return Err("端口范围无效".to_string());
                    }
                    ranges.push(Value::String(format!("{a}:{b}")));
                } else {
                    let p = value
                        .parse::<i32>()
                        .ok()
                        .filter(|p| (1..=65535).contains(p))
                        .ok_or_else(|| "STATE:端口范围无效".to_string())?;
                    ports.push(Value::Number(p.into()));
                }
            }
            rule.shift_remove(key);
            if !ports.is_empty() {
                rule.insert(key.to_string(), Value::Array(ports));
            }
            if !ranges.is_empty() {
                rule.insert(format!("{key}_range"), Value::Array(ranges));
            }
        }
    }

    for key in [
        "source_ip_cidr",
        "protocol",
        "rule_set",
        "network",
        "domain",
        "domain_suffix",
        "domain_keyword",
        "domain_regex",
        "process_name",
        "process_path",
        "package_name",
    ] {
        if let Some(text) = rule.get(key).and_then(Value::as_str).map(str::to_string) {
            let values: Vec<Value> = if key == "domain_regex" {
                text.lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| Value::String(s.to_string()))
                    .collect()
            } else {
                split_tokens(&text).into_iter().map(Value::String).collect()
            };
            rule.insert(key.to_string(), Value::Array(values));
        }
    }

    if let Some(Value::Array(arr)) = rule.get("network") {
        for v in arr {
            let s = v
                .as_str()
                .ok_or_else(|| "网络须为 tcp 或 udp".to_string())?;
            if s != "tcp" && s != "udp" {
                return Err("网络须为 tcp 或 udp".to_string());
            }
        }
    }
    Ok(())
}

pub fn target_warnings(data: &AppData) -> Vec<String> {
    let sub_groups: HashSet<i64> = data
        .groups
        .iter()
        .filter(|g| g.enabled && !g.subscription_url.trim().is_empty())
        .map(|g| g.id)
        .collect();
    let empty = !data.nodes.iter().any(|n| {
        sub_groups.contains(&n.group_id)
            && serde_json::from_str::<Value>(&n.outbound)
                .ok()
                .and_then(|v| v.get("type").and_then(Value::as_str).map(|s| s != "custom"))
                .unwrap_or(true)
    });
    let mut out = Vec::new();
    for service in smart_policy_keys(data) {
        if smart_target(data, &service) == "auto" && empty {
            out.push(format!("{service}: 分流目标没有可用节点，当前使用代理"));
        }
    }
    out
}

pub fn warnings(data: &AppData) -> Vec<String> {
    let mut out = Vec::new();
    for node in &data.nodes {
        if let Some(reason) = chain_unavailable_reason(data, node) {
            out.push(format!("{}：{reason}", node.name));
        }
    }
    out.extend(target_warnings(data));
    for service in smart_policy_keys(data) {
        let rules = data.setting(&format!("smartRules.{service}"));
        for t in rule_compatibility_warning_types(rules) {
            out.push(format!(
                "{service}：此规则格式暂不支持 {t}；该类条目保留但不参与匹配，其余有效规则仍生效"
            ));
        }
    }
    out
}

fn parse_port(s: &str) -> Result<i32, String> {
    s.parse::<i32>()
        .ok()
        .filter(|p| (1..=65535).contains(p))
        .ok_or_else(|| "STATE:端口须为 1–65535 的整数".to_string())
}

fn dns_server(address: &str, tag: &str, detour: &str) -> Result<Value, String> {
    let mut obj = Map::new();
    obj.insert("tag".into(), Value::String(tag.to_string()));
    if address == "local" {
        obj.insert("type".into(), Value::String("local".to_string()));
        return Ok(Value::Object(obj));
    }
    let with_scheme = if address.contains("://") {
        address.to_string()
    } else {
        format!("udp://{address}")
    };
    let uri = Url::parse(&with_scheme).map_err(|_| "DNS 地址无效".to_string())?;
    let host = uri
        .host_str()
        .ok_or_else(|| "DNS 地址无效".to_string())?
        .to_string();
    let scheme = uri.scheme();
    if !matches!(scheme, "https" | "tls" | "tcp" | "udp") {
        return Err("DNS 协议不支持".to_string());
    }
    obj.insert("type".into(), Value::String(scheme.to_string()));
    obj.insert("server".into(), Value::String(host));
    if detour != "direct" {
        obj.insert("detour".into(), Value::String(detour.to_string()));
    }
    if let Some(p) = uri.port() {
        obj.insert("server_port".into(), Value::Number(p.into()));
    }
    if scheme == "https" {
        let raw_after = with_scheme
            .find("://")
            .map(|i| &with_scheme[i + 3..])
            .unwrap_or("");
        let has_explicit_slash = raw_after.contains('/');
        let path = if !has_explicit_slash || uri.path().trim().is_empty() {
            "/dns-query"
        } else {
            uri.path()
        };
        obj.insert("path".into(), Value::String(path.to_string()));
    }
    Ok(Value::Object(obj))
}

fn parse_hosts(text: &str) -> Result<Map<String, Value>, String> {
    if text.trim().starts_with('{') {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("Hosts JSON 无效: {e}"))?;
        return v
            .as_object()
            .cloned()
            .ok_or_else(|| "Hosts 必须是对象".to_string());
    }
    let mut map: IndexMap<String, IndexSet<String>> = IndexMap::new();
    for line in text.lines() {
        let content = line.split('#').next().unwrap_or("").trim();
        let words: Vec<&str> = content.split_whitespace().collect();
        if words.len() > 1 && !words[0].is_empty() {
            let ip = words[0];
            let valid_ip = if ip.contains(':') {
                ip.bytes().all(|b| b.is_ascii_hexdigit() || b == b':')
                    && ip.parse::<IpAddr>().is_ok()
            } else {
                let parts: Vec<&str> = ip.split('.').collect();
                parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok())
            };
            if !valid_ip {
                return Err("Hosts IP无效".to_string());
            }
            for &domain in &words[1..] {
                if domain.is_empty() || domain.contains('/') {
                    return Err("Hosts域名无效".to_string());
                }
                map.entry(domain.to_string())
                    .or_default()
                    .insert(ip.to_string());
            }
        }
    }
    let mut out = Map::new();
    for (domain, ips) in map {
        out.insert(
            domain,
            Value::Array(ips.into_iter().map(Value::String).collect()),
        );
    }
    Ok(out)
}

fn wireguard_endpoint(item: &Map<String, Value>) -> Result<Value, String> {
    if item.contains_key("address") && item.contains_key("peers") {
        return Ok(Value::Object(item.clone()));
    }
    let server = item
        .get("server")
        .and_then(Value::as_str)
        .ok_or_else(|| "WireGuard 缺少 server".to_string())?;
    let port = item
        .get("server_port")
        .and_then(Value::as_i64)
        .ok_or_else(|| "WireGuard 缺少 server_port".to_string())?;
    let pub_key = item
        .get("peer_public_key")
        .and_then(Value::as_str)
        .ok_or_else(|| "WireGuard 缺少 peer_public_key".to_string())?;
    let local_address = item
        .get("local_address")
        .cloned()
        .ok_or_else(|| "WireGuard 缺少 local_address".to_string())?;

    let mut peer = Map::new();
    peer.insert("address".into(), Value::String(server.to_string()));
    peer.insert("port".into(), Value::Number(port.into()));
    peer.insert("public_key".into(), Value::String(pub_key.to_string()));
    peer.insert("allowed_ips".into(), json!(["0.0.0.0/0", "::/0"]));
    for key in ["pre_shared_key", "reserved"] {
        if let Some(v) = item.get(key) {
            peer.insert(key.to_string(), v.clone());
        }
    }
    let mut result = item.clone();
    result.insert("address".into(), local_address);
    result.insert("peers".into(), Value::Array(vec![Value::Object(peer)]));
    for key in [
        "server",
        "server_port",
        "peer_public_key",
        "pre_shared_key",
        "reserved",
        "local_address",
    ] {
        result.shift_remove(key);
    }
    Ok(Value::Object(result))
}

fn apply_hysteria_ports(outbound: &mut Map<String, Value>, raw: &str) -> Result<(), String> {
    if raw.len() > 65536 {
        return Err("跳跃端口列表过长".to_string());
    }
    let parse_single = |v: &str| -> Result<i32, String> {
        if v.is_empty() || v.len() > 5 || !v.bytes().all(|b| b.is_ascii_digit()) {
            return Err("跳跃端口无效".to_string());
        }
        let n = v.parse::<i32>().map_err(|_| "跳跃端口无效".to_string())?;
        if !(1..=65535).contains(&n) {
            return Err("跳跃端口无效".to_string());
        }
        Ok(n)
    };
    let mut ranges: Vec<(i32, i32)> = Vec::new();
    let mut total: i64 = 0;
    for item in raw.split(',') {
        let norm = item.trim().replace('-', ":");
        let parts: Vec<&str> = norm.split(':').collect();
        if parts.is_empty() || parts.len() > 2 {
            return Err("跳跃端口无效".to_string());
        }
        let start = parse_single(parts[0])?;
        let end = if parts.len() == 2 {
            parse_single(parts[1])?
        } else {
            start
        };
        if start > end {
            return Err("跳跃端口范围倒序".to_string());
        }
        total += (end - start + 1) as i64;
        ranges.push((start, end));
    }
    if total > 65535 {
        return Err("跳跃端口总数超限".to_string());
    }
    if ranges.len() == 1 && ranges[0].0 == ranges[0].1 {
        outbound.insert("server_port".into(), Value::Number(ranges[0].0.into()));
        outbound.shift_remove("server_ports");
    } else {
        outbound.shift_remove("server_port");
        outbound.insert(
            "server_ports".into(),
            Value::Array(
                ranges
                    .into_iter()
                    .map(|(a, b)| Value::String(format!("{a}:{b}")))
                    .collect(),
            ),
        );
    }
    Ok(())
}

fn url_test_options(item: &mut Map<String, Value>, data: &AppData) -> Result<(), String> {
    let interval = data.setting("urlTestInterval");
    let interval_re = Regex::new(r"^[1-9][0-9]*(ms|s|m|h)$").unwrap();
    if !interval_re.is_match(interval) {
        return Err("URLTest 间隔无效".to_string());
    }
    let digits_end = interval.bytes().take_while(|b| b.is_ascii_digit()).count();
    let count: u64 = interval[..digits_end]
        .parse()
        .map_err(|_| "URLTest 间隔无效".to_string())?;
    let unit_ms: u64 = match &interval[digits_end..] {
        "ms" => 1,
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        _ => return Err("URLTest 间隔无效".to_string()),
    };
    let interval_ms = count
        .checked_mul(unit_ms)
        .ok_or_else(|| "URLTest 间隔无效".to_string())?;
    let tolerance = data
        .setting("urlTestTolerance")
        .parse::<i32>()
        .map_err(|_| "URLTest 容差须为 0–65535".to_string())?;
    if !(0..=65535).contains(&tolerance) {
        return Err("URLTest 容差须为 0–65535".to_string());
    }
    item.insert("url".into(), Value::String(data.setting("testUrl").to_string()));
    item.insert("interval".into(), Value::String(interval.to_string()));
    if interval_ms > 30 * 60_000 {
        item.insert("idle_timeout".into(), Value::String(interval.to_string()));
    }
    item.insert("tolerance".into(), Value::Number(tolerance.into()));
    Ok(())
}

fn parse_obj(s: &str) -> Result<Map<String, Value>, String> {
    let v: Value = serde_json::from_str(s).map_err(|e| format!("JSON 解析失败: {e}"))?;
    v.as_object()
        .cloned()
        .ok_or_else(|| "JSON 必须是对象".to_string())
}

pub fn build_config(
    data: &AppData,
    purpose: Purpose,
    test_node_id: i64,
    runtime_secret: &str,
    installed_packages: Option<&HashSet<String>>,
) -> Result<String, String> {
    validate_chain_target_syntax(data)?;
    let testing = purpose == Purpose::Test;
    let nodes_by_id: HashMap<i64, &Node> = data.nodes.iter().map(|n| (n.id, n)).collect();
    let groups_by_id: HashMap<i64, &Group> = data.groups.iter().map(|g| (g.id, g)).collect();
    let mut beans: HashMap<i64, Map<String, Value>> = HashMap::with_capacity(data.nodes.len());
    let mut metadata: HashMap<i64, Map<String, Value>> = HashMap::with_capacity(data.nodes.len());
    for n in &data.nodes {
        beans.insert(n.id, parse_obj(&n.outbound)?);
        metadata.insert(n.id, parse_obj(&n.metadata)?);
    }
    let mut sources: HashMap<(i64, String), &Node> = HashMap::new();
    for n in data.nodes.iter().rev() {
        if let Some(st) = metadata[&n.id]
            .get("sourceTag")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
        {
            sources.insert((n.group_id, st.to_string()), n);
        }
    }

    let sub_groups: HashSet<i64> = data
        .groups
        .iter()
        .filter(|g| g.enabled && !g.subscription_url.trim().is_empty())
        .map(|g| g.id)
        .collect();
    let mode = data.setting("routeMode");
    if !matches!(mode, "rule" | "global" | "direct") {
        return Err("路由模式无效".to_string());
    }
    let service_mode = data.setting("serviceMode");
    if !matches!(service_mode, "vpn" | "proxy") {
        return Err("服务模式无效".to_string());
    }
    let fake_dns_fallback = data.bool_setting("fakeDNS", Some(true));
    let fake_dns = !testing && mode != "direct" && data.bool_setting("fakeDns", Some(fake_dns_fallback));

    if !testing {
        if let Some(g) = groups_by_id.get(&data.selected_group_id()) {
            if g.enabled && derived_group_source_id(g) > 0
                && !data.nodes.iter().any(|n| n.group_id == g.id)
            {
                return Err("当前链式分组没有可用成员，请修复源分组".to_string());
            }
        }
    }

    let eligible: Vec<&Node> = if testing {
        data.nodes.iter().collect()
    } else {
        data.nodes
            .iter()
            .filter(|n| groups_by_id.get(&n.group_id).is_some_and(|g| g.enabled))
            .collect()
    };
    if eligible.is_empty() {
        return Err("没有启用分组中的可用节点".to_string());
    }
    let mut eligible_by_group: HashMap<i64, Vec<&Node>> = HashMap::new();
    for &n in &eligible {
        eligible_by_group.entry(n.group_id).or_default().push(n);
    }

    let auto = !testing && data.bool_setting("homeAutoSelect", None);
    let home_eligible: Vec<&Node> = if auto {
        eligible
            .iter()
            .copied()
            .filter(|n| beans[&n.id].get("type").and_then(Value::as_str) != Some("custom"))
            .collect()
    } else {
        eligible.clone()
    };
    if home_eligible.is_empty() {
        return Err("没有可自动选择的节点".to_string());
    }

    let chosen: &Node = if testing {
        nodes_by_id
            .get(&test_node_id)
            .copied()
            .ok_or_else(|| "STATE:测速节点不存在".to_string())?
    } else {
        home_eligible
            .iter()
            .copied()
            .find(|n| n.id == data.selected_node_id())
            .or_else(|| {
                home_eligible
                    .iter()
                    .copied()
                    .find(|n| n.group_id == data.selected_group_id())
            })
            .unwrap_or(home_eligible[0])
    };

    if beans[&chosen.id].get("type").and_then(Value::as_str) == Some("custom") {
        let cfg = beans[&chosen.id]
            .get("config")
            .and_then(Value::as_object)
            .ok_or_else(|| "自定义配置缺少 config".to_string())?;
        return custom_config(cfg, data, purpose, runtime_secret);
    }

    let nodes: Vec<&Node> = if testing {
        vec![chosen]
    } else {
        eligible.clone()
    }
    .into_iter()
    .filter(|n| beans[&n.id].get("type").and_then(Value::as_str) != Some("custom"))
    .collect();
    if nodes.is_empty() {
        return Err("没有可用节点".to_string());
    }

    let mut dynamic_chains = false;
    let mut dynamic_chain_tags: HashSet<String> = HashSet::new();
    let mut out: Vec<Value> = Vec::new();
    let mut endpoints: Vec<Value> = Vec::new();
    let mut emitted: HashSet<String> = HashSet::new();
    let mut source_node_ids: HashMap<String, i64> = HashMap::new();
    let mut raw_paths: HashMap<i64, Vec<i64>> = HashMap::new();

    let add_outbound = |mut item: Map<String, Value>,
                        out_vec: &mut Vec<Value>,
                        ep_vec: &mut Vec<Value>|
     -> Result<(), String> {
        normalize_transport_host(&mut item);
        let otype = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if otype == "hysteria2" {
            if let Some(Value::Array(ports)) = item.get("server_ports").cloned() {
                if !ports.is_empty() {
                    let joined: Vec<String> = ports
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect();
                    if joined.len() == ports.len() {
                        apply_hysteria_ports(&mut item, &joined.join(","))?;
                    }
                }
            }
        }
        if otype == "ssh" && item.contains_key("username") {
            if !item.contains_key("user") {
                if let Some(u) = item.get("username").cloned() {
                    item.insert("user".into(), u);
                }
            }
            item.shift_remove("username");
        }
        if data.bool_setting("globalAllowInsecure", None) {
            if let Some(Value::Object(tls)) = item.get_mut("tls") {
                tls.insert("insecure".into(), Value::Bool(true));
            }
        }
        if data.bool_setting("enableTLSFragment", None) {
            if let Some(Value::Object(tls)) = item.get_mut("tls") {
                if tls.get("enabled").and_then(Value::as_bool).unwrap_or(false)
                    && !tls.contains_key("fragment")
                    && !tls
                        .get("record_fragment")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                {
                    tls.insert("fragment".into(), Value::Bool(true));
                }
            }
        }
        if data.bool_setting("muxEnabled", None)
            && matches!(otype.as_str(), "shadowsocks" | "vmess" | "vless" | "trojan")
            && !item.contains_key("multiplex")
        {
            let streams = data
                .setting("muxMaxStreams")
                .parse::<i32>()
                .map_err(|_| "Mux流数量无效".to_string())?;
            if streams <= 0 {
                return Err("Mux流数量无效".to_string());
            }
            let protocol = data.setting("muxProtocol");
            if !matches!(protocol, "h2mux" | "smux" | "yamux") {
                return Err("Mux协议无效".to_string());
            }
            item.insert(
                "multiplex".into(),
                json!({
                    "enabled": true,
                    "protocol": protocol,
                    "max_streams": streams,
                    "padding": data.bool_setting("muxPadding", None),
                }),
            );
        }
        if otype == "wireguard" {
            ep_vec.push(wireguard_endpoint(&item)?);
        } else {
            out_vec.push(Value::Object(item));
        }
        Ok(())
    };

    #[allow(clippy::too_many_arguments)]
    fn emit_path(
        ids: &[i64],
        tag: &str,
        upstream: Option<&str>,
        active: &mut HashSet<i64>,
        data: &AppData,
        nodes_by_id: &HashMap<i64, &Node>,
        beans: &HashMap<i64, Map<String, Value>>,
        sources: &HashMap<(i64, String), &Node>,
        raw_paths: &mut HashMap<i64, Vec<i64>>,
        emitted: &mut HashSet<String>,
        source_node_ids: &mut HashMap<String, i64>,
        out: &mut Vec<Value>,
        endpoints: &mut Vec<Value>,
        add_outbound: &dyn Fn(Map<String, Value>, &mut Vec<Value>, &mut Vec<Value>) -> Result<(), String>,
    ) -> Result<String, String> {
        let uniq: HashSet<i64> = ids.iter().copied().collect();
        if ids.is_empty() || uniq.len() != ids.len() {
            return Err("代理链为空或节点重复".to_string());
        }
        if emitted.contains(tag) {
            return Ok(tag.to_string());
        }
        let mut previous: Option<String> = upstream.map(str::to_string);
        for (index, &id) in ids.iter().enumerate() {
            if active.len() >= 64 || !active.insert(id) {
                return Err("代理链引用循环或过深".to_string());
            }
            let n = nodes_by_id
                .get(&id)
                .copied()
                .ok_or_else(|| format!("代理链节点不存在: {id}"))?;
            let mut item = beans[&id].clone();
            let current = if index + 1 == ids.len() {
                tag.to_string()
            } else {
                format!("{tag}-chain-{}", ids.len() - 1 - index)
            };
            if let Some(ref prev) = previous {
                if item.contains_key("detour") {
                    return Err("代理链与节点原有 detour 冲突".to_string());
                }
                item.insert("detour".into(), Value::String(prev.clone()));
            } else if let Some(detour) = item
                .get("detour")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                if detour != "direct" {
                    let src = sources
                        .get(&(n.group_id, detour))
                        .copied()
                        .ok_or_else(|| "代理链原始 detour 目标不存在".to_string())?;
                    let dep = format!("{tag}-source-{id}");
                    let src_path = if let Some(p) = raw_paths.get(&src.id) {
                        p.clone()
                    } else {
                        let mut a = HashSet::new();
                        let p = raw_chain_ids(data, src.id, &mut a, false, nodes_by_id)?;
                        raw_paths.insert(src.id, p.clone());
                        p
                    };
                    emit_path(
                        &src_path,
                        &dep,
                        None,
                        active,
                        data,
                        nodes_by_id,
                        beans,
                        sources,
                        raw_paths,
                        emitted,
                        source_node_ids,
                        out,
                        endpoints,
                        add_outbound,
                    )?;
                    item.insert("detour".into(), Value::String(dep));
                }
            }
            item.insert("tag".into(), Value::String(current.clone()));
            add_outbound(item, out, endpoints)?;
            emitted.insert(current.clone());
            source_node_ids.insert(current.clone(), id);
            active.remove(&id);
            previous = Some(current);
        }
        Ok(tag.to_string())
    }

    for &n in &nodes {
        validate_chain_node_settings(data, n, &nodes_by_id)?;
        if chain_unavailable_reason(data, n).is_some() {
            let tag = format!("node-{}", n.id);
            out.push(json!({ "type": "block", "tag": tag }));
            emitted.insert(tag);
            continue;
        }
        let targets = effective_chain_targets(data, n);
        let body = if let Some(p) = raw_paths.get(&n.id) {
            p.clone()
        } else {
            let mut a = HashSet::new();
            let p = raw_chain_ids(data, n.id, &mut a, false, &nodes_by_id)?;
            raw_paths.insert(n.id, p.clone());
            p
        };
        let front_refs = chain_target_nodes(data, &targets.front, false)?;
        let mut fronts: Vec<Vec<i64>> = Vec::with_capacity(front_refs.len());
        for r in &front_refs {
            let p = if let Some(p) = raw_paths.get(&r.id) {
                p.clone()
            } else {
                let mut a = HashSet::new();
                let p = raw_chain_ids(data, r.id, &mut a, false, &nodes_by_id)?;
                raw_paths.insert(r.id, p.clone());
                p
            };
            fronts.push(p);
        }
        let landing_refs = chain_target_nodes(data, &targets.landing, false)?;
        let mut landings: Vec<Vec<i64>> = Vec::with_capacity(landing_refs.len());
        for r in &landing_refs {
            let p = if let Some(p) = raw_paths.get(&r.id) {
                p.clone()
            } else {
                let mut a = HashSet::new();
                let p = raw_chain_ids(data, r.id, &mut a, false, &nodes_by_id)?;
                raw_paths.insert(r.id, p.clone());
                p
            };
            landings.push(p);
        }

        let mut front_ids = HashSet::new();
        for f in &fronts {
            let mut a = HashSet::new();
            for id in raw_route_ids(data, f, &mut a, &nodes_by_id, false)? {
                front_ids.insert(id);
            }
        }
        let mut landing_ids = HashSet::new();
        for l in &landings {
            for &id in l {
                landing_ids.insert(id);
            }
        }
        let body_ids = if front_ids.is_empty() {
            let mut a = HashSet::new();
            raw_route_ids(data, &body, &mut a, &nodes_by_id, false)?
        } else {
            body.clone()
        };
        if body_ids
            .iter()
            .any(|id| front_ids.contains(id) || landing_ids.contains(id))
            || front_ids.intersection(&landing_ids).next().is_some()
        {
            return Err("代理链不能引用自身或重复节点".to_string());
        }

        let front_tag: Option<String> = if targets.front == "none" {
            None
        } else if targets.front.starts_with("group:") {
            dynamic_chains = true;
            let tag = format!("chain-{}-front", n.id);
            let mut candidates = Vec::with_capacity(fronts.len());
            for (i, path) in fronts.iter().enumerate() {
                let ctag = format!("{tag}-{}", front_refs[i].id);
                let mut a = HashSet::new();
                candidates.push(emit_path(
                    path,
                    &ctag,
                    None,
                    &mut a,
                    data,
                    &nodes_by_id,
                    &beans,
                    &sources,
                    &mut raw_paths,
                    &mut emitted,
                    &mut source_node_ids,
                    &mut out,
                    &mut endpoints,
                    &add_outbound,
                )?);
            }
            dynamic_chain_tags.insert(tag.clone());
            if purpose == Purpose::Export {
                return Err("自动链路需要 Links 运行时按接入位置测速，请导出固定节点链路".to_string());
            }
            let unavailable = format!("chain-{}-unavailable", n.id);
            if emitted.insert(unavailable.clone()) {
                out.push(json!({ "type": "block", "tag": unavailable }));
            }
            let mut sel_outs = candidates;
            sel_outs.push(unavailable.clone());
            out.push(json!({
                "type": "selector",
                "tag": tag,
                "outbounds": sel_outs,
                "default": unavailable,
                "restore_cache": false,
            }));
            Some(tag)
        } else {
            let rf = front_refs[0];
            let tag = if effective_chain_targets(data, rf) == ChainTargets::default() {
                format!("node-{}", rf.id)
            } else {
                format!("chain-{}-front-{}", n.id, rf.id)
            };
            let mut a = HashSet::new();
            Some(emit_path(
                &fronts[0],
                &tag,
                None,
                &mut a,
                data,
                &nodes_by_id,
                &beans,
                &sources,
                &mut raw_paths,
                &mut emitted,
                &mut source_node_ids,
                &mut out,
                &mut endpoints,
                &add_outbound,
            )?)
        };

        if targets.landing.starts_with("group:") {
            dynamic_chains = true;
            let hop_tag = format!("node-{}-hop", n.id);
            let mut a = HashSet::new();
            let hop = emit_path(
                &body,
                &hop_tag,
                front_tag.as_deref(),
                &mut a,
                data,
                &nodes_by_id,
                &beans,
                &sources,
                &mut raw_paths,
                &mut emitted,
                &mut source_node_ids,
                &mut out,
                &mut endpoints,
                &add_outbound,
            )?;
            let mut candidates = Vec::with_capacity(landings.len());
            for (i, cand) in landing_refs.iter().enumerate() {
                let ctag = format!("chain-{}-landing-{}", n.id, cand.id);
                let mut a2 = HashSet::new();
                candidates.push(emit_path(
                    &landings[i],
                    &ctag,
                    Some(&hop),
                    &mut a2,
                    data,
                    &nodes_by_id,
                    &beans,
                    &sources,
                    &mut raw_paths,
                    &mut emitted,
                    &mut source_node_ids,
                    &mut out,
                    &mut endpoints,
                    &add_outbound,
                )?);
            }
            let ntag = format!("node-{}", n.id);
            dynamic_chain_tags.insert(ntag.clone());
            if purpose == Purpose::Export {
                return Err("自动链路需要 Links 运行时按接入位置测速，请导出固定节点链路".to_string());
            }
            let unavailable = format!("chain-{}-unavailable", n.id);
            if emitted.insert(unavailable.clone()) {
                out.push(json!({ "type": "block", "tag": unavailable }));
            }
            let mut sel_outs = candidates;
            sel_outs.push(unavailable.clone());
            out.push(json!({
                "type": "selector",
                "tag": ntag,
                "outbounds": sel_outs,
                "default": unavailable,
                "restore_cache": false,
            }));
            emitted.insert(ntag);
        } else if !landings.is_empty() {
            let hop_tag = format!("node-{}-hop", n.id);
            let mut a = HashSet::new();
            let hop = emit_path(
                &body,
                &hop_tag,
                front_tag.as_deref(),
                &mut a,
                data,
                &nodes_by_id,
                &beans,
                &sources,
                &mut raw_paths,
                &mut emitted,
                &mut source_node_ids,
                &mut out,
                &mut endpoints,
                &add_outbound,
            )?;
            let mut a2 = HashSet::new();
            emit_path(
                &landings[0],
                &format!("node-{}", n.id),
                Some(&hop),
                &mut a2,
                data,
                &nodes_by_id,
                &beans,
                &sources,
                &mut raw_paths,
                &mut emitted,
                &mut source_node_ids,
                &mut out,
                &mut endpoints,
                &add_outbound,
            )?;
        } else {
            let mut a = HashSet::new();
            emit_path(
                &body,
                &format!("node-{}", n.id),
                front_tag.as_deref(),
                &mut a,
                data,
                &nodes_by_id,
                &beans,
                &sources,
                &mut raw_paths,
                &mut emitted,
                &mut source_node_ids,
                &mut out,
                &mut endpoints,
                &add_outbound,
            )?;
        }
    }

    out.push(json!({ "type": "direct", "tag": "direct" }));
    let mut tags: HashSet<String> = nodes.iter().map(|n| format!("node-{}", n.id)).collect();
    tags.insert("direct".to_string());

    if !testing {
        for g in data.groups.iter().filter(|g| g.enabled) {
            let members: Vec<String> = eligible_by_group
                .get(&g.id)
                .map(|list| {
                    list.iter()
                        .filter(|n| {
                            beans[&n.id].get("type").and_then(Value::as_str) != Some("custom")
                        })
                        .map(|n| format!("node-{}", n.id))
                        .collect()
                })
                .unwrap_or_default();
            if !members.is_empty() {
                let opts = parse_obj(&g.options).unwrap_or_default();
                let selectable = opts
                    .get("groupIsSelector")
                    .and_then(Value::as_bool)
                    .or_else(|| opts.get("isSelector").and_then(Value::as_bool))
                    .unwrap_or(true);
                let gtag = format!("group-{}", g.id);
                let saved = data.setting_or(&format!("groupSelection.{gtag}"), "");
                let chosen_tag = format!("node-{}", chosen.id);
                let group_default = if selectable && members.iter().any(|m| m == saved) {
                    saved.to_string()
                } else if members.contains(&chosen_tag) {
                    chosen_tag
                } else {
                    members[0].clone()
                };
                let sel_members = if selectable {
                    members
                } else {
                    vec![group_default.clone()]
                };
                out.push(json!({
                    "type": "selector",
                    "tag": gtag,
                    "outbounds": sel_members,
                    "default": group_default,
                }));
                tags.insert(gtag);
            }
        }
        for mg in &data.merges {
            if !matches!(mg.mode.as_str(), "selector" | "urltest") {
                return Err("合并组模式无效".to_string());
            }
            let mut members_set = IndexSet::new();
            for &n in &eligible {
                if (mg.node_ids.contains(&n.id) || mg.group_ids.contains(&n.group_id))
                    && beans[&n.id].get("type").and_then(Value::as_str) != Some("custom")
                {
                    members_set.insert(format!("node-{}", n.id));
                }
            }
            let members: Vec<String> = members_set.into_iter().collect();
            if members.is_empty() {
                continue;
            }
            let mtag = format!("merge-{}", mg.id);
            let mut item = Map::new();
            item.insert("type".into(), Value::String(mg.mode.clone()));
            item.insert("tag".into(), Value::String(mtag.clone()));
            item.insert(
                "outbounds".into(),
                Value::Array(members.iter().cloned().map(Value::String).collect()),
            );
            if mg.mode == "urltest" {
                url_test_options(&mut item, data)?;
            } else {
                let sel_tag = format!("node-{}", mg.selected_id);
                if members.contains(&sel_tag) {
                    item.insert("default".into(), Value::String(sel_tag));
                }
            }
            out.push(Value::Object(item));
            tags.insert(mtag);
        }
    }

    let selector_group = data.groups.iter().find(|g| {
        g.id == chosen.group_id && {
            let opts = parse_obj(&g.options).unwrap_or_default();
            opts.get("groupIsSelector")
                .and_then(Value::as_bool)
                .or_else(|| opts.get("isSelector").and_then(Value::as_bool))
                .unwrap_or(false)
        }
    });
    let candidate_nodes: Vec<&Node> = if testing {
        nodes
            .iter()
            .copied()
            .filter(|n| n.id == test_node_id)
            .collect()
    } else {
        eligible
            .iter()
            .copied()
            .filter(|n| beans[&n.id].get("type").and_then(Value::as_str) != Some("custom"))
            .collect()
    };
    if candidate_nodes.is_empty() {
        return Err("没有可用代理候选".to_string());
    }
    let selected = format!("node-{}", if testing { test_node_id } else { chosen.id });
    let default_node = if tags.contains(&selected) {
        selected
    } else {
        format!("node-{}", candidate_nodes[0].id)
    };
    if auto {
        let outs: Vec<Value> = candidate_nodes
            .iter()
            .map(|n| Value::String(format!("node-{}", n.id)))
            .collect();
        let mut item = Map::new();
        item.insert("type".into(), Value::String("urltest".to_string()));
        item.insert("tag".into(), Value::String("home-auto".to_string()));
        item.insert("outbounds".into(), Value::Array(outs));
        url_test_options(&mut item, data)?;
        out.push(Value::Object(item));
    }
    let mut proxy_outs: Vec<String> = Vec::new();
    if auto {
        proxy_outs.push("home-auto".to_string());
    }
    for &n in &candidate_nodes {
        if auto || selector_group.is_none() || selector_group.is_some_and(|g| n.group_id == g.id) {
            proxy_outs.push(format!("node-{}", n.id));
        }
    }
    out.push(json!({
        "type": "selector",
        "tag": "proxy",
        "outbounds": proxy_outs,
        "default": if auto { "home-auto" } else { default_node.as_str() },
    }));
    tags.insert("proxy".to_string());

    let mut rules: Vec<Value> = Vec::new();
    if !testing {
        if data.bool_setting("sniff", None) {
            rules.push(json!({ "action": "sniff" }));
        }
        rules.push(json!({ "protocol": "dns", "action": "hijack-dns" }));
        if data.bool_setting("resolveDestination", None) {
            rules.push(json!({ "action": "resolve", "server": "dns-direct" }));
        }
        if data.bool_setting("bypassLanInCore", None) {
            rules.push(json!({ "ip_is_private": true, "action": "route", "outbound": "direct" }));
        }
    }

    let mut dns_rules: Vec<Value> = Vec::new();
    let mut priority_rules: Vec<Value> = Vec::new();
    let mut ordinary_rules: Vec<Value> = Vec::new();
    let mut priority_dns_rules: Vec<Value> = Vec::new();
    let mut ordinary_dns_rules: Vec<Value> = Vec::new();
    let mut route_dns_servers: IndexMap<String, Value> = IndexMap::new();

    let mut route_dns_rule = |rule: &Map<String, Value>| -> Result<Option<Value>, String> {
        if !data.bool_setting("enableDnsRouting", None) {
            return Ok(None);
        }
        let action = rule
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("route");
        let dest = match action {
            "reject" => "block",
            "route" => match rule
                .get("outbound")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                Some(d) => d,
                None => return Ok(None),
            },
            _ => return Ok(None),
        };
        let allowed = [
            "domain",
            "domain_suffix",
            "domain_keyword",
            "domain_regex",
            "rule_set",
            "action",
            "outbound",
        ];
        if rule.keys().any(|k| !allowed.contains(&k.as_str())) {
            return Ok(None);
        }
        if let Some(Value::Array(refs)) = rule.get("rule_set") {
            if !refs.is_empty()
                && refs
                    .iter()
                    .all(|r| r.as_str().is_some_and(|s| s.starts_with("geoip:")))
            {
                return Ok(None);
            }
        }
        let mut m = Map::new();
        for key in ["domain", "domain_suffix", "domain_keyword", "domain_regex"] {
            if let Some(v) = rule.get(key) {
                m.insert(key.to_string(), v.clone());
            }
        }
        if let Some(Value::Array(refs)) = rule.get("rule_set") {
            let domains: Vec<Value> = refs
                .iter()
                .filter(|r| !r.as_str().is_some_and(|s| s.starts_with("geoip:")))
                .cloned()
                .collect();
            if !domains.is_empty() {
                m.insert("rule_set".into(), Value::Array(domains));
            }
        }
        if m.is_empty() {
            return Ok(None);
        }
        if dest == "block" {
            m.insert("action".into(), Value::String("reject".to_string()));
            return Ok(Some(Value::Object(m)));
        }
        let server = if dest == "direct" {
            "dns-direct".to_string()
        } else {
            let tag = format!("dns-route-{dest}");
            if !route_dns_servers.contains_key(&tag) {
                let srv = dns_server(data.setting("dnsRemote"), &tag, dest)?;
                route_dns_servers.insert(tag.clone(), srv);
            }
            tag
        };
        m.insert("action".into(), Value::String("route".to_string()));
        m.insert("server".into(), Value::String(server));
        Ok(Some(Value::Object(m)))
    };

    let mut sets: IndexMap<String, Value> = IndexMap::new();
    let resolve_target = |value: &str, tags_set: &HashSet<String>| -> Result<String, String> {
        let resolved = if matches!(value, "proxy" | "direct" | "block") {
            value.to_string()
        } else if let Some(rest) = value.strip_prefix("node:") {
            format!("node-{rest}")
        } else if let Some(rest) = value.strip_prefix("group:") {
            format!("group-{rest}")
        } else if let Some(rest) = value.strip_prefix("merge:") {
            format!("merge-{rest}")
        } else {
            return Err(format!("未知规则目标 {value}"));
        };
        if resolved != "block" && !tags_set.contains(&resolved) {
            return Err(format!("规则引用不存在的目标 {value}"));
        }
        Ok(resolved)
    };

    let code_re = Regex::new(r"^[a-zA-Z0-9_-]+$").unwrap();
    if !testing && mode == "rule" {
        let mut enabled_rules: Vec<_> = data.rules.iter().filter(|r| r.enabled).collect();
        enabled_rules.sort_by_key(|r| r.order);
        for r in enabled_rules {
            let mut rule = parse_obj(&r.advanced)?;
            normalize_advanced(&mut rule)?;
            let custom_rule = rule
                .shift_remove("customRule")
                .and_then(|v| v.as_object().cloned());
            if rule.contains_key("outbound") {
                return Err("高级规则 outbound 必须使用统一目标".to_string());
            }
            let mut domains: Vec<String> = Vec::new();
            let mut suffixes: Vec<String> = Vec::new();
            let mut keywords: Vec<String> = Vec::new();
            let mut regexes: Vec<String> = Vec::new();
            let mut set_tags: Vec<String> = rule
                .get("rule_set")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            for value in &set_tags {
                if value.starts_with("geosite:") || value.starts_with("geoip:") {
                    let (kind, code) = value.split_once(':').unwrap();
                    if !code_re.is_match(code) {
                        return Err("规则集名称无效".to_string());
                    }
                    sets.insert(
                        value.clone(),
                        json!({
                            "type": "local",
                            "tag": value,
                            "format": "binary",
                            "path": format!("{kind}:{code}"),
                        }),
                    );
                }
            }
            for line in r.domains.lines() {
                for piece in line.split(',') {
                    let value = piece.trim();
                    if value.is_empty() {
                        continue;
                    }
                    if value.starts_with("geosite:") || value.starts_with("geoip:") {
                        let (kind, code) = value.split_once(':').unwrap();
                        if !code_re.is_match(code) {
                            return Err("规则集名称无效".to_string());
                        }
                        sets.insert(
                            value.to_string(),
                            json!({
                                "type": "local",
                                "tag": value,
                                "format": "binary",
                                "path": format!("{kind}:{code}"),
                            }),
                        );
                        set_tags.push(value.to_string());
                    } else if let Some(rest) = value.strip_prefix("full:") {
                        domains.push(rest.to_string());
                    } else if let Some(rest) = value.strip_prefix("regexp:") {
                        regexes.push(rest.to_string());
                    } else if let Some(rest) = value.strip_prefix("keyword:") {
                        keywords.push(rest.to_string());
                    } else {
                        let s = value.strip_prefix("domain:").unwrap_or(value);
                        let s = s.strip_prefix("+.").unwrap_or(s);
                        suffixes.push(s.to_string());
                    }
                }
            }
            if !domains.is_empty() {
                rule.insert("domain".into(), json!(domains));
            }
            if !suffixes.is_empty() {
                rule.insert("domain_suffix".into(), json!(suffixes));
            }
            if !keywords.is_empty() {
                rule.insert("domain_keyword".into(), json!(keywords));
            }
            if !regexes.is_empty() {
                rule.insert("domain_regex".into(), json!(regexes));
            }
            if !set_tags.is_empty() {
                let uniq: IndexSet<String> = set_tags.into_iter().collect();
                rule.insert(
                    "rule_set".into(),
                    Value::Array(uniq.into_iter().map(Value::String).collect()),
                );
            }
            let split_ws = |s: &str| -> Vec<String> {
                s.split(|c: char| c.is_whitespace() || c == ',')
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .collect()
            };
            let pkgs = split_ws(&r.packages);
            if !pkgs.is_empty() {
                rule.insert("package_name".into(), json!(pkgs));
            }
            let cidrs = split_ws(&r.ip_cidrs);
            if !cidrs.is_empty() {
                rule.insert("ip_cidr".into(), json!(cidrs));
            }
            if rule.is_empty() && custom_rule.is_none() {
                return Err(format!("规则 {} 没有匹配条件", r.name));
            }
            let dest = resolve_target(&r.outbound, &tags)?;
            if dest == "block" {
                rule.insert("action".into(), Value::String("reject".to_string()));
            } else if rule
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("route")
                == "route"
            {
                rule.insert("action".into(), Value::String("route".to_string()));
                rule.insert("outbound".into(), Value::String(dest));
            }
            if let Some(cr) = custom_rule {
                merge_json(&mut rule, &cr);
            }
            let dns_r = route_dns_rule(&rule)?;
            if r.prioritize {
                priority_rules.push(Value::Object(rule));
                if let Some(dr) = dns_r {
                    priority_dns_rules.push(dr);
                }
            } else {
                ordinary_rules.push(Value::Object(rule));
                if let Some(dr) = dns_r {
                    ordinary_dns_rules.push(dr);
                }
            }
        }
    }

    rules.extend(priority_rules);
    dns_rules.extend(priority_dns_rules.iter().cloned());

    if !testing && mode == "rule" {
        for service in smart_policy_keys(data) {
            let choice = smart_target(data, &service);
            if choice == "off" {
                continue;
            }
            let dest = if choice == "auto" {
                let cands: Vec<&Node> = candidate_nodes
                    .iter()
                    .copied()
                    .filter(|n| sub_groups.contains(&n.group_id))
                    .collect();
                if cands.is_empty() {
                    continue;
                }
                let tag = format!("smart-{service}");
                let mut ut = Map::new();
                ut.insert("type".into(), Value::String("urltest".to_string()));
                ut.insert("tag".into(), Value::String(tag.clone()));
                ut.insert(
                    "outbounds".into(),
                    Value::Array(
                        cands
                            .iter()
                            .map(|n| Value::String(format!("node-{}", n.id)))
                            .collect(),
                    ),
                );
                url_test_options(&mut ut, data)?;
                out.push(Value::Object(ut));
                tags.insert(tag.clone());
                tag
            } else if let Some(rest) = choice.strip_prefix("group:") {
                let Ok(gid) = rest.parse::<i64>() else {
                    continue;
                };
                if gid <= 0 {
                    continue;
                }
                let members: Vec<String> = eligible_by_group
                    .get(&gid)
                    .map(|list| {
                        list.iter()
                            .filter(|n| {
                                beans[&n.id].get("type").and_then(Value::as_str) != Some("custom")
                            })
                            .map(|n| format!("node-{}", n.id))
                            .collect()
                    })
                    .unwrap_or_default();
                if members.is_empty() {
                    continue;
                }
                let tag = format!("smart-group-{gid}");
                if !tags.contains(&tag) {
                    let mut ut = Map::new();
                    ut.insert("type".into(), Value::String("urltest".to_string()));
                    ut.insert("tag".into(), Value::String(tag.clone()));
                    ut.insert(
                        "outbounds".into(),
                        Value::Array(members.into_iter().map(Value::String).collect()),
                    );
                    url_test_options(&mut ut, data)?;
                    out.push(Value::Object(ut));
                    tags.insert(tag.clone());
                }
                tag
            } else {
                if let Some(rest) = choice.strip_prefix("node:") {
                    if !candidate_nodes.iter().any(|n| n.id.to_string() == rest) {
                        continue;
                    }
                }
                match resolve_target(&choice, &tags) {
                    Ok(d) => d,
                    Err(_) => continue,
                }
            };

            let mut add_smart = |mut rule: Map<String, Value>| -> Result<(), String> {
                if dest == "block" {
                    rule.insert("action".into(), Value::String("reject".to_string()));
                } else {
                    rule.insert("action".into(), Value::String("route".to_string()));
                    rule.insert("outbound".into(), Value::String(dest.clone()));
                }
                if let Some(dr) = route_dns_rule(&rule)? {
                    dns_rules.push(dr);
                }
                rules.push(Value::Object(rule));
                Ok(())
            };

            let packages: Vec<String> = effective_smart_packages(data, &service, installed_packages)
                .into_iter()
                .collect();
            if !packages.is_empty() {
                if !packages.iter().all(|p| is_valid_package_name(p)) {
                    return Err("自定义应用包名无效".to_string());
                }
                let mut app_rule = Map::new();
                app_rule.insert("package_name".into(), json!(packages));
                add_smart(app_rule)?;
            }

            let smart_url = data.setting(&format!("smartUrl.{service}"));
            let format = rule_set_format(smart_url);
            if !format.is_empty() {
                let tag = format!("smart-set-{service}");
                let body = data.setting(&format!("smartRules.{service}"));
                let set = if format == "source" && !body.trim().is_empty() {
                    let body_obj = parse_obj(body)?;
                    let rs = body_obj
                        .get("rules")
                        .cloned()
                        .ok_or_else(|| "规则集 JSON 缺少 rules".to_string())?;
                    json!({ "type": "inline", "tag": tag, "rules": rs })
                } else {
                    let base_url = smart_url.split('#').next().unwrap_or(smart_url);
                    let updated = data.setting_or(&format!("smartUpdated.{service}"), "0");
                    let url = format!("{base_url}#zanebox-update={updated}");
                    let interval = match data.setting("rulesUpdateInterval") {
                        "off" => "876000h",
                        "3d" => "72h",
                        "7d" => "168h",
                        other => other,
                    };
                    json!({
                        "type": "remote",
                        "tag": tag,
                        "format": format,
                        "url": url,
                        "download_detour": "direct",
                        "update_interval": interval,
                    })
                };
                sets.insert(tag.clone(), set);
                let mut set_rule = Map::new();
                set_rule.insert("rule_set".into(), json!([tag]));
                add_smart(set_rule)?;
                continue;
            }

            let mut grouped: IndexMap<&'static str, Map<String, Value>> = IndexMap::new();
            let raw_rules = data.setting_or(&format!("smartRules.{service}"), "");
            for line in raw_rules.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                    continue;
                }
                let head = trimmed
                    .split(',')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_uppercase();
                if UNSUPPORTED_SMART_RULE_TYPES.contains(&head.as_str()) {
                    continue;
                }
                let fields: Vec<&str> = trimmed.split(',').map(str::trim).collect();
                let kind = match head.as_str() {
                    "IP-CIDR" | "IP-CIDR6" => "ip",
                    "PROCESS-NAME" => "process",
                    _ => "domain",
                };
                let rule = grouped.entry(kind).or_default();
                let val = fields.get(1).copied().unwrap_or(fields[0]);
                let push = |rule: &mut Map<String, Value>, k: &str, v: &str| {
                    let arr = rule
                        .entry(k.to_string())
                        .or_insert_with(|| Value::Array(Vec::new()));
                    if let Value::Array(vec) = arr {
                        vec.push(Value::String(v.to_string()));
                    }
                };
                match head.as_str() {
                    "DOMAIN" => push(rule, "domain", val),
                    "DOMAIN-SUFFIX" => push(rule, "domain_suffix", val),
                    "DOMAIN-KEYWORD" => push(rule, "domain_keyword", val),
                    "IP-CIDR" | "IP-CIDR6" => push(rule, "ip_cidr", val),
                    "PROCESS-NAME" => push(rule, "process_name", val),
                    _ => {
                        if fields.len() != 1 {
                            return Err(format!("智能规则格式不支持: {}", fields[0]));
                        }
                        push(rule, "domain_suffix", val);
                    }
                }
            }
            for (_, g_rule) in grouped {
                add_smart(g_rule)?;
            }
        }
    }

    rules.extend(ordinary_rules);
    dns_rules.extend(ordinary_dns_rules);

    let final_tag = if testing {
        default_node.clone()
    } else if mode == "direct" {
        "direct".to_string()
    } else {
        "proxy".to_string()
    };

    let rules_text = Value::Array(rules.clone()).to_string();
    let find_process = !testing
        && (data.bool_setting("statsEnabled", None)
            || [
                "package_name",
                "process_name",
                "process_path",
                "process_path_regex",
            ]
            .iter()
            .any(|k| rules_text.contains(&format!("\"{k}\""))));

    let mut route = Map::new();
    route.insert("rules".into(), Value::Array(rules));
    route.insert("final".into(), Value::String(final_tag.clone()));
    route.insert("auto_detect_interface".into(), Value::Bool(true));
    route.insert("find_process".into(), Value::Bool(find_process));
    route.insert(
        "default_domain_resolver".into(),
        Value::String("dns-direct".to_string()),
    );
    if data.bool_setting("concurrentDial", None) {
        route.insert(
            "default_network_strategy".into(),
            Value::String("fallback".to_string()),
        );
    }
    let dom_strat = data.setting("domainStrategy");
    if !dom_strat.trim().is_empty() {
        if !matches!(
            dom_strat,
            "prefer_ipv4" | "prefer_ipv6" | "ipv4_only" | "ipv6_only"
        ) {
            return Err("域名解析策略无效".to_string());
        }
        route.insert(
            "default_domain_resolver".into(),
            json!({ "server": "dns-direct", "strategy": dom_strat }),
        );
    }
    let server_strategy = data.setting_or("dnsStrategyServer", data.setting_or("domainStrategy", ""));
    if !matches!(
        server_strategy,
        "" | "prefer_ipv4" | "prefer_ipv6" | "ipv4_only" | "ipv6_only"
    ) {
        return Err("节点服务器DNS策略无效".to_string());
    }
    if !server_strategy.trim().is_empty() {
        route.insert(
            "default_domain_resolver".into(),
            json!({ "server": "dns-direct", "strategy": server_strategy }),
        );
    }
    if !sets.is_empty() {
        route.insert(
            "rule_set".into(),
            Value::Array(sets.into_values().collect()),
        );
    }

    let mut dns_servers: Vec<Value> = vec![
        dns_server(data.setting("dnsDirect"), "dns-direct", "direct")?,
        dns_server(data.setting("dnsRemote"), "dns-remote", &final_tag)?,
    ];
    for (_, srv) in route_dns_servers {
        dns_servers.push(srv);
    }

    let hosts = parse_hosts(data.setting_or("dnsHosts", data.setting_or("hosts", "")))?;
    if !hosts.is_empty() {
        let host_keys: Vec<Value> = hosts.keys().cloned().map(Value::String).collect();
        dns_servers.push(json!({
            "type": "hosts",
            "tag": "dns-hosts",
            "predefined": Value::Object(hosts),
        }));
        let mut next_dns = vec![json!({
            "domain": host_keys,
            "action": "route",
            "server": "dns-hosts",
        })];
        next_dns.extend(dns_rules);
        dns_rules = next_dns;
    }

    let ipv6 = data.bool_setting("ipv6", Some(false));
    if fake_dns {
        let mut fake = Map::new();
        fake.insert("type".into(), Value::String("fakeip".to_string()));
        fake.insert("tag".into(), Value::String("dns-fake".to_string()));
        fake.insert(
            "inet4_range".into(),
            Value::String("198.18.0.0/15".to_string()),
        );
        if ipv6 {
            fake.insert("inet6_range".into(), Value::String("fc00::/18".to_string()));
        }
        dns_servers.push(Value::Object(fake));
        let mut fake_inbounds: Vec<Value> = Vec::new();
        if service_mode == "vpn" {
            fake_inbounds.push(Value::String("tun-in".to_string()));
        }
        if service_mode == "proxy" || !data.bool_setting("disableMixedInbound", None) {
            fake_inbounds.push(Value::String("mixed-in".to_string()));
        }
        if data.bool_setting("shareEnabled", None) {
            fake_inbounds.push(Value::String("share-in".to_string()));
        }
        let qtypes: Vec<Value> = if ipv6 {
            vec![Value::String("A".into()), Value::String("AAAA".into())]
        } else {
            vec![Value::String("A".into())]
        };
        let fake_rule = json!({
            "inbound": fake_inbounds,
            "query_type": qtypes,
            "action": "route",
            "server": "dns-fake",
        });
        let priority_strs: HashSet<String> =
            priority_dns_rules.iter().map(Value::to_string).collect();
        let mut ordered = Vec::new();
        for rule in dns_rules {
            let is_route = rule.get("action").and_then(Value::as_str) == Some("route");
            let server = rule.get("server").and_then(Value::as_str).unwrap_or("");
            if is_route && server != "dns-hosts" && !priority_strs.contains(&rule.to_string()) {
                if let Value::Object(mut addr) = rule.clone() {
                    addr.insert("server".into(), Value::String("dns-fake".to_string()));
                    addr.insert("inbound".into(), fake_rule["inbound"].clone());
                    addr.insert("query_type".into(), fake_rule["query_type"].clone());
                    addr.insert(
                        "strategy".into(),
                        Value::String(if server == "dns-direct" {
                            "direct".to_string()
                        } else {
                            "remote".to_string()
                        }),
                    );
                    ordered.push(Value::Object(addr));
                }
            }
            ordered.push(rule);
        }
        ordered.push(fake_rule);
        dns_rules = ordered;
    }

    let dns_strategy = data.setting_or("dnsStrategy", if ipv6 { "prefer_ipv4" } else { "ipv4_only" });
    if !matches!(
        dns_strategy,
        "" | "prefer_ipv4" | "prefer_ipv6" | "ipv4_only" | "ipv6_only"
    ) {
        return Err("DNS策略无效".to_string());
    }
    let direct_strategy = data.setting_or("dnsStrategyDirect", dns_strategy);
    let remote_strategy = data.setting_or("dnsStrategyRemote", dns_strategy);
    for s in [direct_strategy, remote_strategy] {
        if !matches!(
            s,
            "" | "prefer_ipv4" | "prefer_ipv6" | "ipv4_only" | "ipv6_only"
        ) {
            return Err("直连/远程DNS策略无效".to_string());
        }
    }

    let mut filtered_dns_rules: Vec<Value> = Vec::new();
    let push_family_filter = |dest: &mut Vec<Value>, match_obj: &Map<String, Value>, strat: &str| {
        if strat != "ipv4_only" && strat != "ipv6_only" {
            return;
        }
        let mut rej = match_obj.clone();
        for k in ["server", "action", "strategy"] {
            rej.shift_remove(k);
        }
        rej.insert(
            "query_type".into(),
            json!([if strat == "ipv4_only" { "AAAA" } else { "A" }]),
        );
        rej.insert("action".into(), Value::String("predefined".to_string()));
        rej.insert("rcode".into(), Value::String("NOERROR".to_string()));
        dest.push(Value::Object(rej));
    };

    let final_strategy = if final_tag == "direct" {
        direct_strategy
    } else {
        remote_strategy
    };
    let mut family_applied = false;
    for mut rule in dns_rules {
        if let Value::Object(ref mut obj) = rule {
            let srv = obj
                .get("server")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if srv == "dns-direct" {
                push_family_filter(&mut filtered_dns_rules, obj, direct_strategy);
            }
            if srv.starts_with("dns-route-") || srv == "dns-remote" {
                push_family_filter(&mut filtered_dns_rules, obj, remote_strategy);
            }
            if srv == "dns-fake" {
                let strat = match obj.get("strategy").and_then(Value::as_str).unwrap_or("") {
                    "direct" => direct_strategy,
                    "remote" => remote_strategy,
                    _ => final_strategy,
                };
                push_family_filter(&mut filtered_dns_rules, obj, strat);
                family_applied = true;
            }
            obj.shift_remove("strategy");
        }
        filtered_dns_rules.push(rule);
    }
    if !family_applied {
        push_family_filter(&mut filtered_dns_rules, &Map::new(), final_strategy);
    }

    let dns = json!({
        "servers": dns_servers,
        "reverse_mapping": true,
        "rules": filtered_dns_rules,
        "final": if final_tag == "direct" { "dns-direct" } else { "dns-remote" },
        "strategy": final_strategy,
    });

    let mut root = Map::new();
    root.insert("log".into(), json!({ "level": data.setting("logLevel") }));
    root.insert("outbounds".into(), Value::Array(out));
    root.insert("route".into(), Value::Object(route));
    root.insert("dns".into(), dns);

    let mut inbound: Vec<Value> = Vec::new();
    if !testing {
        let mut addresses = vec!["172.19.0.1/30".to_string()];
        if ipv6 {
            addresses.push("fdfe:dcba:9876::1/126".to_string());
        }
        if service_mode == "vpn" {
            let mtu = data
                .setting("mtu")
                .parse::<i32>()
                .map_err(|_| "MTU 无效".to_string())?;
            let mut tun = Map::new();
            tun.insert("type".into(), Value::String("tun".to_string()));
            tun.insert("tag".into(), Value::String("tun-in".to_string()));
            tun.insert("address".into(), json!(addresses));
            tun.insert("mtu".into(), Value::Number(mtu.into()));
            tun.insert("stack".into(), Value::String(data.setting("tunStack").to_string()));
            tun.insert("auto_route".into(), Value::Bool(true));
            tun.insert(
                "strict_route".into(),
                Value::Bool(data.bool_setting("strictRoute", None)),
            );
            if data.bool_setting("perAppEnabled", None) {
                let pkgs: IndexSet<String> = data
                    .setting("perAppPackages")
                    .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                if pkgs.is_empty() {
                    return Err("应用分流列表为空".to_string());
                }
                if !pkgs.iter().all(|p| is_valid_package_name(p)) {
                    return Err("应用包名无效".to_string());
                }
                let app_mode = data.setting_or("perAppMode", "exclude");
                if !matches!(app_mode, "include" | "exclude") {
                    return Err("应用分流模式无效".to_string());
                }
                let key = if app_mode == "include" {
                    "include_package"
                } else {
                    "exclude_package"
                };
                tun.insert(
                    key.into(),
                    Value::Array(pkgs.into_iter().map(Value::String).collect()),
                );
            }
            if data.bool_setting("bypassLan", None) {
                let base = vec![
                    "10.0.0.0/8".to_string(),
                    "172.16.0.0/12".to_string(),
                    "192.168.0.0/16".to_string(),
                    "169.254.0.0/16".to_string(),
                    "fc00::/7".to_string(),
                    "fe80::/10".to_string(),
                ];
                let mut excluded = addresses.clone();
                if fake_dns && ipv6 {
                    excluded.push("fc00::/18".to_string());
                }
                let result = subtract_cidrs(&base, &excluded)?;
                tun.insert("route_exclude_address".into(), json!(result));
            }
            inbound.push(Value::Object(tun));
        }
        if service_mode == "proxy" || !data.bool_setting("disableMixedInbound", None) {
            inbound.push(json!({
                "type": "mixed",
                "tag": "mixed-in",
                "listen": if data.bool_setting("allowLan", None) { "0.0.0.0" } else { "127.0.0.1" },
                "listen_port": parse_port(data.setting("mixedPort"))?,
            }));
        }
        if data.bool_setting("shareEnabled", None) {
            inbound.push(json!({
                "type": "mixed",
                "tag": "share-in",
                "listen": "0.0.0.0",
                "listen_port": parse_port(data.setting("sharePort"))?,
            }));
        }
        if data.bool_setting("clashApi", None)
            || data.bool_setting("statsEnabled", None)
            || auto
            || dynamic_chains
        {
            let mut api = Map::new();
            api.insert(
                "external_controller".into(),
                Value::String(format!("127.0.0.1:{}", parse_port(data.setting("apiPort"))?)),
            );
            if purpose == Purpose::Main && !runtime_secret.trim().is_empty() {
                api.insert("secret".into(), Value::String(runtime_secret.to_string()));
            }
            root.insert(
                "experimental".into(),
                json!({ "clash_api": Value::Object(api) }),
            );
        }
        let exp = root
            .entry("experimental".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(exp_obj) = exp {
            exp_obj.insert(
                "cache_file".into(),
                json!({
                    "enabled": true,
                    "path": "../cache/cache.db",
                    "store_fakeip": true,
                }),
            );
        }
    }

    if !endpoints.is_empty() {
        root.insert("endpoints".into(), Value::Array(endpoints));
    }
    root.insert("inbounds".into(), Value::Array(inbound));
    apply_subscription_dns_options(&mut root, data, &source_node_ids)?;

    if !testing && !data.setting("globalCustomConfig").trim().is_empty() {
        let custom = parse_obj(data.setting("globalCustomConfig"))?;
        merge_json(&mut root, &custom);
    }
    if let Some(Value::Object(custom)) = metadata[&chosen.id].get("customConfig") {
        merge_json(&mut root, custom);
    }

    if purpose == Purpose::Main && !dynamic_chain_tags.is_empty() {
        mark_managed_urltests(&mut root, &dynamic_chain_tags);
    }

    sanitize_runtime(root, data, purpose, runtime_secret)
}

fn mark_managed_urltests(root: &mut Map<String, Value>, dynamic_chain_tags: &HashSet<String>) {
    let mut graph: HashMap<String, (Vec<String>, Option<String>)> = HashMap::new();
    for section in ["outbounds", "endpoints"] {
        if let Some(Value::Array(arr)) = root.get(section) {
            for item in arr {
                if let Some(obj) = item.as_object() {
                    if let Some(tag) = obj.get("tag").and_then(Value::as_str) {
                        let members = obj
                            .get("outbounds")
                            .and_then(Value::as_array)
                            .map(|a| {
                                a.iter()
                                    .filter_map(Value::as_str)
                                    .map(str::to_string)
                                    .collect()
                            })
                            .unwrap_or_default();
                        let detour = obj
                            .get("detour")
                            .and_then(Value::as_str)
                            .filter(|s| !s.trim().is_empty())
                            .map(str::to_string);
                        graph.insert(tag.to_string(), (members, detour));
                    }
                }
            }
        }
    }
    let mut memo: HashMap<String, bool> = HashMap::new();
    fn uses_dyn(
        tag: &str,
        dyn_tags: &HashSet<String>,
        graph: &HashMap<String, (Vec<String>, Option<String>)>,
        memo: &mut HashMap<String, bool>,
        active: &mut HashSet<String>,
    ) -> bool {
        if dyn_tags.contains(tag) {
            return true;
        }
        if let Some(&v) = memo.get(tag) {
            return v;
        }
        if !active.insert(tag.to_string()) {
            return false;
        }
        let res = if let Some((members, detour)) = graph.get(tag) {
            members
                .iter()
                .any(|m| uses_dyn(m, dyn_tags, graph, memo, active))
                || detour
                    .as_deref()
                    .is_some_and(|d| uses_dyn(d, dyn_tags, graph, memo, active))
        } else {
            false
        };
        active.remove(tag);
        memo.insert(tag.to_string(), res);
        res
    }

    if let Some(Value::Array(arr)) = root.get_mut("outbounds") {
        for item in arr.iter_mut() {
            if let Some(obj) = item.as_object_mut() {
                if obj.get("type").and_then(Value::as_str) == Some("urltest") {
                    if let Some(tag) = obj.get("tag").and_then(Value::as_str).map(str::to_string) {
                        let mut active = HashSet::new();
                        if uses_dyn(&tag, dynamic_chain_tags, &graph, &mut memo, &mut active) {
                            obj.insert("managed_test".into(), Value::Bool(true));
                        }
                    }
                }
            }
        }
    }
}

fn custom_config(
    input: &Map<String, Value>,
    data: &AppData,
    purpose: Purpose,
    secret: &str,
) -> Result<String, String> {
    let mut root = input.clone();
    if data.setting("serviceMode") == "proxy" {
        if let Some(Value::Array(inbounds)) = root.get("inbounds") {
            if inbounds
                .iter()
                .any(|i| i.get("type").and_then(Value::as_str) == Some("tun"))
            {
                return Err("自定义配置包含TUN，不能使用本地代理模式".to_string());
            }
        }
    }
    let mut experimental = root
        .get("experimental")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if purpose == Purpose::Test {
        root.insert("inbounds".into(), Value::Array(Vec::new()));
        experimental.shift_remove("clash_api");
        experimental.shift_remove("cache_file");
    } else if purpose == Purpose::Export {
        if let Some(Value::Object(api)) = experimental.get_mut("clash_api") {
            api.shift_remove("secret");
        }
    } else if data.bool_setting("statsEnabled", None)
        || data.bool_setting("clashApi", None)
        || experimental.contains_key("clash_api")
    {
        let mut runtime_api = experimental
            .get("clash_api")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        runtime_api.insert(
            "external_controller".into(),
            Value::String(format!("127.0.0.1:{}", parse_port(data.setting("apiPort"))?)),
        );
        runtime_api.shift_remove("secret");
        if !secret.trim().is_empty() {
            runtime_api.insert("secret".into(), Value::String(secret.to_string()));
        }
        experimental.insert("clash_api".into(), Value::Object(runtime_api));
    }
    if !experimental.is_empty() {
        root.insert("experimental".into(), Value::Object(experimental));
    } else {
        root.shift_remove("experimental");
    }
    sanitize_runtime(root, data, purpose, secret)
}

fn migrate_dns_rules(arr: &[Value]) -> Vec<Value> {
    let mut result = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(mut obj) = item.as_object().cloned() else {
            result.push(item.clone());
            continue;
        };
        if let Some(Value::Array(sub)) = obj.get("rules").cloned() {
            obj.insert("rules".into(), Value::Array(migrate_dns_rules(&sub)));
        }
        let strategy = obj
            .shift_remove("strategy")
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        if (strategy == "ipv4_only" || strategy == "ipv6_only")
            && obj.get("type").and_then(Value::as_str) != Some("logical")
        {
            let mut filter = obj.clone();
            for k in [
                "server",
                "action",
                "disable_cache",
                "rewrite_ttl",
                "client_subnet",
            ] {
                filter.shift_remove(k);
            }
            filter.insert(
                "query_type".into(),
                json!([if strategy == "ipv4_only" { "AAAA" } else { "A" }]),
            );
            filter.insert("action".into(), Value::String("predefined".to_string()));
            filter.insert("rcode".into(), Value::String("NOERROR".to_string()));
            result.push(Value::Object(filter));
        }
        result.push(Value::Object(obj));
    }
    result
}

fn sanitize_runtime(
    mut root: Map<String, Value>,
    data: &AppData,
    purpose: Purpose,
    secret: &str,
) -> Result<String, String> {
    {
        let exp = root
            .entry("experimental".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(exp_obj) = exp {
            exp_obj.insert(
                "unified_delay".into(),
                Value::Bool(data.bool_setting("unifiedDelay", None)),
            );
        }
    }
    {
        let route = root
            .entry("route".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(route_obj) = route {
            route_obj.insert(
                "tcp_concurrent".into(),
                Value::Bool(data.bool_setting("tcpConcurrent", None)),
            );
        }
    }
    if let Some(Value::Object(dns)) = root.get_mut("dns") {
        if let Some(Value::Array(arr)) = dns.get("rules").cloned() {
            dns.insert("rules".into(), Value::Array(migrate_dns_rules(&arr)));
        }
    }

    if purpose == Purpose::Test {
        root.insert("inbounds".into(), Value::Array(Vec::new()));
        if let Some(Value::Object(exp)) = root.get_mut("experimental") {
            exp.shift_remove("clash_api");
            exp.shift_remove("cache_file");
        }
    } else {
        if data.setting("serviceMode") == "proxy" {
            if let Some(Value::Array(inbounds)) = root.get("inbounds") {
                if inbounds
                    .iter()
                    .any(|i| i.get("type").and_then(Value::as_str) == Some("tun"))
                {
                    return Err("自定义覆盖配置含TUN，不能使用本地代理模式".to_string());
                }
            }
        }
        if let Some(Value::Object(exp)) = root.get_mut("experimental") {
            if let Some(Value::Object(api)) = exp.get_mut("clash_api") {
                api.shift_remove("secret");
                if purpose == Purpose::Main {
                    api.insert(
                        "external_controller".into(),
                        Value::String(format!("127.0.0.1:{}", parse_port(data.setting("apiPort"))?)),
                    );
                    if !secret.trim().is_empty() {
                        api.insert("secret".into(), Value::String(secret.to_string()));
                    }
                }
            }
        }
    }

    let core_kind = normalize_core_kind(data.setting("proxyCore"));
    if purpose != Purpose::Export && core_kind != CORE_SING_BOX {
        root.insert(
            EXPERIMENTAL_CORE_KEY.into(),
            Value::String(core_kind.to_string()),
        );
    }

    let rendered = serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())?;
    validate_config(&rendered)?;
    Ok(rendered)
}

pub fn validate_config(config: &str) -> Result<(), String> {
    let root: Value = serde_json::from_str(config).map_err(|e| format!("配置 JSON 无效: {e}"))?;
    let obj = root
        .as_object()
        .ok_or_else(|| "配置根节点必须是对象".to_string())?;
    let empty_arr = Vec::new();
    let outs = obj
        .get("outbounds")
        .and_then(Value::as_array)
        .unwrap_or(&empty_arr);
    let mut tags: HashSet<String> = HashSet::new();

    for o in outs {
        let m = o.as_object().ok_or_else(|| "outbound 无效".to_string())?;
        let otype = m
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "outbound 缺少 type".to_string())?;
        if otype.trim().is_empty() {
            return Err("outbound type 不能为空".to_string());
        }
        let tag = m
            .get("tag")
            .and_then(Value::as_str)
            .ok_or_else(|| "outbound 缺少 tag".to_string())?;
        if !tags.insert(tag.to_string()) {
            return Err("重复 outbound tag".to_string());
        }
    }
    if let Some(eps) = obj.get("endpoints").and_then(Value::as_array) {
        for ep in eps {
            let tag = ep
                .get("tag")
                .and_then(Value::as_str)
                .ok_or_else(|| "endpoint 缺少 tag".to_string())?;
            if !tags.insert(tag.to_string()) {
                return Err("重复 outbound tag".to_string());
            }
        }
    }
    if let Some(final_tag) = obj
        .get("route")
        .and_then(|r| r.get("final"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
    {
        if !tags.contains(final_tag) {
            return Err("最终路由不存在".to_string());
        }
    }

    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    let mut collect_edges = |item: &Value| -> Result<(), String> {
        let m = item.as_object().ok_or_else(|| "outbound 无效".to_string())?;
        let tag = m
            .get("tag")
            .and_then(Value::as_str)
            .ok_or_else(|| "缺少 tag".to_string())?;
        let mut refs = Vec::new();
        if let Some(d) = m.get("detour") {
            let ds = d
                .as_str()
                .ok_or_else(|| "detour 无效".to_string())?;
            if !tags.contains(ds) {
                return Err("代理链引用不存在".to_string());
            }
            if !ds.trim().is_empty() {
                refs.push(ds.to_string());
            }
        }
        if let Some(Value::Array(arr)) = m.get("outbounds") {
            for r in arr {
                let rs = r
                    .as_str()
                    .ok_or_else(|| "组引用不存在".to_string())?;
                if !tags.contains(rs) {
                    return Err("组引用不存在".to_string());
                }
                refs.push(rs.to_string());
            }
        }
        graph.insert(tag.to_string(), refs);
        Ok(())
    };
    for o in outs {
        collect_edges(o)?;
    }
    if let Some(eps) = obj.get("endpoints").and_then(Value::as_array) {
        for ep in eps {
            collect_edges(ep)?;
        }
    }

    let mut done: HashSet<String> = HashSet::new();
    let mut active: HashSet<String> = HashSet::new();
    fn visit(
        tag: &str,
        graph: &HashMap<String, Vec<String>>,
        done: &mut HashSet<String>,
        active: &mut HashSet<String>,
    ) -> Result<(), String> {
        if done.contains(tag) {
            return Ok(());
        }
        if !active.insert(tag.to_string()) {
            return Err("代理链存在环".to_string());
        }
        if let Some(edges) = graph.get(tag) {
            for e in edges {
                visit(e, graph, done, active)?;
            }
        }
        active.remove(tag);
        done.insert(tag.to_string());
        Ok(())
    }
    for k in graph.keys() {
        visit(k, &graph, &mut done, &mut active)?;
    }

    fn check_rule(rule: &Value, tags: &HashSet<String>) -> Result<(), String> {
        let Some(m) = rule.as_object() else {
            return Ok(());
        };
        if let Some(o) = m.get("outbound") {
            let os = o.as_str().ok_or_else(|| "规则目标不存在".to_string())?;
            if !tags.contains(os) {
                return Err("规则目标不存在".to_string());
            }
        }
        if let Some(Value::Array(arr)) = m.get("domain_regex") {
            for r in arr {
                let pat = r
                    .as_str()
                    .ok_or_else(|| "域名正则无效".to_string())?;
                Regex::new(pat).map_err(|e| format!("域名正则无效: {e}"))?;
            }
        }
        if let Some(Value::Array(sub)) = m.get("rules") {
            for s in sub {
                check_rule(s, tags)?;
            }
        }
        Ok(())
    }
    if let Some(Value::Array(r_arr)) = obj.get("route").and_then(|r| r.get("rules")) {
        for r in r_arr {
            check_rule(r, &tags)?;
        }
    }
    Ok(())
}

// Re-export smart_target_node_ids so config_builder callers can access it cleanly.
pub fn config_smart_target_node_ids(data: &AppData, target: &str) -> Vec<i64> {
    smart_target_node_ids(data, target)
}
