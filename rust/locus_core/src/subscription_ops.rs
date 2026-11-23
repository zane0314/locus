use crate::chain_routing::{
    derived_group_source_id, derived_node_source_id, sync_derived_chains_with_alloc,
};
use crate::models::{AppData, Group, Node};
use crate::node_identity::node_identity_key;
use crate::share_uri::{percent_decode, socks_user_info, ShareUri};
use crate::subscription_parser::ParsedNode;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionOptions {
    #[serde(rename = "autoUpdate", default)]
    pub auto_update: bool,
    #[serde(rename = "autoUpdateDelay", default = "default_delay")]
    pub auto_update_delay: i32,
    #[serde(rename = "updateWhenConnectedOnly", default)]
    pub update_when_connected_only: bool,
    #[serde(rename = "customUserAgent", default)]
    pub custom_user_agent: String,
    #[serde(rename = "filterMode", default)]
    pub filter_mode: i32,
    #[serde(rename = "filterRegex", default)]
    pub filter_regex: String,
    #[serde(default)]
    pub deduplication: bool,
    #[serde(rename = "forceResolve", default)]
    pub force_resolve: bool,
    #[serde(rename = "serverDnsResolver", default)]
    pub server_dns_resolver: String,
}

fn default_delay() -> i32 {
    1440
}

impl Default for SubscriptionOptions {
    fn default() -> Self {
        Self {
            auto_update: false,
            auto_update_delay: 1440,
            update_when_connected_only: false,
            custom_user_agent: String::new(),
            filter_mode: 0,
            filter_regex: String::new(),
            deduplication: false,
            force_resolve: false,
            server_dns_resolver: String::new(),
        }
    }
}

fn parse_obj(text: &str) -> Result<Map<String, Value>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("JSON 解析失败: {e}"))?;
    v.as_object()
        .cloned()
        .ok_or_else(|| "JSON 必须是对象".to_string())
}

impl SubscriptionOptions {
    pub fn parse(text: &str) -> Result<Self, String> {
        let o = parse_obj(text)?;
        let res = Self {
            auto_update: o.get("autoUpdate").and_then(Value::as_bool).unwrap_or(false),
            auto_update_delay: o
                .get("autoUpdateDelay")
                .and_then(Value::as_i64)
                .unwrap_or(1440) as i32,
            update_when_connected_only: o
                .get("updateWhenConnectedOnly")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            custom_user_agent: o
                .get("customUserAgent")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            filter_mode: o.get("filterMode").and_then(Value::as_i64).unwrap_or(0) as i32,
            filter_regex: o
                .get("filterRegex")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            deduplication: o
                .get("deduplication")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            force_resolve: o
                .get("forceResolve")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            server_dns_resolver: o
                .get("serverDnsResolver")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
        };
        if !(1..=525_600).contains(&res.auto_update_delay) {
            return Err("订阅间隔必须为 1..525600 分钟".to_string());
        }
        if !(0..=2).contains(&res.filter_mode) {
            return Err("订阅过滤模式无效".to_string());
        }
        if res.filter_regex.len() > 4096 {
            return Err("订阅过滤正则过长".to_string());
        }
        if res.filter_mode != 0 && !res.filter_regex.trim().is_empty() {
            Regex::new(&res.filter_regex).map_err(|e| format!("订阅过滤正则无效: {e}"))?;
        }
        if res.custom_user_agent.len() > 1024
            || !res
                .custom_user_agent
                .chars()
                .all(|c| (32..=126).contains(&(c as u32)))
        {
            return Err("订阅 User-Agent 必须为单行 ASCII 文本".to_string());
        }
        if res.server_dns_resolver.len() > 2048
            || res.server_dns_resolver.contains('\n')
            || res.server_dns_resolver.contains('\r')
        {
            return Err("节点 DNS 解析器无效".to_string());
        }
        if !res.server_dns_resolver.is_empty() {
            subscription_dns_server(&res.server_dns_resolver, "validation")?;
        }
        Ok(res)
    }

