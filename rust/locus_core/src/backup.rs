use crate::chain_routing::{validate_chain_target_syntax, validate_derived_chains};
use crate::json_depth::require_json_depth;
use crate::models::AppData;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const MAX_BACKUP: usize = 64 * 1024 * 1024;
const LOG_TAIL_MAX_LINES: usize = 500;
const LOG_TAIL_MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupScope {
    pub configurations: bool,
    pub rules: bool,
    pub settings: bool,
}

pub fn validate_app_data(data: &AppData, previous: Option<&AppData>) -> Result<(), String> {
    if data.nodes.len() > 10_000
        || data.groups.len() > 10_000
        || data.rules.len() > 100_000
        || data.merges.len() > 10_000
    {
        return Err("数据超出记录数量限制".to_string());
    }
    let check_ids = |ids: &[i64]| -> Result<(), String> {
        let mut seen = HashSet::with_capacity(ids.len());
        for &id in ids {
            if id <= 0 || !seen.insert(id) {
                return Err("无效或重复记录ID".to_string());
            }
        }
        Ok(())
    };
    check_ids(&data.nodes.iter().map(|n| n.id).collect::<Vec<_>>())?;
    check_ids(&data.groups.iter().map(|g| g.id).collect::<Vec<_>>())?;
    check_ids(&data.rules.iter().map(|r| r.id).collect::<Vec<_>>())?;
    check_ids(&data.merges.iter().map(|m| m.id).collect::<Vec<_>>())?;

    let group_ids: HashSet<i64> = data.groups.iter().map(|g| g.id).collect();
    let node_ids: HashSet<i64> = data.nodes.iter().map(|n| n.id).collect();
    let merge_ids: HashSet<i64> = data.merges.iter().map(|m| m.id).collect();
    let old_outbounds: HashMap<i64, &str> = previous
        .map(|p| p.nodes.iter().map(|n| (n.id, n.outbound.as_str())).collect())
        .unwrap_or_default();

    for n in &data.nodes {
        if !group_ids.contains(&n.group_id) {
            return Err("节点分组不存在".to_string());
        }
        if old_outbounds.get(&n.id).copied() != Some(n.outbound.as_str()) {
            let val: Value =
                serde_json::from_str(&n.outbound).map_err(|e| format!("节点 JSON 无效: {e}"))?;
            let otype = val
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("");
            if otype.trim().is_empty() {
                return Err("节点缺少协议类型".to_string());
            }
        }
    }
    for g in &data.groups {
        if g.front_proxy < 0 || g.landing_proxy < 0 {
            return Err("前置或落地节点引用无效".to_string());
        }
    }
    validate_chain_target_syntax(data)?;
    validate_derived_chains(data)?;

    for m in &data.merges {
        if !m.node_ids.iter().all(|id| node_ids.contains(id))
            || !m.group_ids.iter().all(|id| group_ids.contains(id))
        {
            return Err("合并组成员不存在".to_string());
        }
        if m.selected_id != 0 && !node_ids.contains(&m.selected_id) {
            return Err("合并组默认节点不存在".to_string());
        }
    }
    for r in &data.rules {
        let target = r.outbound.as_str();
        let valid = matches!(target, "proxy" | "direct" | "block")
            || target
                .strip_prefix("node:")
                .and_then(|s| s.parse::<i64>().ok())
                .is_some_and(|id| node_ids.contains(&id))
            || target
                .strip_prefix("group:")
                .and_then(|s| s.parse::<i64>().ok())
                .is_some_and(|id| group_ids.contains(&id))
            || target
                .strip_prefix("merge:")
                .and_then(|s| s.parse::<i64>().ok())
                .is_some_and(|id| merge_ids.contains(&id));
        if !valid {
            return Err(format!("规则 {} 的目标不存在", r.name));
        }
    }
    let sel_node = data.selected_node_id();
    if sel_node != 0 && !node_ids.contains(&sel_node) {
        return Err("默认节点不存在".to_string());
    }
    let sel_group = data.selected_group_id();
    if sel_group != 0 && !group_ids.contains(&sel_group) {
        return Err("默认节点组不存在".to_string());
    }
    Ok(())
}

