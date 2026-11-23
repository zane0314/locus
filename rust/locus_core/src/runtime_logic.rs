use crate::chain_routing::effective_chain_targets;
use crate::models::{AppData, Group, Node};
use crate::route_rules::{normalize_smart_target, smart_target};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

pub const TRAFFIC_MAX_BYTES: usize = 4 * 1024 * 1024;
pub const GROUP_SELECTION_PREFIX: &str = "groupSelection.";
pub const GROUP_SELECTION_DELAY_PREFIX: &str = "groupSelectionDelay.";

pub fn sampling_pause_core(device_idle: bool, subscription_busy: bool, wireguard: bool) -> bool {
    device_idle && !subscription_busy && !wireguard
}

pub fn sampling_interval(interactive: bool, observed: bool) -> i64 {
    if interactive && observed {
        1000
    } else {
        10_000
    }
}

pub fn sampling_tracked_tags(config: &str) -> Result<Vec<String>, String> {
    let root: Value = serde_json::from_str(config).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let mut tags: IndexSet<String> = IndexSet::new();
    let route_final = root
        .get("route")
        .and_then(|r| r.get("final"))
        .and_then(Value::as_str)
        .unwrap_or("");
    tags.insert(route_final.to_string());

    fn visit(value: &Value, tags: &mut IndexSet<String>) {
        match value {
            Value::Object(map) => {
                if let Some(o) = map.get("outbound").and_then(Value::as_str) {
                    if !o.trim().is_empty() {
                        tags.insert(o.to_string());
                    }
                }
                for v in map.values() {
                    visit(v, tags);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    visit(v, tags);
                }
            }
            _ => {}
        }
    }
    if let Some(rules) = root.get("route").and_then(|r| r.get("rules")) {
        visit(rules, &mut tags);
    }
    if route_final.trim().is_empty() {
        if let Some(first_tag) = root
            .get("outbounds")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|o| o.get("tag"))
            .and_then(Value::as_str)
        {
            tags.insert(first_tag.to_string());
        }
    }
    Ok(tags
        .into_iter()
        .filter(|t| !t.trim().is_empty())
        .collect())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrafficSample {
    pub id: String,
    pub app: String,
    pub domain: String,
    pub tx: i64,
    pub rx: i64,
}

pub fn traffic_domain(value: &str) -> String {
    let trimmed = value.trim();
    let after_scheme = trimmed
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(trimmed);
    let cut = after_scheme
        .split('/')
        .next()
        .unwrap_or(after_scheme)
        .split('?')
        .next()
        .unwrap_or(after_scheme)
        .split('#')
        .next()
        .unwrap_or(after_scheme);
    let host_port = cut.rsplit_once('@').map(|(_, r)| r).unwrap_or(cut);
    let host = if host_port.starts_with('[') {
        let before = host_port.split(']').next().unwrap_or(host_port);
        format!("{before}]")
    } else if host_port.bytes().filter(|&b| b == b':').count() == 1 {
        host_port.split(':').next().unwrap_or(host_port).to_string()
    } else {
        host_port.to_string()
    };
    host.trim()
        .to_ascii_lowercase()
        .chars()
        .take(253)
        .collect()
}