    pub fn select(&self, nodes: &[ParsedNode]) -> Result<Vec<ParsedNode>, String> {
        let pattern = if self.filter_mode != 0 && !self.filter_regex.trim().is_empty() {
            Some(Regex::new(&self.filter_regex).map_err(|e| format!("订阅过滤正则无效: {e}"))?)
        } else {
            None
        };
        let mut filtered = Vec::with_capacity(nodes.len());
        for n in nodes {
            let keep = match &pattern {
                None => true,
                Some(re) => {
                    let matched = re.is_match(&n.name);
                    if self.filter_mode == 1 {
                        matched
                    } else {
                        !matched
                    }
                }
            };
            if keep {
                filtered.push(n.clone());
            }
        }
        let selected = if self.deduplication {
            let mut seen = HashSet::new();
            let mut deduped = Vec::with_capacity(filtered.len());
            for n in filtered {
                let key = node_identity_key(&n.outbound)?;
                if seen.insert(key) {
                    deduped.push(n);
                }
            }
            deduped
        } else {
            filtered
        };
        if selected.is_empty() {
            return Err("订阅过滤后没有有效节点，已保留原节点".to_string());
        }
        Ok(selected)
    }

    pub fn signature(text: &str) -> Result<String, String> {
        let mut o = parse_obj(text)?;
        o.shift_remove("subscriptionRuntime");
        o.shift_remove("subscriptionLastUpdated");
        let wrapper = json!({ "options": Value::Object(o) }).to_string();
        node_identity_key(&wrapper)
    }
}

pub fn subscription_dns_server(address: &str, tag: &str) -> Result<Value, String> {
    let value = address.trim();
    let mut result = Map::new();
    result.insert("tag".into(), Value::String(tag.to_string()));
    if value == "local" {
        result.insert("type".into(), Value::String("local".to_string()));
        return Ok(Value::Object(result));
    }
    let with_scheme = if value.contains("://") {
        value.to_string()
    } else {
        format!("udp://{value}")
    };
    let uri = Url::parse(&with_scheme).map_err(|_| "节点 DNS 地址无效".to_string())?;
    if !uri.username().is_empty() || uri.password().is_some() || uri.fragment().is_some() {
        return Err("节点 DNS 地址无效".to_string());
    }
    let raw_host = uri.host_str().ok_or_else(|| "节点 DNS 地址无效".to_string())?;
    let host = raw_host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(raw_host)
        .to_string();
    let scheme = uri.scheme();
    if !matches!(scheme, "udp" | "tcp" | "tls" | "https" | "quic" | "h3") {
        return Err("节点 DNS 协议不支持".to_string());
    }
    result.insert("type".into(), Value::String(scheme.to_string()));
    result.insert("server".into(), Value::String(host.clone()));
    if let Some(port) = uri.port() {
        if port == 0 {
            return Err("节点 DNS 端口无效".to_string());
        }
        result.insert("server_port".into(), Value::Number(port.into()));
    }
    if matches!(scheme, "https" | "h3") {
        let p = uri.path();
        let path = if p.is_empty() || (p == "/" && !with_scheme.ends_with('/') && !with_scheme[with_scheme.find("://").unwrap() + 3..].contains('/')) {
            "/dns-query"
        } else if p.trim().is_empty() {
            "/dns-query"
        } else {
            p
        };
        result.insert("path".into(), Value::String(path.to_string()));
    }
    if is_domain_name(&host) {
        result.insert(
            "domain_resolver".into(),
            Value::String("dns-direct".to_string()),
        );
    }
    Ok(Value::Object(result))
}