pub fn validate_backup_json(text: &str) -> Result<(), String> {
    if text.len() > MAX_BACKUP {
        return Err("备份JSON超过64MiB".to_string());
    }
    require_json_depth(text)?;
    let val: Value =
        serde_json::from_str(text).map_err(|_| "备份JSON必须是单个对象".to_string())?;
    if !val.is_object() {
        return Err("备份JSON必须是单个对象".to_string());
    }
    Ok(())
}

pub fn backup_selection(data: &AppData, scope: &BackupScope) -> Result<AppData, String> {
    if !scope.configurations && !scope.rules && !scope.settings {
        return Err("至少选择一项备份内容".to_string());
    }
    let rules = if scope.rules {
        data.rules.clone()
    } else {
        Vec::new()
    };
    if !scope.configurations
        && !rules
            .iter()
            .all(|r| matches!(r.outbound.as_str(), "proxy" | "direct" | "block"))
    {
        return Err("路由规则引用节点或分组，请同时勾选组与配置".to_string());
    }
    let mut settings = if scope.settings {
        data.settings.clone()
    } else {
        Default::default()
    };
    if !scope.configurations {
        let drop_exact = [
            "selectedNodeId",
            "selectedGroupId",
            "browseGroupId",
            "smartSourceGroupId",
            "smartSourceMergeId",
        ];
        settings.retain(|k, _| {
            !drop_exact.contains(&k.as_str())
                && !k.starts_with("nodeRegion.")
                && !k.starts_with("sort_group_")
                && !k.starts_with("sort_mode_group_")
        });
        for (k, v) in settings.iter_mut() {
            if k.starts_with("smart.")
                && k.ends_with(".target")
                && (v.starts_with("node:") || v.starts_with("group:") || v.starts_with("merge:"))
            {
                *v = "off".to_string();
            }
        }
    }
    let out = AppData {
        nodes: if scope.configurations {
            data.nodes.clone()
        } else {
            Vec::new()
        },
        groups: if scope.configurations {
            data.groups.clone()
        } else {
            Vec::new()
        },
        rules,
        merges: if scope.configurations {
            data.merges.clone()
        } else {
            Vec::new()
        },
        settings,
    };
    validate_app_data(&out, None)?;
    Ok(out)
}

pub fn read_log_tail_file(path: &str, max_bytes: usize) -> Result<Vec<String>, String> {
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};
    if !(1..=LOG_TAIL_MAX_BYTES).contains(&max_bytes) {
        return Err(format!("maxBytes must be between 1 and {LOG_TAIL_MAX_BYTES}"));
    }
    let meta = match std::fs::metadata(path) {
        Ok(m) if m.is_file() => m,
        _ => return Ok(Vec::new()),
    };
    let length = meta.len();
    let start = length.saturating_sub(max_bytes as u64);
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let starts_at_line_boundary = if start == 0 {
        true
    } else {
        file.seek(SeekFrom::Start(start - 1))
            .map_err(|e| e.to_string())?;
        let mut prev = [0u8; 1];
        let n = file.read(&mut prev).map_err(|e| e.to_string())?;
        n == 1 && (prev[0] == b'\n' || prev[0] == b'\r')
    };
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::with_capacity((length - start).min(max_bytes as u64) as usize);
    file.take(max_bytes as u64)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    let decoded = decode_utf8_ignore_malformed(&buf);
    let mut lines: Vec<String> = decoded
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect();
    if !starts_at_line_boundary && !lines.is_empty() {
        lines.remove(0);
    }
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    if lines.len() > LOG_TAIL_MAX_LINES {
        let drop = lines.len() - LOG_TAIL_MAX_LINES;
        lines.drain(0..drop);
    }
    Ok(lines)
}

fn decode_utf8_ignore_malformed(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match std::str::from_utf8(&bytes[i..]) {
            Ok(valid) => {
                out.push_str(valid);
                break;
            }
            Err(err) => {
                let valid_len = err.valid_up_to();
                if valid_len > 0 {
                    out.push_str(&String::from_utf8_lossy(&bytes[i..i + valid_len]));
                }
                let skip = err.error_len().unwrap_or(bytes.len() - (i + valid_len));
                i += valid_len + skip.max(1);
            }
        }
    }
    out
}