pub fn parse_traffic_samples(text: &str) -> Result<Vec<TrafficSample>, String> {
    if text.len() > TRAFFIC_MAX_BYTES {
        return Err("连接统计超过4MiB".to_string());
    }
    let root: Value = serde_json::from_str(text).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let Some(rows) = root.get("connections").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        let Some(obj) = row.as_object() else { continue };
        let id = obj.get("id").and_then(Value::as_str).unwrap_or("");
        if id.trim().is_empty() || !seen.insert(id.to_string()) {
            continue;
        }
        let meta = obj.get("metadata").and_then(Value::as_object);
        let proc_path = meta
            .and_then(|m| m.get("processPath"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let app_base = proc_path.rsplit('/').next().unwrap_or("");
        let app_raw = if app_base.trim().is_empty() {
            meta.and_then(|m| m.get("process"))
                .and_then(Value::as_str)
                .unwrap_or("未知应用")
        } else {
            app_base
        };
        let app: String = app_raw.chars().take(253).collect();

        let host = meta
            .and_then(|m| m.get("host"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let dest_raw = if host.trim().is_empty() {
            meta.and_then(|m| m.get("destinationIP"))
                .and_then(Value::as_str)
                .unwrap_or("未知域名")
        } else {
            host
        };
        let domain = traffic_domain(dest_raw);
        let tx = obj.get("upload").and_then(Value::as_i64).unwrap_or(0).max(0);
        let rx = obj
            .get("download")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0);
        out.push(TrafficSample {
            id: id.to_string(),
            app,
            domain,
            tx,
            rx,
        });
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainProbe {
    pub tag: String,
    pub candidates: Vec<String>,
    pub unavailable: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainProbePlan {
    #[serde(rename = "nodeId")]
    pub node_id: i64,
    pub front: Option<ChainProbe>,
    pub landing: Option<ChainProbe>,
}

pub fn read_chain_probe_plans(config: &str) -> Result<Vec<ChainProbePlan>, String> {
    let root: Value = serde_json::from_str(config).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let Some(arr) = root.get("outbounds").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut groups: HashMap<String, &Map<String, Value>> = HashMap::new();
    for item in arr {
        if let Some(obj) = item.as_object() {
            if obj.get("type").and_then(Value::as_str) == Some("selector") {
                if let Some(tag) = obj.get("tag").and_then(Value::as_str) {
                    groups.insert(tag.to_string(), obj);
                }
            }
        }
    }
    let probe = |tag: &str| -> Option<ChainProbe> {
        let g = groups.get(tag)?;
        let members = g.get("outbounds")?.as_array()?;
        let candidates: Vec<String> = members
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| !s.ends_with("-unavailable"))
            .map(str::to_string)
            .collect();
        if candidates.is_empty() {
            return None;
        }
        let unavailable = g
            .get("default")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Some(ChainProbe {
            tag: tag.to_string(),
            candidates,
            unavailable,
        })
    };

    let mut ids = BTreeSet::new();
    for (tag, g) in &groups {
        if let Some(mid) = tag
            .strip_prefix("chain-")
            .and_then(|s| s.strip_suffix("-front"))
        {
            if !mid.is_empty() && mid.bytes().all(|b| b.is_ascii_digit()) {
                if let Ok(id) = mid.parse::<i64>() {
                    ids.insert(id);
                }
            }
        }
        if let Some(rest) = tag.strip_prefix("node-") {
            if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
                let needle = format!("chain-{rest}-landing-");
                if g.get("outbounds")
                    .map(|v| v.to_string().contains(&needle))
                    .unwrap_or(false)
                {
                    if let Ok(id) = rest.parse::<i64>() {
                        ids.insert(id);
                    }
                }
            }
        }
    }

    Ok(ids
        .into_iter()
        .map(|id| ChainProbePlan {
            node_id: id,
            front: probe(&format!("chain-{id}-front")),
            landing: probe(&format!("node-{id}")),
        })
        .collect())
}

pub fn chain_probe_keys(config: &str) -> Result<IndexMap<String, String>, String> {
    let root: Value = serde_json::from_str(config).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let arr = root
        .get("outbounds")
        .and_then(Value::as_array)
        .ok_or_else(|| "缺少 outbounds".to_string())?;
    let mut nodes: HashMap<String, &Map<String, Value>> = HashMap::new();
    for item in arr {
        if let Some(obj) = item.as_object() {
            if let Some(tag) = obj.get("tag").and_then(Value::as_str) {
                nodes.insert(tag.to_string(), obj);
            }
        }
    }
    fn path_sig(
        key: &str,
        nodes: &HashMap<String, &Map<String, Value>>,
        seen: &HashSet<String>,
    ) -> String {
        if seen.contains(key) {
            return key.to_string();
        }
        let Some(orig) = nodes.get(key) else {
            return key.to_string();
        };
        let mut cloned = (*orig).clone();
        cloned.shift_remove("tag");
        if let Some(detour) = cloned
            .get("detour")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
        {
            let mut next_seen = seen.clone();
            next_seen.insert(key.to_string());
            let resolved = path_sig(&detour, nodes, &next_seen);
            cloned.insert("detour".into(), Value::String(resolved));
        }
        Value::Object(cloned).to_string()
    }
    let dns = root
        .get("dns")
        .map(Value::to_string)
        .unwrap_or_else(|| "null".to_string());
    let plans = read_chain_probe_plans(config)?;
    let mut out = IndexMap::new();
    for plan in plans {
        if let Some(front) = plan.front {
            for cand in front.candidates {
                if !out.contains_key(&cand) {
                    let sig = format!("{dns}{}", path_sig(&cand, &nodes, &HashSet::new()));
                    out.insert(cand, sig);
                }
            }
        }
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainParentProbe {
    pub tag: String,
    pub members: Vec<String>,
    pub dependencies: Vec<i64>,
}

pub fn chain_parent_probes(config: &str) -> Result<Vec<ChainParentProbe>, String> {
    let root: Value = serde_json::from_str(config).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let arr = root
        .get("outbounds")
        .and_then(Value::as_array)
        .ok_or_else(|| "缺少 outbounds".to_string())?;
    let mut nodes: IndexMap<String, &Map<String, Value>> = IndexMap::new();
    for item in arr {
        if let Some(obj) = item.as_object() {
            if let Some(tag) = obj.get("tag").and_then(Value::as_str) {
                nodes.insert(tag.to_string(), obj);
            }
        }
    }
    let dynamic: HashSet<i64> = read_chain_probe_plans(config)?
        .into_iter()
        .map(|p| p.node_id)
        .collect();

    fn deps(
        tag: &str,
        nodes: &IndexMap<String, &Map<String, Value>>,
        dynamic: &HashSet<i64>,
        seen: &HashSet<String>,
        out: &mut BTreeSet<i64>,
    ) {
        if seen.contains(tag) {
            return;
        }
        if let Some(rest) = tag.strip_prefix("node-") {
            if let Ok(id) = rest.parse::<i64>() {
                if dynamic.contains(&id) {
                    out.insert(id);
                    return;
                }
            }
        }
        let Some(node) = nodes.get(tag) else { return };
        let Some(members) = node.get("outbounds").and_then(Value::as_array) else {
            return;
        };
        let mut next_seen = seen.clone();
        next_seen.insert(tag.to_string());
        for m in members {
            if let Some(s) = m.as_str() {
                deps(s, nodes, dynamic, &next_seen, out);
            }
        }
    }

    let mut result = Vec::new();
    for (tag, obj) in &nodes {
        if obj.get("type").and_then(Value::as_str) == Some("urltest")
            && obj
                .get("managed_test")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            let members: Vec<String> = obj
                .get("outbounds")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let mut dep_set = BTreeSet::new();
            for m in &members {
                deps(m, &nodes, &dynamic, &HashSet::new(), &mut dep_set);
            }
            result.push(ChainParentProbe {
                tag: tag.clone(),
                members,
                dependencies: dep_set.into_iter().collect(),
            });
        }
    }
    Ok(result)
}

pub fn routed_chain_nodes(config: &str) -> Result<Vec<i64>, String> {
    let root: Value = serde_json::from_str(config).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let arr = root
        .get("outbounds")
        .and_then(Value::as_array)
        .ok_or_else(|| "缺少 outbounds".to_string())?;
    let mut nodes: HashMap<String, &Map<String, Value>> = HashMap::new();
    for item in arr {
        if let Some(obj) = item.as_object() {
            if let Some(tag) = obj.get("tag").and_then(Value::as_str) {
                nodes.insert(tag.to_string(), obj);
            }
        }
    }
    let mut seen = HashSet::new();
    let mut ids = BTreeSet::new();
    fn visit(
        tag: &str,
        nodes: &HashMap<String, &Map<String, Value>>,
        seen: &mut HashSet<String>,
        ids: &mut BTreeSet<i64>,
    ) {
        if !seen.insert(tag.to_string()) {
            return;
        }
        if let Some(rest) = tag.strip_prefix("node-") {
            if let Ok(id) = rest.parse::<i64>() {
                ids.insert(id);
            }
        }
        let Some(node) = nodes.get(tag) else { return };
        if tag == "proxy" {
            if let Some(def) = node
                .get("default")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                visit(def, nodes, seen, ids);
            }
        } else if let Some(members) = node.get("outbounds").and_then(Value::as_array) {
            for m in members {
                if let Some(s) = m.as_str() {
                    visit(s, nodes, seen, ids);
                }
            }
        }
    }
    for tag in sampling_tracked_tags(config)? {
        visit(&tag, &nodes, &mut seen, &mut ids);
    }
    Ok(ids.into_iter().collect())
}

pub fn saved_choice_delay(data: &AppData, tag: &str, node_ping: i32) -> i32 {
    let key = format!("{GROUP_SELECTION_DELAY_PREFIX}{tag}");
    data.setting_or(&key, "")
        .parse::<i32>()
        .ok()
        .filter(|v| (1..=65535).contains(v))
        .unwrap_or(node_ping)
}

pub fn restorable_selector_choices(
    data: &AppData,
    config: &str,
) -> Result<IndexMap<String, String>, String> {
    let root: Value = serde_json::from_str(config).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let Some(outbounds) = root.get("outbounds").and_then(Value::as_array) else {
        return Ok(IndexMap::new());
    };
    let mut result = IndexMap::new();
    for item in outbounds {
        let Some(obj) = item.as_object() else { continue };
        let tag = obj.get("tag").and_then(Value::as_str).unwrap_or("");
        if obj.get("type").and_then(Value::as_str) != Some("selector")
            || !(tag.starts_with("group-") || tag.starts_with("merge-"))
        {
            continue;
        }
        let marker_key = format!("{GROUP_SELECTION_PREFIX}{tag}");
        let marker = data.setting_or(&marker_key, "");
        if marker.trim().is_empty() {
            continue;
        }
        let members: Vec<&str> = obj
            .get("outbounds")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if !members.contains(&marker) {
            continue;
        }
        if tag.starts_with("merge-") {
            let merge = data.merges.iter().find(|m| format!("merge-{}", m.id) == tag);
            let valid = merge.is_some_and(|m| {
                m.mode == "selector" && format!("node-{}", m.selected_id) == marker
            });
            if !valid {
                continue;
            }
        }
        result.insert(tag.to_string(), marker.to_string());
    }
    Ok(result)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetGroup {
    pub tag: String,
    #[serde(rename = "urlTest")]
    pub url_test: bool,
    #[serde(rename = "mergeId")]
    pub merge_id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetPlanData {
    pub target: String,
    #[serde(rename = "serviceKey")]
    pub service_key: String,
    pub members: Vec<i64>,
    pub group: Option<TargetGroup>,
    pub outbounds: IndexMap<String, String>,
    pub fingerprint: String,
}

pub fn smart_target_node_ids(data: &AppData, target: &str) -> Vec<i64> {
    let enabled_groups: HashSet<i64> = data
        .groups
        .iter()
        .filter(|g| g.enabled)
        .map(|g| g.id)
        .collect();
    let sub_groups: HashSet<i64> = data
        .groups
        .iter()
        .filter(|g| g.enabled && !g.subscription_url.trim().is_empty())
        .map(|g| g.id)
        .collect();
    let active: Vec<&Node> = data
        .nodes
        .iter()
        .filter(|n| {
            enabled_groups.contains(&n.group_id)
                && serde_json::from_str::<Value>(&n.outbound)
                    .ok()
                    .and_then(|v| v.get("type").and_then(Value::as_str).map(|s| s != "custom"))
                    .unwrap_or(true)
        })
        .collect();
    let choice = normalize_smart_target(target);
    if choice == "auto" {
        active
            .into_iter()
            .filter(|n| sub_groups.contains(&n.group_id))
            .map(|n| n.id)
            .collect()
    } else if let Some(rest) = choice.strip_prefix("node:") {
        let id = rest.parse::<i64>().ok();
        active
            .into_iter()
            .filter(|n| Some(n.id) == id)
            .map(|n| n.id)
            .collect()
    } else if let Some(rest) = choice.strip_prefix("group:") {
        let id = rest.parse::<i64>().ok();
        active
            .into_iter()
            .filter(|n| Some(n.group_id) == id)
            .map(|n| n.id)
            .collect()
    } else if let Some(rest) = choice.strip_prefix("merge:") {
        let id = rest.parse::<i64>().ok();
        let Some(merge) = data.merges.iter().find(|m| Some(m.id) == id) else {
            return Vec::new();
        };
        active
            .into_iter()
            .filter(|n| merge.node_ids.contains(&n.id) || merge.group_ids.contains(&n.group_id))
            .map(|n| n.id)
            .collect()
    } else if choice == "proxy" {
        let selected: Vec<i64> = active
            .iter()
            .filter(|n| n.id == data.selected_node_id())
            .map(|n| n.id)
            .collect();
        if selected.is_empty() {
            active.into_iter().take(1).map(|n| n.id).collect()
        } else {
            selected
        }
    } else {
        Vec::new()
    }
}

pub fn resolve_target_group(data: &AppData, target: &str, service_key: &str) -> Option<TargetGroup> {
    let choice = normalize_smart_target(target);
    if choice == "auto" {
        if !service_key.trim().is_empty() {
            Some(TargetGroup {
                tag: format!("smart-{service_key}"),
                url_test: true,
                merge_id: 0,
            })
        } else {
            None
        }
    } else if let Some(rest) = choice.strip_prefix("group:") {
        let id = rest.parse::<i64>().ok()?;
        let group = data.groups.iter().find(|g| g.enabled && g.id == id)?;
        let opts: Map<String, Value> = serde_json::from_str::<Value>(&group.options)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        let selectable = opts
            .get("groupIsSelector")
            .and_then(Value::as_bool)
            .or_else(|| opts.get("isSelector").and_then(Value::as_bool))
            .unwrap_or(true);
        if selectable {
            Some(TargetGroup {
                tag: format!("group-{}", group.id),
                url_test: false,
                merge_id: 0,
            })
        } else {
            None
        }
    } else if let Some(rest) = choice.strip_prefix("merge:") {
        let id = rest.parse::<i64>().ok()?;
        let merge = data.merges.iter().find(|m| m.id == id)?;
        Some(TargetGroup {
            tag: format!("merge-{}", merge.id),
            url_test: merge.mode == "urltest",
            merge_id: merge.id,
        })
    } else {
        None
    }
}

pub fn target_plan_create(
    data: &AppData,
    target: &str,
    service_key: &str,
) -> Option<TargetPlanData> {
    if !service_key.trim().is_empty()
        && smart_target(data, service_key) != normalize_smart_target(target)
    {
        return None;
    }
    let members = smart_target_node_ids(data, target);
    let by_id: HashMap<i64, &Node> = data.nodes.iter().map(|n| (n.id, n)).collect();
    let mut outbounds = IndexMap::new();
    for &id in &members {
        if let Some(n) = by_id.get(&id) {
            outbounds.insert(id.to_string(), n.outbound.clone());
        }
    }
    let fp = target_link_fingerprint(data, &members);
    Some(TargetPlanData {
        target: target.to_string(),
        service_key: service_key.to_string(),
        members,
        group: resolve_target_group(data, target, service_key),
        outbounds,
        fingerprint: fp,
    })
}

fn target_link_fingerprint(data: &AppData, members: &[i64]) -> String {
    let nodes: HashMap<i64, &Node> = data.nodes.iter().map(|n| (n.id, n)).collect();
    let groups: HashMap<i64, &Group> = data.groups.iter().map(|g| (g.id, g)).collect();
    let parse_map = |s: &str| -> Map<String, Value> {
        serde_json::from_str::<Value>(s)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    };
    let mut sources: HashMap<(i64, String), &Node> = HashMap::new();
    for n in data.nodes.iter().rev() {
        let meta = parse_map(&n.metadata);
        if let Some(tag) = meta
            .get("sourceTag")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
        {
            sources.insert((n.group_id, tag.to_string()), n);
        }
    }
    let mut seen = BTreeSet::new();
    let mut used_groups = BTreeSet::new();
    let mut lookups: Vec<String> = Vec::new();
    let mut queue: VecDeque<i64> = members.iter().copied().collect();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = nodes.get(&id).copied() else {
            continue;
        };
        let outbound = parse_map(&node.outbound);
        if outbound.get("type").and_then(Value::as_str) == Some("chain") {
            if let Some(ids) = outbound.get("node_ids").and_then(Value::as_array) {
                for v in ids {
                    if let Some(mid) = v.as_i64() {
                        queue.push_back(mid);
                    }
                }
            }
        }
        if outbound.contains_key("detour") {
            let tag = outbound
                .get("detour")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let src = sources.get(&(node.group_id, tag.clone())).copied();
            let src_str = src
                .map(|s| s.id.to_string())
                .unwrap_or_else(|| "null".to_string());
            lookups.push(format!("{}/{tag}={src_str}", node.group_id));
            if let Some(s) = src {
                queue.push_back(s.id);
            }
        }
        if let Some(group) = groups.get(&node.group_id).copied() {
            used_groups.insert(group.id);
            let targets = effective_chain_targets(data, node);
            for t in [&targets.front, &targets.landing] {
                if let Some(tid) = t.strip_prefix("node:").and_then(|s| s.parse::<i64>().ok()) {
                    queue.push_back(tid);
                } else if let Some(tid) =
                    t.strip_prefix("group:").and_then(|s| s.parse::<i64>().ok())
                {
                    used_groups.insert(tid);
                    let mut cands: Vec<i64> = data
                        .nodes
                        .iter()
                        .filter(|n| n.group_id == tid)
                        .map(|n| n.id)
                        .collect();
                    cands.sort_unstable();
                    let joined: Vec<String> = cands.iter().map(|i| i.to_string()).collect();
                    lookups.push(format!("chain/{t}={}", joined.join(",")));
                    for c in cands {
                        queue.push_back(c);
                    }
                }
            }
        }
    }
    lookups.sort();
    let nodes_snap: Vec<Value> = seen
        .iter()
        .map(|id| match nodes.get(id) {
            Some(n) => serde_json::json!([n.id, n.group_id, n.outbound, n.metadata]),
            None => Value::Null,
        })
        .collect();
    let groups_snap: Vec<Value> = used_groups
        .iter()
        .map(|id| match groups.get(id) {
            Some(g) => serde_json::json!([g.id, g.enabled, g.front_proxy, g.landing_proxy, g.options]),
            None => Value::Null,
        })
        .collect();
    serde_json::json!([nodes_snap, groups_snap, lookups]).to_string()
}

pub fn reorder_visible_ids(current: &[i64], requested: &[i64]) -> Result<Vec<i64>, String> {
    let cur_set: HashSet<i64> = current.iter().copied().collect();
    let mut req_set = HashSet::with_capacity(requested.len());
    for &id in requested {
        if !req_set.insert(id) || !cur_set.contains(&id) {
            return Err("排序记录已变化，请重试".to_string());
        }
    }
    let mut iter = requested.iter().copied();
    Ok(current
        .iter()
        .map(|&id| {
            if req_set.contains(&id) {
                iter.next().unwrap_or(id)
            } else {
                id
            }
        })
        .collect())
}

pub fn node_test_label(status: i32, ping: i32) -> String {
    if status == 1 || ping == -2 {
        "失败".to_string()
    } else if ping >= 0 {
        format!("{ping} ms")
    } else {
        "未测试".to_string()
    }
}

pub fn target_latency_label(
    data: &AppData,
    target: &str,
    testing: &HashSet<i64>,
) -> Option<String> {
    let members: HashSet<i64> = smart_target_node_ids(data, target).into_iter().collect();
    if members.is_empty() {
        return None;
    }
    if members.iter().any(|id| testing.contains(id)) {
        return Some("测试中".to_string());
    }
    let nodes: Vec<&Node> = data
        .nodes
        .iter()
        .filter(|n| members.contains(&n.id))
        .collect();
    if let Some(min_ping) = nodes
        .iter()
        .filter(|n| n.ping > 0 && n.status != 1)
        .map(|n| n.ping)
        .min()
    {
        return Some(format!("{min_ping} ms"));
    }
    if nodes.iter().any(|n| n.status == 1 || n.ping == -2) {
        Some("失败".to_string())
    } else {
        Some("未测试".to_string())
    }
}

pub fn smart_lowest_node(
    data: &AppData,
    value: &str,
    testing: &HashSet<i64>,
) -> Option<String> {
    let norm = normalize_smart_target(value);
    if !(norm == "auto" || norm.starts_with("group:") || norm.starts_with("merge:")) {
        return None;
    }
    let members = smart_target_node_ids(data, value);
    if members.iter().any(|id| testing.contains(id)) {
        return None;
    }
    let by_id: HashMap<i64, &Node> = data.nodes.iter().map(|n| (n.id, n)).collect();
    let mut best: Option<&Node> = None;
    for id in members {
        if let Some(&n) = by_id.get(&id) {
            if n.status != 1 && n.ping > 0 && best.is_none_or(|b| n.ping < b.ping) {
                best = Some(n);
            }
        }
    }
    best.map(|n| n.name.clone())
}
