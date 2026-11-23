use crate::models::{AppData, Group, Node};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainTargets {
    pub front: String,
    pub landing: String,
}

impl Default for ChainTargets {
    fn default() -> Self {
        Self {
            front: "none".to_string(),
            landing: "none".to_string(),
        }
    }
}

fn parse_obj(s: &str) -> Map<String, Value> {
    serde_json::from_str::<Value>(s)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

pub fn group_chain_targets(group: &Group) -> ChainTargets {
    let opts = parse_obj(&group.options);
    let chain = opts.get("chain").and_then(Value::as_object);
    let front = chain
        .and_then(|c| c.get("front"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            if group.front_proxy > 0 {
                format!("node:{}", group.front_proxy)
            } else {
                "none".to_string()
            }
        });
    let landing = chain
        .and_then(|c| c.get("landing"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            if group.landing_proxy > 0 {
                format!("node:{}", group.landing_proxy)
            } else {
                "none".to_string()
            }
        });
    ChainTargets { front, landing }
}

pub fn node_chain_targets(node: &Node) -> ChainTargets {
    let meta = parse_obj(&node.metadata);
    let chain = meta.get("chain").and_then(Value::as_object);
    let front = chain
        .and_then(|c| c.get("front"))
        .and_then(Value::as_str)
        .unwrap_or("inherit")
        .to_string();
    let landing = chain
        .and_then(|c| c.get("landing"))
        .and_then(Value::as_str)
        .unwrap_or("inherit")
        .to_string();
    ChainTargets { front, landing }
}

pub fn effective_chain_targets(data: &AppData, node: &Node) -> ChainTargets {
    let own = node_chain_targets(node);
    let inherited = data
        .groups
        .iter()
        .find(|g| g.id == node.group_id)
        .map(group_chain_targets)
        .unwrap_or_default();
    let self_ref = format!("node:{}", node.id);
    let exempt = inherited.front == self_ref || inherited.landing == self_ref;
    let base = if exempt {
        ChainTargets::default()
    } else {
        inherited
    };
    ChainTargets {
        front: if own.front == "inherit" {
            base.front
        } else {
            own.front
        },
        landing: if own.landing == "inherit" {
            base.landing
        } else {
            own.landing
        },
    }
}

pub fn group_with_chain_targets(group: &Group, targets: &ChainTargets) -> Group {
    let mut opts = parse_obj(&group.options);
    opts.insert(
        "chain".into(),
        json!({ "front": targets.front, "landing": targets.landing }),
    );
    let front_proxy = targets
        .front
        .strip_prefix("node:")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let landing_proxy = targets
        .landing
        .strip_prefix("node:")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let mut out = group.clone();
    out.options = Value::Object(opts).to_string();
    out.front_proxy = front_proxy;
    out.landing_proxy = landing_proxy;
    out
}

pub fn node_with_chain_targets(node: &Node, targets: &ChainTargets) -> Node {
    let mut meta = parse_obj(&node.metadata);
    meta.insert(
        "chain".into(),
        json!({ "front": targets.front, "landing": targets.landing }),
    );
    let mut out = node.clone();
    out.metadata = Value::Object(meta).to_string();
    out
}

pub fn derived_group_source_id(group: &Group) -> i64 {
    parse_obj(&group.options)
        .get("chainSourceGroupId")
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

pub fn derived_node_source_id(node: &Node) -> i64 {
    parse_obj(&node.metadata)
        .get("chainSourceNodeId")
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

fn is_valid_positive_target_id(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some('1'..='9') => chars.all(|c| c.is_ascii_digit()) && s.parse::<i64>().is_ok(),
        _ => false,
    }
}

pub fn validate_chain_target_syntax(data: &AppData) -> Result<(), String> {
    let check = |value: &str, is_node: bool| -> Result<(), String> {
        if value == "none" || (is_node && value == "inherit") {
            return Ok(());
        }
        if let Some(rest) = value
            .strip_prefix("node:")
            .or_else(|| value.strip_prefix("group:"))
        {
            if is_valid_positive_target_id(rest) {
                return Ok(());
            }
        }
        Err(format!("代理链目标无效: {value}"))
    };
    for g in &data.groups {
        let t = group_chain_targets(g);
        check(&t.front, false)?;
        check(&t.landing, false)?;
    }
    for n in &data.nodes {
        let t = node_chain_targets(n);
        check(&t.front, true)?;
        check(&t.landing, true)?;
    }
    Ok(())
}

pub fn chain_target_nodes<'a>(
    data: &'a AppData,
    target: &str,
    allow_missing: bool,
) -> Result<Vec<&'a Node>, String> {
    if target == "none" {
        return Ok(Vec::new());
    }
    let result: Vec<&'a Node> = if let Some(rest) = target.strip_prefix("node:") {
        let id = rest
            .parse::<i64>()
            .map_err(|_| format!("代理链目标无效: {target}"))?;
        data.nodes.iter().filter(|n| n.id == id).collect()
    } else if let Some(rest) = target.strip_prefix("group:") {
        let id = rest
            .parse::<i64>()
            .map_err(|_| format!("代理链目标无效: {target}"))?;
        data.nodes.iter().filter(|n| n.group_id == id).collect()
    } else {
        return Err(format!("代理链目标无效: {target}"));
    };

    if allow_missing && result.is_empty() {
        return Ok(result);
    }
    if result.is_empty() {
        return Err(format!("代理链目标不存在或分组为空: {target}"));
    }
    if !allow_missing
        && !result
            .iter()
            .all(|n| data.groups.iter().any(|g| g.id == n.group_id && g.enabled))
    {
        return Err(format!("代理链目标分组已停用: {target}"));
    }
    if result.iter().any(|n| outbound_type(n) == "custom") {
        return Err("完整自定义配置不能嵌入代理链".to_string());
    }
    Ok(result)
}

