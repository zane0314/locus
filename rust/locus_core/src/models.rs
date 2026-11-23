use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: i64,
    #[serde(rename = "groupId")]
    pub group_id: i64,
    pub name: String,
    pub outbound: String,
    #[serde(rename = "shareLink", default)]
    pub share_link: String,
    #[serde(default = "default_ping")]
    pub ping: i32,
    #[serde(default)]
    pub tx: i64,
    #[serde(default)]
    pub rx: i64,
    #[serde(default)]
    pub order: i32,
    #[serde(default)]
    pub status: i32,
    #[serde(default = "default_empty_obj")]
    pub metadata: String,
}

fn default_ping() -> i32 {
    -1
}

fn default_empty_obj() -> String {
    "{}".to_string()
}

fn default_true() -> bool {
    true
}

fn default_proxy() -> String {
    "proxy".to_string()
}

fn default_selector() -> String {
    "selector".to_string()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: i64,
    pub name: String,
    #[serde(rename = "subscriptionUrl", default)]
    pub subscription_url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub order: i32,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: i64,
    #[serde(rename = "userInfo", default)]
    pub user_info: String,
    #[serde(rename = "frontProxy", default)]
    pub front_proxy: i64,
    #[serde(rename = "landingProxy", default)]
    pub landing_proxy: i64,
    #[serde(default = "default_empty_obj")]
    pub options: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteRule {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub domains: String,
    #[serde(default)]
    pub packages: String,
    #[serde(rename = "ipCidrs", default)]
    pub ip_cidrs: String,
    #[serde(default = "default_proxy")]
    pub outbound: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub order: i32,
    #[serde(default = "default_empty_obj")]
    pub advanced: String,
    #[serde(default)]
    pub prioritize: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeGroup {
    pub id: i64,
    pub name: String,
    #[serde(rename = "nodeIds", default)]
    pub node_ids: Vec<i64>,
    #[serde(rename = "groupIds", default)]
    pub group_ids: Vec<i64>,
    #[serde(default = "default_selector")]
    pub mode: String,
    #[serde(rename = "selectedId", default)]
    pub selected_id: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppData {
    pub nodes: Vec<Node>,
    pub groups: Vec<Group>,
    pub rules: Vec<RouteRule>,
    pub merges: Vec<MergeGroup>,
    pub settings: IndexMap<String, String>,
}

pub const TEST_TIMEOUT_MS: i32 = 4000;
pub const TEST_CONCURRENCY: i32 = 12;

pub fn setting_default(key: &str) -> &'static str {
    match key {
        "autoStart" => "false",
        "statsEnabled" => "true",
        "showDirectSpeed" => "false",
        "showGroupInNotification" => "false",
        "confirmProfileDelete" => "true",
        "alwaysShowAddress" => "false",
        "meteredNetwork" => "false",
        "acquireWakeLock" => "false",
        "hideFromRecentApps" => "false",
        "bypassLan" => "false",
        "bypassLanInCore" => "false",
        "concurrentDial" => "false",
        "tcpConcurrent" => "true",
        "unifiedDelay" => "true",
        "autoTestSelectedNode" => "true",
        "sniff" => "true",
        "resolveDestination" => "false",
        "enableTLSFragment" => "false",
        "enableDnsRouting" => "true",
        "fakeDns" => "true",
        "allowLan" => "false",
        "shareEnabled" => "false",
        "disableMixedInbound" => "false",
        "appendHttpProxy" => "false",
        "strictRoute" => "true",
        "muxEnabled" => "false",
        "muxPadding" => "false",
        "clashApi" => "false",
        "networkReset" => "true",
        "wakeResetConnections" => "false",
        "globalAllowInsecure" => "false",
        "allowInsecureOnRequest" => "false",
        "showBottomBar" => "true",
        "uiSkin" => "linear",
        "launcherIcon" => "default",
        "theme" => "dark",
        "proxyCore" => "sing-box",
        "appLanguage" => "",
        "serviceMode" => "vpn",
        "tunStack" => "mixed",
        "mtu" => "1500",
        "speedInterval" => "1000",
        "logLevel" => "warn",
        "routeMode" => "rule",
        "perAppEnabled" => "false",
        "ipv6Mode" => "ipv4_only",
        "rulesProvider" => "0",
        "rulesGeositeUrl" => {
            "https://github.com/SagerNet/sing-geosite/releases/latest/download/geosite.db"
        }
        "rulesGeoipUrl" => {
            "https://github.com/SagerNet/sing-geoip/releases/latest/download/geoip.db"
        }
        "rulesUpdateInterval" => "24h",
        "urlTestInterval" => "5m",
        "urlTestTolerance" => "5",
        "autoTestPowerMode" => "normal",
        "fragmentLength" => "100-200",
        "fragmentInterval" => "10-20",
        "dnsRemote" => "https://1.1.1.1/dns-query",
        "dnsStrategyRemote" => "",
        "dnsDirect" => "local",
        "dnsStrategyDirect" => "",
        "dnsStrategyServer" => "",
        "sharePort" => "2081",
        "mixedPort" => "2080",
        "muxProtocol" => "h2mux",
        "muxMaxStreams" => "8",
        "testUrl" => "https://www.gstatic.com/generate_204",
        "testTimeout" => "4000",
        "testConcurrency" => "12",
        "apiPort" => "9090",
        "appTLSVersion" => "1.2",
        _ => "",
    }
}

impl AppData {
    pub fn setting<'a>(&'a self, key: &str) -> &'a str {
        self.settings
            .get(key)
            .map(String::as_str)
            .unwrap_or_else(|| setting_default(key))
    }

    pub fn setting_or<'a>(&'a self, key: &str, fallback: &'a str) -> &'a str {
        self.settings
            .get(key)
            .map(String::as_str)
            .unwrap_or(fallback)
    }

    pub fn bool_setting(&self, key: &str, default: Option<bool>) -> bool {
        if let Some(val) = self.settings.get(key) {
            match val.as_str() {
                "true" => return true,
                "false" => return false,
                _ => {}
            }
        }
        match default {
            Some(d) => d,
            None => setting_default(key) == "true",
        }
    }

    pub fn selected_node_id(&self) -> i64 {
        self.setting_or("selectedNodeId", "0")
            .parse::<i64>()
            .unwrap_or(0)
    }

    pub fn selected_group_id(&self) -> i64 {
        self.setting_or("selectedGroupId", "0")
            .parse::<i64>()
            .unwrap_or(0)
    }

    /// Parses the canonical AppData format=1 JSON wire format (where `outbound`/`metadata`/`options`/`advanced` are nested JSON objects).
    pub fn from_json(text: &str) -> Result<Self, String> {
        if text.len() > 64 * 1024 * 1024 {
            return Err("数据超过64MiB".to_string());
        }
        let root: Value = serde_json::from_str(text).map_err(|e| format!("JSON 解析失败: {e}"))?;
        let obj = root
            .as_object()
            .ok_or_else(|| "数据根节点必须是对象".to_string())?;
        let format = obj.get("format").and_then(Value::as_i64).unwrap_or(1);
        if format != 1 {
            return Err("不支持的数据版本".to_string());
        }

        let nodes_arr = obj
            .get("nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if nodes_arr.len() > 100_000 {
            return Err("数据超出记录数量限制".to_string());
        }
        let mut nodes = Vec::with_capacity(nodes_arr.len());
        for item in nodes_arr {
            let o = item
                .as_object()
                .ok_or_else(|| "节点格式无效".to_string())?;
            let id = o
                .get("id")
                .and_then(Value::as_i64)
                .ok_or_else(|| "节点缺少 id".to_string())?;
            let group_id = o
                .get("groupId")
                .and_then(Value::as_i64)
                .ok_or_else(|| "节点缺少 groupId".to_string())?;
            let name = o
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "节点缺少 name".to_string())?
                .to_string();
            let outbound = match o.get("outbound") {
                Some(Value::Object(m)) => Value::Object(m.clone()).to_string(),
                Some(Value::String(s)) => s.clone(),
                _ => return Err("节点缺少 outbound".to_string()),
            };
            let metadata = match o.get("metadata") {
                Some(Value::Object(m)) => Value::Object(m.clone()).to_string(),
                Some(Value::String(s)) => s.clone(),
                _ => "{}".to_string(),
            };
            nodes.push(Node {
                id,
                group_id,
                name,
                outbound,
                share_link: opt_str(o, "shareLink", ""),
                ping: o.get("ping").and_then(Value::as_i64).unwrap_or(-1) as i32,
                tx: o.get("tx").and_then(Value::as_i64).unwrap_or(0),
                rx: o.get("rx").and_then(Value::as_i64).unwrap_or(0),
                order: o.get("order").and_then(Value::as_i64).unwrap_or(0) as i32,
                status: o.get("status").and_then(Value::as_i64).unwrap_or(0) as i32,
                metadata,
            });
        }

        let groups_arr = obj
            .get("groups")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if groups_arr.len() > 100_000 {
            return Err("数据超出记录数量限制".to_string());
        }
        let mut groups = Vec::with_capacity(groups_arr.len());
        for item in groups_arr {
            let o = item
                .as_object()
                .ok_or_else(|| "分组格式无效".to_string())?;
            let id = o
                .get("id")
                .and_then(Value::as_i64)
                .ok_or_else(|| "分组缺少 id".to_string())?;
            let name = o
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "分组缺少 name".to_string())?
                .to_string();
            let options = match o.get("options") {
                Some(Value::Object(m)) => Value::Object(m.clone()).to_string(),
                Some(Value::String(s)) => s.clone(),
                _ => "{}".to_string(),
            };
            groups.push(Group {
                id,
                name,
                subscription_url: opt_str(o, "subscriptionUrl", ""),
                enabled: o.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                order: o.get("order").and_then(Value::as_i64).unwrap_or(0) as i32,
                updated_at: o.get("updatedAt").and_then(Value::as_i64).unwrap_or(0),
                user_info: opt_str(o, "userInfo", ""),
                front_proxy: o.get("frontProxy").and_then(Value::as_i64).unwrap_or(0),
                landing_proxy: o.get("landingProxy").and_then(Value::as_i64).unwrap_or(0),
                options,
            });
        }

        let rules_arr = obj
            .get("rules")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if rules_arr.len() > 100_000 {
            return Err("数据超出记录数量限制".to_string());
        }
        let mut rules = Vec::with_capacity(rules_arr.len());
        for item in rules_arr {
            let o = item
                .as_object()
                .ok_or_else(|| "规则格式无效".to_string())?;
            let id = o
                .get("id")
                .and_then(Value::as_i64)
                .ok_or_else(|| "规则缺少 id".to_string())?;
            let name = o
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "规则缺少 name".to_string())?
                .to_string();
            let advanced = match o.get("advanced") {
                Some(Value::Object(m)) => Value::Object(m.clone()).to_string(),
                Some(Value::String(s)) => s.clone(),
                _ => "{}".to_string(),
            };
            rules.push(RouteRule {
                id,
                name,
                domains: opt_str(o, "domains", ""),
                packages: opt_str(o, "packages", ""),
                ip_cidrs: opt_str(o, "ipCidrs", ""),
                outbound: opt_str(o, "outbound", "proxy"),
                enabled: o.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                order: o.get("order").and_then(Value::as_i64).unwrap_or(0) as i32,
                advanced,
                prioritize: o
                    .get("prioritize")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
        }

        let merges_arr = obj
            .get("merges")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if merges_arr.len() > 100_000 {
            return Err("数据超出记录数量限制".to_string());
        }
        let mut merges = Vec::with_capacity(merges_arr.len());
        for item in merges_arr {
            let o = item
                .as_object()
                .ok_or_else(|| "合并组格式无效".to_string())?;
            let id = o
                .get("id")
                .and_then(Value::as_i64)
                .ok_or_else(|| "合并组缺少 id".to_string())?;
            let name = o
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "合并组缺少 name".to_string())?
                .to_string();
            let node_ids = o
                .get("nodeIds")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default();
            let group_ids = o
                .get("groupIds")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default();
            merges.push(MergeGroup {
                id,
                name,
                node_ids,
                group_ids,
                mode: opt_str(o, "mode", "selector"),
                selected_id: o.get("selectedId").and_then(Value::as_i64).unwrap_or(0),
            });
        }

        let mut settings = IndexMap::new();
        if let Some(s) = obj.get("settings").and_then(Value::as_object) {
            for (k, v) in s {
                let val = match v {
                    Value::String(st) => st.clone(),
                    other => other.to_string(),
                };
                settings.insert(k.clone(), val);
            }
        }

        Ok(Self {
            nodes,
            groups,
            rules,
            merges,
            settings,
        })
    }

    pub fn to_json(&self) -> Result<String, String> {
        let mut root = Map::new();
        root.insert("format".into(), Value::Number(1.into()));

        let mut nodes_val = Vec::with_capacity(self.nodes.len());
        for n in &self.nodes {
            let mut o = Map::new();
            o.insert("id".into(), n.id.into());
            o.insert("groupId".into(), n.group_id.into());
            o.insert("name".into(), n.name.clone().into());
            let out: Value = serde_json::from_str(&n.outbound)
                .map_err(|e| format!("节点 outbound JSON 无效: {e}"))?;
            o.insert("outbound".into(), out);
            o.insert("shareLink".into(), n.share_link.clone().into());
            o.insert("ping".into(), n.ping.into());
            o.insert("tx".into(), n.tx.into());
            o.insert("rx".into(), n.rx.into());
            o.insert("order".into(), n.order.into());
            o.insert("status".into(), n.status.into());
            let meta: Value = serde_json::from_str(&n.metadata)
                .map_err(|e| format!("节点 metadata JSON 无效: {e}"))?;
            o.insert("metadata".into(), meta);
            nodes_val.push(Value::Object(o));
        }
        root.insert("nodes".into(), Value::Array(nodes_val));

        let mut groups_val = Vec::with_capacity(self.groups.len());
        for g in &self.groups {
            let mut o = Map::new();
            o.insert("id".into(), g.id.into());
            o.insert("name".into(), g.name.clone().into());
            o.insert("subscriptionUrl".into(), g.subscription_url.clone().into());
            o.insert("enabled".into(), g.enabled.into());
            o.insert("order".into(), g.order.into());
            o.insert("updatedAt".into(), g.updated_at.into());
            o.insert("userInfo".into(), g.user_info.clone().into());
            o.insert("frontProxy".into(), g.front_proxy.into());
            o.insert("landingProxy".into(), g.landing_proxy.into());
            let opts: Value = serde_json::from_str(&g.options)
                .map_err(|e| format!("分组 options JSON 无效: {e}"))?;
            o.insert("options".into(), opts);
            groups_val.push(Value::Object(o));
        }
        root.insert("groups".into(), Value::Array(groups_val));

        let mut rules_val = Vec::with_capacity(self.rules.len());
        for r in &self.rules {
            let mut o = Map::new();
            o.insert("id".into(), r.id.into());
            o.insert("name".into(), r.name.clone().into());
            o.insert("domains".into(), r.domains.clone().into());
            o.insert("packages".into(), r.packages.clone().into());
            o.insert("ipCidrs".into(), r.ip_cidrs.clone().into());
            o.insert("outbound".into(), r.outbound.clone().into());
            o.insert("enabled".into(), r.enabled.into());
            o.insert("order".into(), r.order.into());
            let adv: Value = serde_json::from_str(&r.advanced)
                .map_err(|e| format!("规则 advanced JSON 无效: {e}"))?;
            o.insert("advanced".into(), adv);
            o.insert("prioritize".into(), r.prioritize.into());
            rules_val.push(Value::Object(o));
        }
        root.insert("rules".into(), Value::Array(rules_val));

        let mut merges_val = Vec::with_capacity(self.merges.len());
        for m in &self.merges {
            let mut o = Map::new();
            o.insert("id".into(), m.id.into());
            o.insert("name".into(), m.name.clone().into());
            o.insert(
                "nodeIds".into(),
                Value::Array(m.node_ids.iter().map(|&i| i.into()).collect()),
            );
            o.insert(
                "groupIds".into(),
                Value::Array(m.group_ids.iter().map(|&i| i.into()).collect()),
            );
            o.insert("mode".into(), m.mode.clone().into());
            o.insert("selectedId".into(), m.selected_id.into());
            merges_val.push(Value::Object(o));
        }
        root.insert("merges".into(), Value::Array(merges_val));

        let mut settings_map = Map::new();
        for (k, v) in &self.settings {
            settings_map.insert(k.clone(), v.clone().into());
        }
        root.insert("settings".into(), Value::Object(settings_map));

        serde_json::to_string(&Value::Object(root)).map_err(|e| e.to_string())
    }

    /// Preserves raw `outbound`/`metadata`/`options`/`advanced` JSON strings byte-for-byte across JNI.
    pub fn to_json_raw(&self) -> Result<String, String> {
        let mut root = Map::new();
        root.insert("format".into(), Value::Number(1.into()));

        let nodes_val: Vec<Value> = self
            .nodes
            .iter()
            .map(|n| {
                let mut o = Map::new();
                o.insert("id".into(), n.id.into());
                o.insert("groupId".into(), n.group_id.into());
                o.insert("name".into(), n.name.clone().into());
                o.insert("outbound".into(), n.outbound.clone().into());
                o.insert("shareLink".into(), n.share_link.clone().into());
                o.insert("ping".into(), n.ping.into());
                o.insert("tx".into(), n.tx.into());
                o.insert("rx".into(), n.rx.into());
                o.insert("order".into(), n.order.into());
                o.insert("status".into(), n.status.into());
                o.insert("metadata".into(), n.metadata.clone().into());
                Value::Object(o)
            })
            .collect();
        root.insert("nodes".into(), Value::Array(nodes_val));

        let groups_val: Vec<Value> = self
            .groups
            .iter()
            .map(|g| {
                let mut o = Map::new();
                o.insert("id".into(), g.id.into());
                o.insert("name".into(), g.name.clone().into());
                o.insert("subscriptionUrl".into(), g.subscription_url.clone().into());
                o.insert("enabled".into(), g.enabled.into());
                o.insert("order".into(), g.order.into());
                o.insert("updatedAt".into(), g.updated_at.into());
                o.insert("userInfo".into(), g.user_info.clone().into());
                o.insert("frontProxy".into(), g.front_proxy.into());
                o.insert("landingProxy".into(), g.landing_proxy.into());
                o.insert("options".into(), g.options.clone().into());
                Value::Object(o)
            })
            .collect();
        root.insert("groups".into(), Value::Array(groups_val));

        let rules_val: Vec<Value> = self
            .rules
            .iter()
            .map(|r| {
                let mut o = Map::new();
                o.insert("id".into(), r.id.into());
                o.insert("name".into(), r.name.clone().into());
                o.insert("domains".into(), r.domains.clone().into());
                o.insert("packages".into(), r.packages.clone().into());
                o.insert("ipCidrs".into(), r.ip_cidrs.clone().into());
                o.insert("outbound".into(), r.outbound.clone().into());
                o.insert("enabled".into(), r.enabled.into());
                o.insert("order".into(), r.order.into());
                o.insert("advanced".into(), r.advanced.clone().into());
                o.insert("prioritize".into(), r.prioritize.into());
                Value::Object(o)
            })
            .collect();
        root.insert("rules".into(), Value::Array(rules_val));

        let merges_val: Vec<Value> = self
            .merges
            .iter()
            .map(|m| {
                let mut o = Map::new();
                o.insert("id".into(), m.id.into());
                o.insert("name".into(), m.name.clone().into());
                o.insert("nodeIds".into(), Value::Array(m.node_ids.iter().map(|&i| i.into()).collect()));
                o.insert("groupIds".into(), Value::Array(m.group_ids.iter().map(|&i| i.into()).collect()));
                o.insert("mode".into(), m.mode.clone().into());
                o.insert("selectedId".into(), m.selected_id.into());
                Value::Object(o)
            })
            .collect();
        root.insert("merges".into(), Value::Array(merges_val));

        let mut settings_map = Map::new();
        for (k, v) in &self.settings {
            settings_map.insert(k.clone(), v.clone().into());
        }
        root.insert("settings".into(), Value::Object(settings_map));

        serde_json::to_string(&Value::Object(root)).map_err(|e| e.to_string())
    }

    pub fn without_stale_node_settings(&self) -> Self {
        let node_ids: HashSet<i64> = self.nodes.iter().map(|n| n.id).collect();
        let active_smart_groups: HashSet<String> = self
            .settings
            .iter()
            .filter_map(|(k, v)| {
                if k.starts_with("smart.") && k.ends_with(".target") {
                    v.strip_prefix("group:").map(|gid| format!("smart-group-{gid}"))
                } else {
                    None
                }
            })
            .collect();
        let active_smart_autos: HashSet<String> = self
            .settings
            .iter()
            .filter_map(|(k, v)| {
                let rest = k.strip_prefix("smart.")?;
                let svc = rest.strip_suffix(".target")?;
                if !svc.is_empty() && v == "auto" {
                    Some(format!("smart-{svc}"))
                } else {
                    None
                }
            })
            .collect();
        let mut selectors: HashSet<String> = HashSet::new();
        for g in &self.groups {
            selectors.insert(format!("group-{}", g.id));
            if g.enabled {
                let smart_tag = format!("smart-group-{}", g.id);
                if active_smart_groups.contains(&smart_tag) {
                    selectors.insert(smart_tag);
                }
            }
        }
        selectors.extend(active_smart_autos);
        for m in &self.merges {
            selectors.insert(format!("merge-{}", m.id));
        }
        let mut stale_selections: HashSet<String> = HashSet::new();
        for (key, value) in &self.settings {
            if let Some(tag) = key.strip_prefix("groupSelection.") {
                if is_selector_tag(tag) {
                    let node_missing = value
                        .strip_prefix("node-")
                        .filter(|s| is_ascii_digits(s))
                        .is_some_and(|s| {
                            !s.parse::<i64>().ok().is_some_and(|id| node_ids.contains(&id))
                        });
                    if !selectors.contains(tag) || node_missing {
                        stale_selections.insert(tag.to_string());
                    }
                }
            }
        }
        let mut next_settings = IndexMap::new();
        for (key, value) in &self.settings {
            let stale_sort = key
                .strip_prefix("sort_ping_")
                .filter(|s| is_ascii_digits(s))
                .is_some_and(|s| !s.parse::<i64>().ok().is_some_and(|id| node_ids.contains(&id)));
            let is_selection = key.starts_with("groupSelection.")
                || key.starts_with("groupSelectionDelay.");
            let tag = key.split_once('.').map(|(_, r)| r).unwrap_or("");
            let stale_sel = is_selection
                && (stale_selections.contains(tag)
                    || (is_selector_tag(tag) && !selectors.contains(tag)));
            if !stale_sort && !stale_sel {
                next_settings.insert(key.clone(), value.clone());
            }
        }
        let mut out = self.clone();
        out.settings = next_settings;
        out
    }
}

fn opt_str(o: &Map<String, Value>, key: &str, default: &str) -> String {
    o.get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_string()
}

fn is_ascii_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_selector_tag(tag: &str) -> bool {
    if let Some(rest) = tag.strip_prefix("group-") {
        is_ascii_digits(rest)
    } else if let Some(rest) = tag.strip_prefix("smart-group-") {
        is_ascii_digits(rest)
    } else if let Some(rest) = tag.strip_prefix("smart-") {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    } else if let Some(rest) = tag.strip_prefix("merge-") {
        is_ascii_digits(rest)
    } else {
        false
    }
}