pub fn subscription_dns_bootstrapped(server: &Value, address: &str) -> Result<Value, String> {
    let mut obj = server
        .as_object()
        .cloned()
        .ok_or_else(|| "DNS 服务器配置无效".to_string())?;
    let host = obj
        .get("server")
        .and_then(Value::as_str)
        .ok_or_else(|| "DNS 服务器缺少 server".to_string())?
        .to_string();
    let stype = obj.get("type").and_then(Value::as_str).unwrap_or("");
    if matches!(stype, "tls" | "https" | "quic" | "h3") {
        let tls_entry = obj
            .entry("tls".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(tls_map) = tls_entry.as_object_mut() {
            let existing = tls_map
                .get("server_name")
                .and_then(Value::as_str)
                .unwrap_or("");
            if existing.trim().is_empty() {
                tls_map.insert("server_name".into(), Value::String(host));
            }
        }
    }
    obj.insert("server".into(), Value::String(address.to_string()));
    obj.shift_remove("domain_resolver");
    Ok(Value::Object(obj))
}

fn is_domain_name(host: &str) -> bool {
    !host.trim().is_empty()
        && !host.contains(':')
        && host.split('.').any(|part| part.parse::<i32>().is_err())
}

pub fn apply_subscription_dns_options(
    root: &mut Map<String, Value>,
    data: &AppData,
    source_node_ids: &HashMap<String, i64>,
) -> Result<(), String> {
    let mut resolvers: HashMap<i64, String> = HashMap::new();
    for g in &data.groups {
        if let Ok(opts) = parse_obj(&g.options) {
            if let Some(r) = opts.get("serverDnsResolver").and_then(Value::as_str) {
                let trimmed = r.trim();
                if !trimmed.is_empty() {
                    resolvers.insert(g.id, trimmed.to_string());
                }
            }
        }
    }
    if resolvers.is_empty() {
        return Ok(());
    }
    let mut definitions: HashMap<i64, Value> = HashMap::new();
    let mut used: Vec<i64> = Vec::new();
    let mut used_set: HashSet<i64> = HashSet::new();

    for section in ["outbounds", "endpoints"] {
        if let Some(Value::Array(arr)) = root.get_mut(section) {
            for item in arr.iter_mut() {
                let Some(outbound) = item.as_object_mut() else {
                    continue;
                };
                let tag = outbound
                    .get("tag")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let node_id = source_node_ids.get(&tag).copied().or_else(|| {
                    tag.strip_prefix("node-").and_then(|rest| {
                        let digits = rest.split('-').next().unwrap_or("");
                        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                            digits.parse::<i64>().ok()
                        } else {
                            None
                        }
                    })
                });
                let Some(id) = node_id else { continue };
                let Some(group_id) = data.nodes.iter().find(|n| n.id == id).map(|n| n.group_id)
                else {
                    continue;
                };
                if !resolvers.contains_key(&group_id) {
                    continue;
                }
                let host = outbound
                    .get("server")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let has_domain = is_domain_name(host);
                let peer_domain = outbound
                    .get("peers")
                    .and_then(Value::as_array)
                    .is_some_and(|peers| {
                        peers.iter().any(|p| {
                            p.get("address")
                                .and_then(Value::as_str)
                                .is_some_and(is_domain_name)
                        })
                    });
                if has_domain || peer_domain {
                    if !definitions.contains_key(&group_id) {
                        let def = subscription_dns_server(
                            &resolvers[&group_id],
                            &format!("subscription-dns-{group_id}"),
                        )?;
                        definitions.insert(group_id, def);
                    }
                    let def_tag = definitions[&group_id]
                        .get("tag")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    outbound.insert("domain_resolver".into(), Value::String(def_tag));
                    if used_set.insert(group_id) {
                        used.push(group_id);
                    }
                }
            }
        }
    }

    let dns = root
        .entry("dns".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let dns_obj = dns
        .as_object_mut()
        .ok_or_else(|| "dns 配置无效".to_string())?;
    let servers = dns_obj
        .entry("servers".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let servers_arr = servers
        .as_array_mut()
        .ok_or_else(|| "dns.servers 无效".to_string())?;
    for gid in used {
        if let Some(def) = definitions.remove(&gid) {
            servers_arr.push(def);
        }
    }
    Ok(())
}

pub fn subscription_due_at(group: &Group) -> Result<i64, String> {
    let options = SubscriptionOptions::parse(&group.options)?;
    let obj = parse_obj(&group.options)?;
    let legacy = obj
        .get("subscriptionLastUpdated")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        * 1000;
    let last = if group.updated_at > 0 {
        group.updated_at
    } else {
        legacy
    };
    if last <= 0 {
        Ok(0)
    } else {
        Ok(last + (options.auto_update_delay as i64) * 60_000)
    }
}

pub fn subscription_next_at(group: &Group) -> Result<i64, String> {
    let obj = parse_obj(&group.options)?;
    let runtime = obj.get("subscriptionRuntime").and_then(Value::as_object);
    let retry = runtime
        .and_then(|r| r.get("nextAttempt"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let state = runtime
        .and_then(|r| r.get("state"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if matches!(state, "applying" | "apply-error") && retry > 0 {
        Ok(retry)
    } else {
        Ok(subscription_due_at(group)?.max(retry))
    }
}

pub fn subscription_should_update(group: &Group, now: i64, connected: bool) -> Result<bool, String> {
    let options = SubscriptionOptions::parse(&group.options)?;
    Ok(group.enabled
        && !group.subscription_url.trim().is_empty()
        && options.auto_update
        && now >= subscription_due_at(group)?
        && (!options.update_when_connected_only || connected))
}

pub fn subscription_scheduled(group: &Group) -> Result<bool, String> {
    Ok(group.enabled
        && !group.subscription_url.trim().is_empty()
        && SubscriptionOptions::parse(&group.options)?.auto_update)
}

fn update_journal(options: &str, f: impl FnOnce(&mut Map<String, Value>)) -> Result<String, String> {
    let mut obj = parse_obj(options)?;
    let mut state = obj
        .get("subscriptionRuntime")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    f(&mut state);
    obj.insert("subscriptionRuntime".into(), Value::Object(state));
    Ok(Value::Object(obj).to_string())
}

pub fn subscription_begin(
    current: &AppData,
    expected: &Group,
    request: &str,
    now: i64,
    automatic: bool,
) -> Result<AppData, String> {
    let group = current
        .groups
        .iter()
        .find(|g| g.id == expected.id)
        .ok_or_else(|| "订阅组已删除".to_string())?;
    if group.subscription_url != expected.subscription_url
        || SubscriptionOptions::signature(&group.options)?
            != SubscriptionOptions::signature(&expected.options)?
    {
        return Err("订阅组 URL 或选项已变更，请重新更新".to_string());
    }
    let obj = parse_obj(&group.options)?;
    let runtime = obj.get("subscriptionRuntime").and_then(Value::as_object);
    if automatic {
        let state = runtime
            .and_then(|r| r.get("state"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let last_attempt = runtime
            .and_then(|r| r.get("lastAttempt"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if group.updated_at != expected.updated_at
            || (state == "updating" && now - last_attempt < 10 * 60_000)
        {
            return Err("订阅已变更或已有更新任务，已保留当前数据".to_string());
        }
    }
    let mut out = current.clone();
    for g in &mut out.groups {
        if g.id == group.id {
            g.options = update_journal(&g.options, |state| {
                state.insert("request".into(), Value::String(request.to_string()));
                state.insert("state".into(), Value::String("updating".to_string()));
                state.insert("lastAttempt".into(), now.into());
                state.insert("nextAttempt".into(), (now + 10 * 60_000).into());
                state.shift_remove("error");
            })?;
        }
    }
    Ok(out)
}

pub fn subscription_apply(
    current: &AppData,
    baseline: &Group,
    parsed: &[ParsedNode],
    user_info: &str,
    now: i64,
    next_id_start: Option<i64>,
) -> Result<AppData, String> {
    if parsed.is_empty() {
        return Err("订阅没有有效节点，已保留原节点".to_string());
    }
    let group = current
        .groups
        .iter()
        .find(|g| g.id == baseline.id)
        .ok_or_else(|| "订阅组已删除".to_string())?;
    let base_obj = parse_obj(&baseline.options)?;
    let req = base_obj
        .get("subscriptionRuntime")
        .and_then(Value::as_object)
        .and_then(|r| r.get("request"))
        .and_then(Value::as_str)
        .ok_or_else(|| "订阅响应已过期，已保留当前数据".to_string())?;
    let cur_req = parse_obj(&group.options)?
        .get("subscriptionRuntime")
        .and_then(Value::as_object)
        .and_then(|r| r.get("request"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if group.subscription_url != baseline.subscription_url
        || group.updated_at != baseline.updated_at
        || SubscriptionOptions::signature(&group.options)?
            != SubscriptionOptions::signature(&baseline.options)?
        || cur_req.as_deref() != Some(req)
    {
        return Err("订阅响应已过期，已保留当前数据".to_string());
    }

    let mut local_chains: Vec<Node> = Vec::new();
    let mut previous: Vec<Node> = Vec::new();
    for n in &current.nodes {
        if n.group_id == group.id {
            let is_chain = parse_obj(&n.outbound)
                .ok()
                .and_then(|o| o.get("type").and_then(Value::as_str).map(|s| s == "chain"))
                .unwrap_or(false);
            if is_chain {
                local_chains.push(n.clone());
            } else {
                previous.push(n.clone());
            }
        }
    }

    let mut remaining: HashMap<i64, Node> = previous.iter().map(|n| (n.id, n.clone())).collect();
    let node_ident = |outbound: &str, metadata: &str| -> String {
        if let Ok(m) = parse_obj(metadata) {
            if let Some(s) = m.get("subscriptionIdentity").and_then(Value::as_str) {
                if !s.trim().is_empty() {
                    return s.to_string();
                }
            }
        }
        node_identity_key(outbound).unwrap_or_default()
    };

    let mut identities: HashMap<String, VecDeque<Node>> = HashMap::new();
    let mut links: HashMap<String, VecDeque<Node>> = HashMap::new();
    for n in &previous {
        identities
            .entry(node_ident(&n.outbound, &n.metadata))
            .or_default()
            .push_back(n.clone());
        if !n.share_link.trim().is_empty() {
            links
                .entry(n.share_link.clone())
                .or_default()
                .push_back(n.clone());
        }
    }

    let take = |bucket: Option<&mut VecDeque<Node>>, rem: &mut HashMap<i64, Node>| -> Option<Node> {
        let q = bucket?;
        while let Some(n) = q.pop_front() {
            if rem.remove(&n.id).is_some() {
                return Some(n);
            }
        }
        None
    };

    let mut max_id = next_id_start.unwrap_or_else(|| {
        current
            .nodes
            .iter()
            .map(|n| n.id)
            .chain(current.groups.iter().map(|g| g.id))
            .chain(current.rules.iter().map(|r| r.id))
            .chain(current.merges.iter().map(|m| m.id))
            .max()
            .unwrap_or(0)
            + 1
    });
    let mut alloc = || {
        let id = max_id;
        max_id += 1;
        id
    };

    let mut next_order = previous.iter().map(|n| n.order).max().unwrap_or(-1) + 1;
    let mut fresh = Vec::with_capacity(parsed.len());
    for incoming in parsed {
        let key = node_ident(&incoming.outbound, &incoming.metadata);
        let matched = take(identities.get_mut(&key), &mut remaining)
            .or_else(|| take(links.get_mut(&incoming.share_link), &mut remaining));
        let mut meta = match &matched {
            Some(m) => parse_obj(&m.metadata).unwrap_or_default(),
            None => Map::new(),
        };
        if let Ok(add) = parse_obj(&incoming.metadata) {
            for (k, v) in add {
                if !matches!(k.as_str(), "chain" | "chainSourceNodeId" | "chainSourceName") {
                    meta.insert(k, v);
                }
            }
        }
        let mut node = matched.unwrap_or_else(|| {
            let order = next_order;
            next_order += 1;
            Node {
                id: alloc(),
                group_id: group.id,
                name: incoming.name.clone(),
                outbound: incoming.outbound.clone(),
                share_link: String::new(),
                ping: -1,
                tx: 0,
                rx: 0,
                order,
                status: 0,
                metadata: "{}".to_string(),
            }
        });
        node.name = incoming.name.clone();
        node.outbound = incoming.outbound.clone();
        node.share_link = incoming.share_link.clone();
        node.metadata = Value::Object(meta).to_string();
        fresh.push(node);
    }

    let removed: HashSet<i64> = remaining.keys().copied().collect();
    let mut all: Vec<Node> = current
        .nodes
        .iter()
        .filter(|n| n.group_id != group.id && !removed.contains(&n.id))
        .cloned()
        .collect();
    all.extend(local_chains.into_iter().filter(|n| !removed.contains(&n.id)));
    all.extend(fresh.into_iter().filter(|n| !removed.contains(&n.id)));

    let mut settings = current.settings.clone();
    settings.retain(|k, _| {
        !k.strip_prefix("nodeRegion.")
            .and_then(|s| s.parse::<i64>().ok())
            .is_some_and(|id| removed.contains(&id))
    });
    for (k, v) in settings.iter_mut() {
        if k.starts_with("smart.") && k.ends_with(".target") {
            if let Some(id) = v.strip_prefix("node:").and_then(|s| s.parse::<i64>().ok()) {
                if removed.contains(&id) {
                    *v = "off".to_string();
                }
            }
        }
    }

    let mut enabled_groups: Vec<&Group> = current.groups.iter().filter(|g| g.enabled).collect();
    enabled_groups.sort_by_key(|g| g.order);
    let enabled_ids: HashSet<i64> = enabled_groups.iter().map(|g| g.id).collect();
    let pinned_derived = enabled_groups
        .iter()
        .copied()
        .find(|g| g.id == current.selected_group_id() && derived_group_source_id(g) > 0);
    let selected = all
        .iter()
        .find(|n| n.id == current.selected_node_id() && enabled_ids.contains(&n.group_id))
        .or_else(|| {
            if let Some(pd) = pinned_derived {
                all.iter()
                    .filter(|n| n.group_id == pd.id)
                    .min_by_key(|n| n.order)
            } else {
                enabled_groups.iter().find_map(|cand| {
                    all.iter()
                        .filter(|n| n.group_id == cand.id)
                        .min_by_key(|n| n.order)
                })
            }
        });
    settings.insert(
        "selectedNodeId".into(),
        selected.map(|n| n.id).unwrap_or(0).to_string(),
    );
    settings.insert(
        "selectedGroupId".into(),
        pinned_derived
            .map(|g| g.id)
            .or_else(|| selected.map(|n| n.group_id))
            .unwrap_or(0)
            .to_string(),
    );

    let mut disabled_targets: HashSet<String> = current
        .groups
        .iter()
        .filter(|g| !g.enabled)
        .map(|g| format!("group:{}", g.id))
        .collect();
    for n in &all {
        if !enabled_ids.contains(&n.group_id) {
            disabled_targets.insert(format!("node:{}", n.id));
        }
    }
    for (k, v) in settings.iter_mut() {
        if k.starts_with("smart.") && k.ends_with(".target") && disabled_targets.contains(v.as_str())
        {
            *v = "off".to_string();
        }
    }

    let mut updated_groups = Vec::with_capacity(current.groups.len());
    for g in &current.groups {
        if g.id != group.id {
            updated_groups.push(g.clone());
        } else {
            let journaled = update_journal(&g.options, |state| {
                state.insert("state".into(), Value::String("applying".to_string()));
                state.insert("lastSuccess".into(), now.into());
                state.insert("nextAttempt".into(), (now + 5 * 60_000).into());
                state.shift_remove("error");
            })?;
            let mut opts_map = parse_obj(&journaled)?;
            opts_map.insert("subscriptionLastUpdated".into(), (now / 1000).into());
            let mut g2 = g.clone();
            g2.updated_at = now;
            g2.user_info = user_info.to_string();
            g2.options = Value::Object(opts_map).to_string();
            updated_groups.push(g2);
        }
    }

    let updated_rules = current
        .rules
        .iter()
        .map(|r| {
            if r.outbound
                .strip_prefix("node:")
                .and_then(|s| s.parse::<i64>().ok())
                .is_some_and(|id| removed.contains(&id))
            {
                let mut r2 = r.clone();
                r2.outbound = "proxy".to_string();
                r2
            } else {
                r.clone()
            }
        })
        .collect();

    let updated_merges = current
        .merges
        .iter()
        .map(|m| {
            let mut m2 = m.clone();
            m2.node_ids.retain(|id| !removed.contains(id));
            if removed.contains(&m2.selected_id) {
                m2.selected_id = 0;
            }
            m2
        })
        .collect();

    let intermediate = AppData {
        nodes: all,
        groups: updated_groups,
        rules: updated_rules,
        merges: updated_merges,
        settings,
    };
    let synced = sync_derived_chains_with_alloc(&intermediate, &mut alloc)?;
    Ok(synced.without_stale_node_settings())
}

pub fn subscription_record(
    current: &AppData,
    group_id: i64,
    state: &str,
    now: i64,
    error: &str,
    next_attempt: i64,
    request: Option<&str>,
) -> Result<AppData, String> {
    let mut out = current.clone();
    for g in &mut out.groups {
        if g.id != group_id {
            continue;
        }
        if let Some(req) = request {
            let cur_req = parse_obj(&g.options)?
                .get("subscriptionRuntime")
                .and_then(Value::as_object)
                .and_then(|r| r.get("request"))
                .and_then(Value::as_str)
                .map(str::to_string);
            if cur_req.as_deref() != Some(req) {
                continue;
            }
        }
        g.options = update_journal(&g.options, |val| {
            val.insert("state".into(), Value::String(state.to_string()));
            val.insert("lastAttempt".into(), now.into());
            if error.trim().is_empty() {
                val.shift_remove("error");
            } else {
                let truncated: String = error.chars().take(256).collect();
                val.insert("error".into(), Value::String(truncated));
            }
            if next_attempt > 0 {
                val.insert("nextAttempt".into(), next_attempt.into());
            } else {
                val.shift_remove("nextAttempt");
            }
        })?;
    }
    Ok(out)
}

pub fn node_removal_ids(nodes: &[Node], ids: &HashSet<i64>) -> HashSet<i64> {
    let mut removed = ids.clone();
    loop {
        let mut added = false;
        for n in nodes {
            if removed.contains(&n.id) || derived_node_source_id(n) != 0 {
                continue;
            }
            if let Ok(out) = parse_obj(&n.outbound) {
                if out.get("type").and_then(Value::as_str) == Some("chain") {
                    if let Some(hops) = out.get("node_ids").and_then(Value::as_array) {
                        if hops
                            .iter()
                            .filter_map(Value::as_i64)
                            .any(|h| removed.contains(&h))
                        {
                            removed.insert(n.id);
                            added = true;
                        }
                    }
                }
            }
        }
        if !added {
            return removed;
        }
    }
}

pub fn repair_legacy_socks_credentials(node: &Node) -> Node {
    let Ok(uri) = ShareUri::parse(&node.share_link) else {
        return node.clone();
    };
    if uri.scheme != "socks" && uri.scheme != "socks5" {
        return node.clone();
    }
    let Some(ref raw_user) = uri.raw_user_info else {
        return node.clone();
    };
    let encoded = percent_decode(raw_user);
    let credentials = socks_user_info(Some(raw_user));
    if credentials == encoded || !credentials.contains(':') {
        return node.clone();
    }
    let Ok(mut out) = parse_obj(&node.outbound) else {
        return node.clone();
    };
    let expected_port = if uri.port > 0 { uri.port } else { 1080 } as i64;
    let pass_empty = match out.get("password") {
        None => true,
        Some(Value::String(s)) => s.is_empty(),
        _ => false,
    };
    if out.get("type").and_then(Value::as_str) != Some("socks")
        || out.get("version").and_then(Value::as_str) != Some("5")
        || out.get("server").and_then(Value::as_str) != Some(uri.host.as_str())
        || out.get("server_port").and_then(Value::as_i64) != Some(expected_port)
        || out.get("username").and_then(Value::as_str) != Some(encoded.as_str())
        || !pass_empty
    {
        return node.clone();
    }
    if let Some((u, p)) = credentials.split_once(':') {
        out.insert("username".into(), Value::String(u.to_string()));
        out.insert("password".into(), Value::String(p.to_string()));
        let mut repaired = node.clone();
        repaired.outbound = Value::Object(out).to_string();
        repaired
    } else {
        node.clone()
    }
}