fn outbound_type(node: &Node) -> String {
    parse_obj(&node.outbound)
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

pub fn raw_chain_ids(
    data: &AppData,
    id: i64,
    active: &mut HashSet<i64>,
    allow_missing: bool,
    nodes_by_id: &HashMap<i64, &Node>,
) -> Result<Vec<i64>, String> {
    if active.len() >= 64 || !active.insert(id) {
        return Err("代理链引用循环或过深".to_string());
    }
    let Some(node) = nodes_by_id.get(&id).copied() else {
        active.remove(&id);
        if allow_missing {
            return Ok(Vec::new());
        }
        return Err(format!("代理链节点不存在: {id}"));
    };
    let bean = parse_obj(&node.outbound);
    let otype = bean.get("type").and_then(Value::as_str).unwrap_or("");
    let ids = if otype == "chain" {
        let members = bean
            .get("node_ids")
            .and_then(Value::as_array)
            .ok_or_else(|| "代理链为空".to_string())?;
        if members.is_empty() {
            active.remove(&id);
            return Err("代理链为空".to_string());
        }
        let mut member_ids = Vec::with_capacity(members.len());
        let mut seen_members = HashSet::new();
        for m in members {
            let mid = m
                .as_i64()
                .ok_or_else(|| "代理链节点 ID 无效".to_string())?;
            if !seen_members.insert(mid) {
                active.remove(&id);
                return Err("代理链不能重复经过同一节点".to_string());
            }
            member_ids.push(mid);
        }
        let mut acc = Vec::new();
        for &mid in member_ids.iter().rev() {
            let sub = raw_chain_ids(data, mid, active, allow_missing, nodes_by_id)?;
            acc.extend(sub);
        }
        acc
    } else {
        if otype == "custom" {
            active.remove(&id);
            return Err("完整自定义配置不能嵌入代理链".to_string());
        }
        vec![id]
    };
    active.remove(&id);
    let uniq: HashSet<i64> = ids.iter().copied().collect();
    if uniq.len() != ids.len() {
        return Err("代理链不能重复经过同一节点".to_string());
    }
    Ok(ids)
}

pub fn raw_route_ids(
    data: &AppData,
    path: &[i64],
    active: &mut HashSet<i64>,
    nodes_by_id: &HashMap<i64, &Node>,
    allow_missing: bool,
) -> Result<Vec<i64>, String> {
    if path.is_empty() {
        return Ok(Vec::new());
    }
    if active.len() >= 64 || path.iter().any(|id| active.contains(id)) {
        return Err("代理链引用循环或过深".to_string());
    }
    for (index, &id) in path.iter().enumerate() {
        let node = nodes_by_id
            .get(&id)
            .ok_or_else(|| format!("代理链节点不存在: {id}"))?;
        if index > 0 && parse_obj(&node.outbound).contains_key("detour") {
            return Err("代理链与节点原有 detour 冲突".to_string());
        }
    }
    let first = nodes_by_id
        .get(&path[0])
        .ok_or_else(|| format!("代理链节点不存在: {}", path[0]))?;
    let detour = parse_obj(&first.outbound)
        .get("detour")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let upstream = if detour.trim().is_empty() || detour == "direct" {
        Vec::new()
    } else {
        let source = data.nodes.iter().find(|n| {
            n.group_id == first.group_id
                && parse_obj(&n.metadata)
                    .get("sourceTag")
                    .and_then(Value::as_str)
                    == Some(detour.as_str())
        });
        match source {
            None => Vec::new(),
            Some(src) => {
                for &id in path {
                    active.insert(id);
                }
                let mut chain_active = HashSet::new();
                let src_ids =
                    raw_chain_ids(data, src.id, &mut chain_active, allow_missing, nodes_by_id)?;
                let res = raw_route_ids(data, &src_ids, active, nodes_by_id, allow_missing);
                for &id in path {
                    active.remove(&id);
                }
                res?
            }
        }
    };
    let mut result = upstream;
    result.extend_from_slice(path);
    let uniq: HashSet<i64> = result.iter().copied().collect();
    if uniq.len() != result.len() {
        return Err("代理链不能重复经过同一节点".to_string());
    }
    Ok(result)
}

pub fn validate_chain_node_settings(
    data: &AppData,
    node: &Node,
    nodes_by_id: &HashMap<i64, &Node>,
) -> Result<(), String> {
    if outbound_type(node) == "custom" {
        return Ok(());
    }
    let mut act = HashSet::new();
    let body = raw_chain_ids(data, node.id, &mut act, true, nodes_by_id)?;
    let t = effective_chain_targets(data, node);
    let front_nodes = chain_target_nodes(data, &t.front, true)?;
    let mut fronts: Vec<Vec<i64>> = Vec::new();
    for fn_node in front_nodes {
        let mut a = HashSet::new();
        fronts.push(raw_chain_ids(data, fn_node.id, &mut a, true, nodes_by_id)?);
    }
    if fronts.is_empty() {
        fronts.push(Vec::new());
    }
    let landing_nodes = chain_target_nodes(data, &t.landing, true)?;
    let mut landings: Vec<Vec<i64>> = Vec::new();
    for ln_node in landing_nodes {
        let mut a = HashSet::new();
        landings.push(raw_chain_ids(data, ln_node.id, &mut a, true, nodes_by_id)?);
    }
    if landings.is_empty() {
        landings.push(Vec::new());
    }

    let mut front_ids = HashSet::new();
    for f in &fronts {
        let mut a = HashSet::new();
        for id in raw_route_ids(data, f, &mut a, nodes_by_id, true)? {
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
        raw_route_ids(data, &body, &mut a, nodes_by_id, true)?
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

    let check_path = |path: &[i64], has_upstream: bool| -> Result<(), String> {
        for (index, &id) in path.iter().enumerate() {
            if let Some(n) = nodes_by_id.get(&id) {
                if (index > 0 || has_upstream) && parse_obj(&n.outbound).contains_key("detour") {
                    return Err("代理链与节点原有 detour 冲突".to_string());
                }
            }
        }
        Ok(())
    };
    for f in &fronts {
        check_path(f, false)?;
    }
    check_path(&body, !front_ids.is_empty())?;
    for l in &landings {
        check_path(l, true)?;
    }
    Ok(())
}

pub fn validate_chain_settings(data: &AppData) -> Result<(), String> {
    validate_chain_target_syntax(data)?;
    let nodes_by_id: HashMap<i64, &Node> = data.nodes.iter().map(|n| (n.id, n)).collect();
    for n in &data.nodes {
        validate_chain_node_settings(data, n, &nodes_by_id)?;
    }
    Ok(())
}

pub fn derived_group_unavailable_reason(data: &AppData, group: &Group) -> Option<String> {
    let source_id = derived_group_source_id(group);
    if source_id == 0 {
        return None;
    }
    if !data.groups.iter().any(|g| g.id == source_id) {
        return Some("源分组已删除".to_string());
    }
    if !data.nodes.iter().any(|n| n.group_id == source_id) {
        return Some("源分组没有节点".to_string());
    }
    None
}

pub fn chain_unavailable_reason(data: &AppData, node: &Node) -> Option<String> {
    let groups: HashMap<i64, &Group> = data.groups.iter().map(|g| (g.id, g)).collect();
    let nodes: HashMap<i64, &Node> = data.nodes.iter().map(|n| (n.id, n)).collect();
    if let Some(g) = groups.get(&node.group_id) {
        if let Some(reason) = derived_group_unavailable_reason(data, g) {
            return Some(reason);
        }
    }
    let mut visited = HashSet::new();
    fn raw(
        id: i64,
        data: &AppData,
        nodes: &HashMap<i64, &Node>,
        visited: &mut HashSet<i64>,
    ) -> Option<String> {
        let source = match nodes.get(&id).copied() {
            Some(s) => s,
            None => return Some("链路引用的节点已删除".to_string()),
        };
        if !visited.insert(id) {
            return None;
        }
        let bean = parse_obj(&source.outbound);
        if bean.get("type").and_then(Value::as_str) == Some("chain") {
            if let Some(members) = bean.get("node_ids").and_then(Value::as_array) {
                for m in members {
                    if let Some(mid) = m.as_i64() {
                        if let Some(r) = raw(mid, data, nodes, visited) {
                            return Some(r);
                        }
                    }
                }
            }
        }
        let detour = bean.get("detour").and_then(Value::as_str).unwrap_or("");
        if !detour.trim().is_empty() && detour != "direct" {
            let upstream = data.nodes.iter().find(|n| {
                n.group_id == source.group_id
                    && parse_obj(&n.metadata)
                        .get("sourceTag")
                        .and_then(Value::as_str)
                        == Some(detour)
            });
            match upstream {
                None => return Some("原始上游节点已失效".to_string()),
                Some(u) => {
                    if let Some(r) = raw(u.id, data, nodes, visited) {
                        return Some(r);
                    }
                }
            }
        }
        None
    }

    if let Some(r) = raw(node.id, data, &nodes, &mut visited) {
        return Some(r);
    }
    let targets = effective_chain_targets(data, node);
    for target in [&targets.front, &targets.landing] {
        if let Some(rest) = target.strip_prefix("node:") {
            if let Ok(id) = rest.parse::<i64>() {
                if let Some(r) = raw(id, data, &nodes, &mut visited) {
                    return Some(r);
                }
            }
        } else if let Some(rest) = target.strip_prefix("group:") {
            if let Ok(id) = rest.parse::<i64>() {
                if !groups.contains_key(&id) {
                    return Some("链路引用的分组已删除".to_string());
                }
                let members: Vec<&Node> = data.nodes.iter().filter(|n| n.group_id == id).collect();
                if members.is_empty() {
                    return Some("链路引用的分组没有节点".to_string());
                }
                for m in members {
                    if let Some(r) = raw(m.id, data, &nodes, &mut visited) {
                        return Some(r);
                    }
                }
            }
        }
    }
    None
}

pub fn chain_path_labels(data: &AppData, node_id: i64) -> Vec<String> {
    let Some(node) = data.nodes.iter().find(|n| n.id == node_id) else {
        return vec!["节点已失效".to_string()];
    };
    let nodes_by_id: HashMap<i64, &Node> = data.nodes.iter().map(|n| (n.id, n)).collect();
    let label = |target: &str| -> Vec<String> {
        if target == "none" {
            return Vec::new();
        }
        if let Some(rest) = target.strip_prefix("group:") {
            let gid = rest.parse::<i64>().ok();
            let gname = gid
                .and_then(|id| data.groups.iter().find(|g| g.id == id))
                .map(|g| g.name.as_str())
                .unwrap_or("分组已失效");
            return vec![format!("{gname} · 自动")];
        }
        let nid = target
            .split_once(':')
            .and_then(|(_, r)| r.parse::<i64>().ok());
        if let Some(id) = nid {
            let mut act = HashSet::new();
            if let Ok(ids) = raw_chain_ids(data, id, &mut act, false, &nodes_by_id) {
                if ids.iter().all(|i| nodes_by_id.contains_key(i)) {
                    return ids
                        .into_iter()
                        .map(|i| nodes_by_id[&i].name.clone())
                        .collect();
                }
            }
        }
        vec!["节点已失效".to_string()]
    };
    let t = effective_chain_targets(data, node);
    let mut out = label(&t.front);
    let mut act = HashSet::new();
    match raw_chain_ids(data, node.id, &mut act, false, &nodes_by_id) {
        Ok(ids) if ids.iter().all(|i| nodes_by_id.contains_key(i)) => {
            out.extend(ids.into_iter().map(|i| nodes_by_id[&i].name.clone()));
        }
        _ => out.push(format!("{} · 链路失效", node.name)),
    }
    out.extend(label(&t.landing));
    out
}

pub fn validate_derived_chains(data: &AppData) -> Result<(), String> {
    let groups: HashMap<i64, &Group> = data.groups.iter().map(|g| (g.id, g)).collect();
    for g in &data.groups {
        let opts = parse_obj(&g.options);
        if opts.contains_key("chainSourceGroupId") {
            let source_id = derived_group_source_id(g);
            if source_id <= 0 || source_id == g.id || !g.subscription_url.trim().is_empty() {
                return Err("派生分组来源或订阅地址无效".to_string());
            }
            if let Some(src) = groups.get(&source_id) {
                if derived_group_source_id(src) != 0 {
                    return Err("暂不支持多层派生分组".to_string());
                }
            }
        }
    }
    for n in &data.nodes {
        let meta = parse_obj(&n.metadata);
        if meta.contains_key("chainSourceNodeId")
            && (derived_node_source_id(n) <= 0 || outbound_type(n) != "chain")
        {
            return Err("派生节点来源无效".to_string());
        }
    }
    Ok(())
}

fn largest_record_id(data: &AppData) -> i64 {
    data.nodes
        .iter()
        .map(|n| n.id)
        .chain(data.groups.iter().map(|g| g.id))
        .chain(data.rules.iter().map(|r| r.id))
        .chain(data.merges.iter().map(|m| m.id))
        .max()
        .unwrap_or(0)
}

fn derived_outbound(source_id: i64) -> String {
    json!({ "type": "chain", "node_ids": [source_id] }).to_string()
}

pub fn create_derived_chain_node(
    data: &AppData,
    source_id: i64,
    name: &str,
    targets: &ChainTargets,
) -> Result<AppData, String> {
    let mut next_id = largest_record_id(data);
    let mut alloc = || {
        next_id += 1;
        next_id
    };
    create_derived_chain_node_with_alloc(data, source_id, name, targets, &mut alloc)
}

pub fn create_derived_chain_node_with_alloc(
    data: &AppData,
    source_id: i64,
    name: &str,
    targets: &ChainTargets,
    alloc: &mut dyn FnMut() -> i64,
) -> Result<AppData, String> {
    let source = data
        .nodes
        .iter()
        .find(|n| n.id == source_id)
        .ok_or_else(|| "源节点已删除".to_string())?;
    if name.trim().is_empty() {
        return Err("链式节点名称不能为空".to_string());
    }
    if outbound_type(source) == "custom" {
        return Err("完整自定义配置不能创建链式节点".to_string());
    }
    if !data.groups.iter().any(|g| g.id == source.group_id) {
        return Err("源节点分组不存在".to_string());
    }
    let existing_dest = data.groups.iter().find(|g| {
        g.enabled
            && parse_obj(&g.options)
                .get("chainNodeContainer")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            && derived_group_source_id(g) == 0
    });
    let (destination, is_new_dest) = match existing_dest {
        Some(g) => (g.clone(), false),
        None => {
            let max_order = data.groups.iter().map(|g| g.order).max().unwrap_or(-1);
            (
                Group {
                    id: alloc(),
                    name: "链式节点".to_string(),
                    subscription_url: String::new(),
                    enabled: true,
                    order: max_order + 1,
                    updated_at: 0,
                    user_info: String::new(),
                    front_proxy: 0,
                    landing_proxy: 0,
                    options: r#"{"chainNodeContainer":true}"#.to_string(),
                },
                true,
            )
        }
    };
    let max_node_order = data
        .nodes
        .iter()
        .filter(|n| n.group_id == destination.id)
        .map(|n| n.order)
        .max()
        .unwrap_or(-1);
    let node = node_with_chain_targets(
        &Node {
            id: alloc(),
            group_id: destination.id,
            name: name.trim().to_string(),
            outbound: derived_outbound(source_id),
            share_link: String::new(),
            ping: -1,
            tx: 0,
            rx: 0,
            order: max_node_order + 1,
            status: 0,
            metadata: json!({ "chainSourceNodeId": source_id }).to_string(),
        },
        targets,
    );
    let mut result = data.clone();
    if is_new_dest {
        result.groups.push(destination);
    }
    result.nodes.push(node.clone());
    validate_chain_settings(&result)?;
    if let Some(reason) = chain_unavailable_reason(&result, &node) {
        return Err(reason);
    }
    Ok(result)
}

pub fn create_derived_chain_group(
    data: &AppData,
    source_group_id: i64,
    name: &str,
    targets: &ChainTargets,
) -> Result<AppData, String> {
    let mut next_id = largest_record_id(data);
    let mut alloc = || {
        next_id += 1;
        next_id
    };
    create_derived_chain_group_with_alloc(data, source_group_id, name, targets, &mut alloc)
}

pub fn create_derived_chain_group_with_alloc(
    data: &AppData,
    source_group_id: i64,
    name: &str,
    targets: &ChainTargets,
    alloc: &mut dyn FnMut() -> i64,
) -> Result<AppData, String> {
    let source = data
        .groups
        .iter()
        .find(|g| g.id == source_group_id)
        .ok_or_else(|| "源分组已删除".to_string())?;
    if derived_group_source_id(source) != 0 {
        return Err("暂不支持以派生分组继续创建派生分组".to_string());
    }
    if name.trim().is_empty() {
        return Err("链式分组名称不能为空".to_string());
    }
    if !data.nodes.iter().any(|n| n.group_id == source.id) {
        return Err("源分组没有节点".to_string());
    }
    let max_order = data.groups.iter().map(|g| g.order).max().unwrap_or(-1);
    let group = group_with_chain_targets(
        &Group {
            id: alloc(),
            name: name.trim().to_string(),
            subscription_url: String::new(),
            enabled: true,
            order: max_order + 1,
            updated_at: 0,
            user_info: String::new(),
            front_proxy: 0,
            landing_proxy: 0,
            options: json!({ "chainSourceGroupId": source.id }).to_string(),
        },
        targets,
    );
    let group_id = group.id;
    let mut next = data.clone();
    next.groups.push(group);
    let result = sync_derived_chains_with_alloc(&next, alloc)?;
    validate_chain_settings(&result)?;
    for member in result.nodes.iter().filter(|n| n.group_id == group_id) {
        if let Some(reason) = chain_unavailable_reason(&result, member) {
            return Err(reason);
        }
    }
    Ok(result)
}

pub fn sync_derived_chains(data: &AppData) -> Result<AppData, String> {
    let mut next_id = largest_record_id(data);
    let mut alloc = || {
        next_id += 1;
        next_id
    };
    sync_derived_chains_with_alloc(data, &mut alloc)
}

pub fn sync_derived_chains_with_alloc(
    data: &AppData,
    alloc: &mut dyn FnMut() -> i64,
) -> Result<AppData, String> {
    if !data.groups.iter().any(|g| derived_group_source_id(g) > 0) {
        return Ok(data.clone());
    }
    let mut by_group: HashMap<i64, Vec<&Node>> = HashMap::new();
    for n in &data.nodes {
        by_group.entry(n.group_id).or_default().push(n);
    }
    let groups_by_id: HashMap<i64, &Group> = data.groups.iter().map(|g| (g.id, g)).collect();
    let mut members: HashMap<i64, Vec<Node>> = HashMap::new();
    let mut member_group_order: Vec<i64> = Vec::new();
    let mut updated_groups = Vec::with_capacity(data.groups.len());

    for group in &data.groups {
        let source_id = derived_group_source_id(group);
        if source_id == 0 {
            updated_groups.push(group.clone());
            continue;
        }
        let source = groups_by_id.get(&source_id).copied();
        if source_id == group.id || source.is_some_and(|s| derived_group_source_id(s) != 0) {
            return Err("派生分组不能引用自身或另一派生分组".to_string());
        }
        let mut opts = parse_obj(&group.options);
        if source.is_none() {
            opts.insert("chainSourceMissing".into(), Value::Bool(true));
        } else {
            opts.shift_remove("chainSourceMissing");
            let old = by_group.get(&group.id).cloned().unwrap_or_default();
            let mut generated: HashMap<i64, &Node> = HashMap::new();
            for n in &old {
                let sid = derived_node_source_id(n);
                if sid > 0 {
                    generated.entry(sid).or_insert(n);
                }
            }
            let mut order = old.iter().map(|n| n.order).max().unwrap_or(-1) + 1;
            let mut group_members: Vec<Node> = old
                .iter()
                .filter(|n| derived_node_source_id(n) == 0)
                .map(|n| (*n).clone())
                .collect();
            for src_node in by_group.get(&source_id).cloned().unwrap_or_default() {
                if let Some(prev) = generated.get(&src_node.id) {
                    let mut meta = parse_obj(&prev.metadata);
                    let old_source_name = meta
                        .get("chainSourceName")
                        .and_then(Value::as_str)
                        .unwrap_or(&prev.name)
                        .to_string();
                    meta.insert(
                        "chainSourceName".into(),
                        Value::String(src_node.name.clone()),
                    );
                    let mut updated = (*prev).clone();
                    if prev.name == old_source_name {
                        updated.name = src_node.name.clone();
                    }
                    updated.outbound = derived_outbound(src_node.id);
                    updated.metadata = Value::Object(meta).to_string();
                    group_members.push(updated);
                } else {
                    group_members.push(Node {
                        id: alloc(),
                        group_id: group.id,
                        name: src_node.name.clone(),
                        outbound: derived_outbound(src_node.id),
                        share_link: String::new(),
                        ping: -1,
                        tx: 0,
                        rx: 0,
                        order,
                        status: 0,
                        metadata: json!({
                            "chainSourceNodeId": src_node.id,
                            "chainSourceName": src_node.name,
                        })
                        .to_string(),
                    });
                    order += 1;
                }
            }
            member_group_order.push(group.id);
            members.insert(group.id, group_members);
        }
        let val = Value::Object(opts).to_string();
        if group.options == val {
            updated_groups.push(group.clone());
        } else {
            let mut g2 = group.clone();
            g2.options = val;
            updated_groups.push(g2);
        }
    }

    let mut updated_nodes: Vec<Node> = data
        .nodes
        .iter()
        .filter(|n| !members.contains_key(&n.group_id))
        .cloned()
        .collect();
    for gid in member_group_order {
        if let Some(list) = members.get(&gid) {
            updated_nodes.extend(list.iter().cloned());
        }
    }

    let updated_ids: HashSet<i64> = updated_nodes.iter().map(|n| n.id).collect();
    let mut removed_groups: HashMap<i64, i64> = HashMap::new();
    let mut removed_ids: HashSet<i64> = HashSet::new();
    for n in &data.nodes {
        if members.contains_key(&n.group_id) && !updated_ids.contains(&n.id) {
            removed_ids.insert(n.id);
            removed_groups.insert(n.id, n.group_id);
        }
    }

    let mut updated_settings = data.settings.clone();
    for (key, value) in updated_settings.iter_mut() {
        if key.starts_with("smart.") && key.ends_with(".target") {
            if let Some(id) = value.strip_prefix("node:").and_then(|s| s.parse::<i64>().ok()) {
                if let Some(&gid) = removed_groups.get(&id) {
                    *value = format!("group:{gid}");
                }
            }
        }
    }

    let selected_group = groups_by_id
        .get(&data.selected_group_id())
        .copied()
        .filter(|g| derived_group_source_id(g) > 0)
        .or_else(|| {
            data.nodes
                .iter()
                .find(|n| n.id == data.selected_node_id())
                .and_then(|n| groups_by_id.get(&n.group_id).copied())
                .filter(|g| derived_group_source_id(g) > 0)
        });
    if let Some(sel_g) = selected_group {
        let sel_node_id = data.selected_node_id();
        if !updated_nodes
            .iter()
            .any(|n| n.id == sel_node_id && n.group_id == sel_g.id)
        {
            let replacement = updated_nodes
                .iter()
                .filter(|n| n.group_id == sel_g.id)
                .min_by_key(|n| n.order);
            updated_settings.insert(
                "selectedNodeId".into(),
                replacement.map(|n| n.id).unwrap_or(0).to_string(),
            );
            updated_settings.insert("selectedGroupId".into(), sel_g.id.to_string());
        }
    }

    let updated_rules = data
        .rules
        .iter()
        .map(|r| {
            if let Some(id) = r
                .outbound
                .strip_prefix("node:")
                .and_then(|s| s.parse::<i64>().ok())
            {
                if let Some(&gid) = removed_groups.get(&id) {
                    let mut r2 = r.clone();
                    r2.outbound = format!("group:{gid}");
                    return r2;
                }
            }
            r.clone()
        })
        .collect();

    let updated_merges = data
        .merges
        .iter()
        .map(|m| {
            let mut m2 = m.clone();
            m2.node_ids.retain(|id| !removed_ids.contains(id));
            if removed_ids.contains(&m2.selected_id) {
                m2.selected_id = 0;
            }
            m2
        })
        .collect();

    let result = AppData {
        nodes: updated_nodes,
        groups: updated_groups,
        rules: updated_rules,
        merges: updated_merges,
        settings: updated_settings,
    };
    if removed_ids.is_empty() {
        Ok(result)
    } else {
        Ok(result.without_stale_node_settings())
    }
}
