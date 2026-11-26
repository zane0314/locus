use crate::json_depth::parse_lenient_json;
use crate::share_uri::{
    decode_base64_utf8, hysteria_hop_interval, percent_decode, socks_user_info, ShareUri,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const LIMIT: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedNode {
    pub name: String,
    pub outbound: String,
    #[serde(rename = "shareLink", default)]
    pub share_link: String,
    #[serde(default = "default_empty_obj")]
    pub metadata: String,
}

fn default_empty_obj() -> String {
    "{}".to_string()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseReport {
    pub nodes: Vec<ParsedNode>,
    pub skipped: usize,
    #[serde(rename = "firstError")]
    pub first_error: Option<String>,
}

struct Skips {
    count: usize,
    first: Option<String>,
}

impl Skips {
    fn new() -> Self {
        Self {
            count: 0,
            first: None,
        }
    }

    fn record(&mut self, err: String) {
        self.count += 1;
        if self.first.is_none() {
            if !err.contains("://") {
                self.first = Some(err.chars().take(128).collect());
            } else {
                self.first = Some("IllegalArgumentException".to_string());
            }
        }
    }
}

pub fn parse_report(text: &str) -> Result<ParseReport, String> {
    let mut skips = Skips::new();
    let nodes = parse_internal(text.trim(), 0, &mut skips)?;
    if nodes.is_empty() {
        return match skips.first {
            Some(ref err) => Err(format!("订阅没有可用节点（跳过 {} 条：{err}）", skips.count)),
            None => Err("订阅没有节点".to_string()),
        };
    }
    if nodes.len() > 10_000 {
        return Err("节点数量超过限制".to_string());
    }
    Ok(ParseReport {
        nodes,
        skipped: skips.count,
        first_error: skips.first,
    })
}

fn parse_internal(text: &str, depth: usize, skips: &mut Skips) -> Result<Vec<ParsedNode>, String> {
    let text = text.trim_start_matches('\u{FEFF}').trim();
    if text.is_empty() {
        return Err("没有可导入的内容".to_string());
    }
    if text.chars().count() > LIMIT || depth > 2 {
        return Err("订阅超过解析限制".to_string());
    }

    if text.starts_with('{') || text.starts_with('[') {
        let parsed: Value = parse_lenient_json(text)?;
        let items: Vec<Value> = match parsed {
            Value::Array(arr) => arr,
            Value::Object(root) => {
                let outs = root.get("outbounds").and_then(Value::as_array);
                let eps = root.get("endpoints").and_then(Value::as_array);
                if outs.is_some() || eps.is_some() {
                    let mut combined = Vec::new();
                    if let Some(a) = outs {
                        combined.extend(a.iter().cloned());
                    }
                    if let Some(a) = eps {
                        combined.extend(a.iter().cloned());
                    }
                    combined
                } else if let Some(p) = root.get("proxies").and_then(Value::as_array) {
                    p.clone()
                } else if let Some(s) = root.get("servers").and_then(Value::as_array) {
                    s.clone()
                } else {
                    vec![Value::Object(root)]
                }
            }
            _ => return Err("JSON 根格式无效".to_string()),
        };
        let mut nodes = Vec::new();
        for (index, item) in items.into_iter().enumerate() {
            match parse_json_entry(item, index) {
                Ok(Some(n)) => nodes.push(n),
                Ok(None) => {}
                Err(e) => skips.record(e),
            }
        }
        return Ok(nodes);
    }

    let first_line = text.lines().next().unwrap_or("");
    if text.contains("proxies:") && !first_line.contains("://") {
        let root: serde_yaml::Value =
            serde_yaml::from_str(text).map_err(|_| "Clash YAML 根格式无效".to_string())?;
        let mapping = root
            .as_mapping()
            .ok_or_else(|| "Clash YAML 根格式无效".to_string())?;
        let proxies = mapping
            .get(serde_yaml::Value::String("proxies".to_string()))
            .and_then(serde_yaml::Value::as_sequence)
            .ok_or_else(|| "Clash YAML 缺少 proxies".to_string())?;
        let mut nodes = Vec::new();
        for item in proxies {
            match yaml_to_json(item) {
                Ok(Value::Object(map)) => match clash_map(&map) {
                    Ok(n) => nodes.push(n),
                    Err(e) => skips.record(e),
                },
                Ok(_) => skips.record("Clash 节点格式无效".to_string()),
                Err(e) => skips.record(e),
            }
        }
        return Ok(nodes);
    }

    if !text.contains("://") {
        let decoded = decode_base64_utf8(text)?;
        return parse_internal(&decoded, depth + 1, skips);
    }

    let mut nodes = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match parse_link(line) {
            Ok(n) => nodes.push(n),
            Err(e) => skips.record(e),
        }
    }
    Ok(nodes)
}

fn parse_json_entry(item: Value, index: usize) -> Result<Option<ParsedNode>, String> {
    let mut obj = match item {
        Value::Object(m) => m,
        _ => return Err("节点格式无效".to_string()),
    };
    if !obj.contains_key("type") && obj.contains_key("method") && obj.contains_key("server") {
        obj.insert("type".into(), Value::String("shadowsocks".into()));
    }
    let kind = obj
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if matches!(
        kind.as_str(),
        "direct" | "block" | "dns" | "selector" | "urltest"
    ) {
        return Ok(None);
    }
    if obj.contains_key("server-port")
        || (obj.contains_key("port") && !obj.contains_key("server_port"))
    {
        return clash_map(&obj).map(Some);
    }
    let name = opt_nonempty_str(&obj, "display_name")
        .or_else(|| opt_nonempty_str(&obj, "name"))
        .or_else(|| opt_nonempty_str(&obj, "remarks"))
        .or_else(|| opt_nonempty_str(&obj, "tag"))
        .unwrap_or_else(|| format!("节点 {}", index + 1));
    canonical(obj, &name, "").map(Some)
}

pub fn apply_hysteria_ports(out: &mut Map<String, Value>, text: &str) -> Result<(), String> {
    if text.len() > 65536 {
        return Err("跳跃端口列表过长".to_string());
    }
    let mut ranges: Vec<(i32, i32)> = Vec::new();
    for raw in text.split(',') {
        let normalized = raw.trim().replace('-', ":");
        let parts: Vec<&str> = normalized.split(':').collect();
        if !(1..=2).contains(&parts.len()) {
            return Err("跳跃端口无效".to_string());
        }
        let mut nums = Vec::with_capacity(parts.len());
        for p in &parts {
            if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
                return Err("跳跃端口无效".to_string());
            }
            let n: i32 = p.parse().map_err(|_| "跳跃端口无效".to_string())?;
            if !(1..=65535).contains(&n) {
                return Err("跳跃端口无效".to_string());
            }
            nums.push(n);
        }
        let start = nums[0];
        let end = *nums.get(1).unwrap_or(&start);
        if start > end {
            return Err("跳跃端口范围倒序".to_string());
        }
        ranges.push((start, end));
    }
    let total: i64 = ranges
        .iter()
        .map(|(s, e)| (*e as i64) - (*s as i64) + 1)
        .sum();
    if total > 65535 {
        return Err("跳跃端口总数超限".to_string());
    }
    if ranges.len() == 1 && ranges[0].0 == ranges[0].1 {
        out.insert("server_port".into(), Value::Number(ranges[0].0.into()));
        out.shift_remove("server_ports");
    } else {
        out.shift_remove("server_port");
        let arr: Vec<Value> = ranges
            .into_iter()
            .map(|(s, e)| Value::String(format!("{s}:{e}")))
            .collect();
        out.insert("server_ports".into(), Value::Array(arr));
    }
    Ok(())
}

fn canonical(
    mut obj: Map<String, Value>,
    name: &str,
    link: &str,
) -> Result<ParsedNode, String> {
    let kind = obj
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if matches!(kind.as_str(), "hysteria" | "hysteria2") {
        if let Some(hop) = obj.get("hop_interval") {
            let raw = value_as_string(hop);
            let normalized = hysteria_hop_interval(&raw)?;
            obj.insert("hop_interval".into(), Value::String(normalized));
        }
    }
    if kind.trim().is_empty() {
        return Err("节点缺少协议".to_string());
    }
    let mut metadata = Map::new();
    if let Some(tag) = obj.get("tag").and_then(Value::as_str) {
        metadata.insert("sourceTag".into(), Value::String(tag.to_string()));
    }
    obj.shift_remove("name");
    obj.shift_remove("display_name");
    obj.shift_remove("tag");
    if kind == "shadowsocks" {
        obj.shift_remove("id");
        obj.shift_remove("remarks");
    }
    if !obj.contains_key("server") && !matches!(kind.as_str(), "wireguard" | "tailscale") {
        return Err("节点缺少服务器".to_string());
    }
    let final_name = if name.trim().is_empty() {
        obj.get("server")
            .and_then(Value::as_str)
            .unwrap_or("节点")
            .to_string()
    } else {
        name.to_string()
    };
    Ok(ParsedNode {
        name: final_name,
        outbound: Value::Object(obj).to_string(),
        share_link: link.to_string(),
        metadata: Value::Object(metadata).to_string(),
    })
}

fn checked_json_obj(text: &str) -> Result<Map<String, Value>, String> {
    if text.len() > LIMIT {
        return Err("订阅超过解析限制".to_string());
    }
    let val: Value = parse_lenient_json(text)?;
    match val {
        Value::Object(m) => Ok(m),
        _ => Err("JSON 必须是对象".to_string()),
    }
}

fn apply_utls(tls: &mut Map<String, Value>, fingerprint: Option<&str>) {
    let fp = fingerprint
        .filter(|s| !s.trim().is_empty())
        .or_else(|| if tls.contains_key("reality") { Some("chrome") } else { None });
    if let Some(value) = fp {
        let mut utls = Map::new();
        utls.insert("enabled".into(), Value::Bool(true));
        utls.insert("fingerprint".into(), Value::String(value.to_string()));
        tls.insert("utls".into(), Value::Object(utls));
    }
}

fn parse_link(raw: &str) -> Result<ParsedNode, String> {
    let scheme = raw
        .split_once("://")
        .map(|(s, _)| s.to_ascii_lowercase())
        .ok_or_else(|| "链接缺少协议".to_string())?;
    if scheme == "vmess" {
        let payload = raw
            .split_once("://")
            .map(|(_, r)| r)
            .unwrap_or("")
            .split_once('#')
            .map(|(a, _)| a)
            .unwrap_or_else(|| raw.split_once("://").map(|(_, r)| r).unwrap_or(""));
        let decoded = decode_base64_utf8(payload)?;
        let v = checked_json_obj(&decoded)?;
        let server = v
            .get("add")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "VMess 缺少服务器".to_string())?
            .to_string();
        let port = v
            .get("port")
            .map(value_as_string)
            .and_then(|s| s.trim().parse::<i32>().ok())
            .filter(|p| (1..=65535).contains(p))
            .ok_or_else(|| "VMess 端口无效".to_string())?;
        let uuid = v
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "VMess 缺少 UUID".to_string())?
            .to_string();
        let scy = v
            .get("scy")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("auto")
            .to_string();
        let aid = v
            .get("aid")
            .map(value_as_string)
            .and_then(|s| s.trim().parse::<i32>().ok())
            .unwrap_or(0);

        let mut o = Map::new();
        o.insert("type".into(), "vmess".into());
        o.insert("server".into(), server.clone().into());
        o.insert("server_port".into(), port.into());
        o.insert("uuid".into(), uuid.into());
        o.insert("security".into(), scy.into());
        o.insert("alter_id".into(), aid.into());

        if opt_str(&v, "tls") == "tls" {
            let sni_raw = opt_str(&v, "sni");
            let host_first = opt_str(&v, "host")
                .split(',')
                .next()
                .unwrap_or("")
                .to_string();
            let sni = if !sni_raw.trim().is_empty() {
                sni_raw
            } else if !host_first.trim().is_empty() {
                host_first
            } else {
                server.clone()
            };
            let mut tls = Map::new();
            tls.insert("enabled".into(), true.into());
            tls.insert("server_name".into(), sni.into());
            let allow_insec = opt_str(&v, "allowInsecure");
            if matches!(allow_insec.as_str(), "1" | "true") {
                tls.insert("insecure".into(), true.into());
            }
            let alpn = opt_str(&v, "alpn");
            if !alpn.trim().is_empty() {
                tls.insert(
                    "alpn".into(),
                    Value::Array(alpn.split(',').map(|s| Value::String(s.to_string())).collect()),
                );
            }
            let fp = opt_str(&v, "fp");
            apply_utls(&mut tls, Some(&fp));
            o.insert("tls".into(), Value::Object(tls));
        }

        let mut q = IndexMap::new();
        let net = opt_str(&v, "net");
        q.insert(
            "type".to_string(),
            if net.is_empty() { "tcp".to_string() } else { net },
        );
        q.insert("headerType".to_string(), opt_str(&v, "type"));
        q.insert("host".to_string(), opt_str(&v, "host"));
        q.insert("path".to_string(), opt_str(&v, "path"));
        apply_transport(&mut o, &q)?;
        let ps = v
            .get("ps")
            .and_then(Value::as_str)
            .unwrap_or("VMess")
            .to_string();
        return canonical(o, &ps, raw);
    }

    if scheme == "ss" {
        return parse_shadowsocks(raw);
    }

    let uri = ShareUri::parse(raw)?;
    let q = uri.query();
    let kind = match scheme.as_str() {
        "hy2" | "hysteria2" => "hysteria2",
        "socks" | "socks4" | "socks4a" | "socks5" => "socks",
        "https" => "http",
        other => other,
    };
    if !matches!(
        kind,
        "vless"
            | "trojan"
            | "hysteria"
            | "hysteria2"
            | "tuic"
            | "socks"
            | "http"
            | "ssh"
            | "anytls"
            | "shadowtls"
            | "juicity"
            | "snell"
    ) {
        return Err(format!("暂不支持分享协议 {scheme}；可导入完整 sing-box JSON"));
    }

    let default_port = match kind {
        "socks" => 1080,
        "ssh" => 22,
        "http" => {
            if scheme == "https" {
                443
            } else {
                80
            }
        }
        _ => 443,
    };
    let port = if uri.port > 0 { uri.port } else { default_port };

    let mut o = Map::new();
    o.insert("type".into(), kind.into());
    o.insert("server".into(), uri.host.clone().into());
    o.insert("server_port".into(), port.into());

    let user = if matches!(scheme.as_str(), "socks" | "socks5") {
        socks_user_info(uri.raw_user_info.as_deref())
    } else {
        percent_decode(uri.raw_user_info.as_deref().unwrap_or(""))
    };
    let parts: Vec<&str> = user.splitn(2, ':').collect();

    match kind {
        "vless" => {
            if user.trim().is_empty() {
                return Err("VLESS 缺少 UUID".to_string());
            }
            o.insert("uuid".into(), user.clone().into());
            if let Some(flow) = q.get("flow").filter(|s| !s.trim().is_empty()) {
                o.insert("flow".into(), flow.clone().into());
            }
        }
        "shadowtls" => {
            let ver: i32 = q
                .get("version")
                .map(String::as_str)
                .unwrap_or("3")
                .parse()
                .map_err(|_| "ShadowTLS 版本无效".to_string())?;
            o.insert("password".into(), user.clone().into());
            o.insert("version".into(), ver.into());
        }
        "snell" => {
            let ver: i32 = q
                .get("version")
                .map(String::as_str)
                .unwrap_or("4")
                .parse()
                .map_err(|_| "Snell 版本无效".to_string())?;
            o.insert("psk".into(), user.clone().into());
            o.insert("version".into(), ver.into());
        }
        "trojan" | "anytls" => {
            if user.trim().is_empty() {
                return Err("缺少密码".to_string());
            }
            o.insert("password".into(), user.clone().into());
        }
        "hysteria2" => {
            o.insert("password".into(), user.clone().into());
        }
        "hysteria" => {
            let auth_str = q.get("auth").cloned().unwrap_or_else(|| user.clone());
            let up: i32 = q
                .get("upmbps")
                .map(String::as_str)
                .unwrap_or("100")
                .parse()
                .map_err(|_| "Hysteria upmbps 无效".to_string())?;
            let down: i32 = q
                .get("downmbps")
                .map(String::as_str)
                .unwrap_or("100")
                .parse()
                .map_err(|_| "Hysteria downmbps 无效".to_string())?;
            o.insert("auth_str".into(), auth_str.into());
            o.insert("up_mbps".into(), up.into());
            o.insert("down_mbps".into(), down.into());
            if let Some(proto) = q.get("protocol") {
                if proto != "udp" {
                    return Err(format!("当前内核不支持 Hysteria {proto} 伪装"));
                }
            }
            let obfs = q.get("obfs").map(String::as_str).unwrap_or("");
            let password = q.get("obfsParam").or_else(|| q.get("obfs-password"));
            if let Some(p) = password.filter(|s| !s.trim().is_empty()) {
                o.insert("obfs".into(), p.clone().into());
            } else if obfs.trim().is_empty() || obfs == "none" {
                // no obfs
            } else if obfs == "xplus" {
                return Err("Hysteria 混淆缺少密码".to_string());
            } else {
                o.insert("obfs".into(), obfs.into());
            }
        }
        "tuic" | "juicity" => {
            if parts.len() != 2 {
                return Err("TUIC 缺少密码".to_string());
            }
            o.insert("uuid".into(), parts[0].into());
            o.insert("password".into(), parts[1].into());
            if kind == "tuic" {
                o.insert(
                    "congestion_control".into(),
                    q.get("congestion_control")
                        .cloned()
                        .unwrap_or_else(|| "cubic".to_string())
                        .into(),
                );
                if let Some(m) = q.get("udp_relay_mode") {
                    o.insert("udp_relay_mode".into(), m.clone().into());
                }
            } else if let Some(pin) = q.get("pinned_certchain_sha256") {
                o.insert("pin_cert_sha256".into(), pin.clone().into());
            }
        }
        _ => {
            if !parts[0].is_empty() {
                o.insert(
                    if kind == "ssh" { "user" } else { "username" }.into(),
                    parts[0].into(),
                );
            }
            if parts.len() > 1 {
                o.insert("password".into(), parts[1].into());
            }
            if kind == "socks" {
                let ver = match scheme.as_str() {
                    "socks4" => "4",
                    "socks4a" => "4a",
                    _ => "5",
                };
                o.insert("version".into(), ver.into());
            }
        }
    }

    let security = q.get("security").map(String::as_str).unwrap_or_else(|| {
        if matches!(
            kind,
            "trojan" | "anytls" | "hysteria" | "hysteria2" | "tuic" | "shadowtls" | "juicity"
        ) || scheme == "https"
        {
            "tls"
        } else {
            "none"
        }
    });

    if matches!(security, "tls" | "reality" | "xtls") {
        let sni = q
            .get("sni")
            .filter(|s| !s.trim().is_empty())
            .or_else(|| q.get("peer").filter(|s| !s.trim().is_empty()))
            .cloned()
            .unwrap_or_else(|| uri.host.clone());
        let insecure = matches!(
            q.get("allowInsecure").map(String::as_str),
            Some("1" | "true")
        ) || matches!(q.get("insecure").map(String::as_str), Some("1" | "true"));
        let mut tls = Map::new();
        tls.insert("enabled".into(), true.into());
        tls.insert("server_name".into(), sni.into());
        tls.insert("insecure".into(), insecure.into());
        if let Some(alpn) = q.get("alpn").filter(|s| !s.trim().is_empty()) {
            tls.insert(
                "alpn".into(),
                Value::Array(alpn.split(',').map(|s| Value::String(s.to_string())).collect()),
            );
        }
        if security == "reality" {
            let pbk = q
                .get("pbk")
                .ok_or_else(|| "Reality 缺少公钥".to_string())?
                .clone();
            let sid = q.get("sid").cloned().unwrap_or_default();
            let mut reality = Map::new();
            reality.insert("enabled".into(), true.into());
            reality.insert("public_key".into(), pbk.into());
            reality.insert("short_id".into(), sid.into());
            tls.insert("reality".into(), Value::Object(reality));
        }
        apply_utls(&mut tls, q.get("fp").map(String::as_str));
        o.insert("tls".into(), Value::Object(tls));
    }

    if kind == "hysteria2" {
        if let Some(obfs_type) = q
            .get("obfs")
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty() && s != "none")
        {
            if !matches!(obfs_type.as_str(), "salamander" | "gecko") {
                return Err(format!("不支持的 Hysteria2 混淆类型：{obfs_type}"));
            }
            let pwd = q
                .get("obfs-password")
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "混淆缺少密码".to_string())?
                .clone();
            let mut obfs = Map::new();
            obfs.insert("type".into(), obfs_type.clone().into());
            obfs.insert("password".into(), pwd.into());
            if obfs_type == "gecko" {
                let min_raw = q
                    .get("obfs-min-packet-size")
                    .or_else(|| q.get("min_packet_size"))
                    .map(String::as_str);
                let max_raw = q
                    .get("obfs-max-packet-size")
                    .or_else(|| q.get("max_packet_size"))
                    .map(String::as_str);
                apply_hysteria2_gecko_packet_sizes(&mut obfs, min_raw, max_raw)?;
            }
            o.insert("obfs".into(), Value::Object(obfs));
        }
    }

    if matches!(kind, "hysteria" | "hysteria2") {
        if let Some(ports) = q
            .get("mport")
            .or_else(|| q.get("ports"))
            .or_else(|| q.get("server_ports"))
            .filter(|s| !s.trim().is_empty())
        {
            apply_hysteria_ports(&mut o, ports)?;
        }
        if let Some(hop) = q.get("hop_interval") {
            o.insert("hop_interval".into(), hysteria_hop_interval(hop)?.into());
        }
    }

    apply_transport(&mut o, &q)?;
    let frag = percent_decode(uri.raw_fragment.as_deref().unwrap_or(""));
    let name = if frag.trim().is_empty() {
        format!("{kind} {}", uri.host)
    } else {
        frag
    };
    canonical(o, &name, raw)
}

fn split_hosts(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn http_transport(path: Option<&str>, hosts: Vec<String>, method: Option<&str>) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), "http".into());
    if let Some(p) = path.filter(|s| !s.trim().is_empty()) {
        m.insert("path".into(), p.into());
    }
    if !hosts.is_empty() {
        m.insert(
            "host".into(),
            Value::Array(hosts.into_iter().map(Value::String).collect()),
        );
    }
    if let Some(meth) = method.filter(|s| !s.trim().is_empty()) {
        m.insert("method".into(), meth.into());
    }
    Value::Object(m)
}

fn websocket_transport(
    raw_path: Option<&str>,
    host: Option<&str>,
    early_data: Option<i32>,
    early_header: Option<&str>,
) -> Value {
    let mut path = raw_path
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("/")
        .to_string();
    let mut ed = early_data;
    if let Some((before, after)) = path.split_once('?') {
        let params: Vec<&str> = after.split('&').filter(|s| !s.is_empty()).collect();
        if let Some(val) = params
            .iter()
            .find(|p| p.starts_with("ed="))
            .and_then(|p| p.strip_prefix("ed="))
            .and_then(|v| v.parse::<i32>().ok())
        {
            ed = Some(val);
            let kept: Vec<&str> = params
                .into_iter()
                .filter(|p| !p.starts_with("ed="))
                .collect();
            path = if kept.is_empty() {
                before.to_string()
            } else {
                format!("{before}?{}", kept.join("&"))
            };
        }
    }
    let mut tr = Map::new();
    tr.insert("type".into(), "ws".into());
    tr.insert(
        "path".into(),
        if path.trim().is_empty() { "/" } else { &path }.into(),
    );
    if let Some(h) = host.filter(|s| !s.is_empty()) {
        let mut headers = Map::new();
        headers.insert("Host".into(), h.into());
        tr.insert("headers".into(), Value::Object(headers));
    }
    if let Some(ed_val) = ed.filter(|&v| v > 0) {
        tr.insert("max_early_data".into(), ed_val.into());
        let hdr = early_header
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("Sec-WebSocket-Protocol");
        tr.insert("early_data_header_name".into(), hdr.into());
    }
    Value::Object(tr)
}

fn apply_transport(o: &mut Map<String, Value>, q: &IndexMap<String, String>) -> Result<(), String> {
    let t = q.get("type").map(String::as_str).unwrap_or("tcp");
    match t {
        "tcp" | "none" | "" => {
            if q.get("headerType").map(String::as_str) == Some("http") {
                o.insert(
                    "transport".into(),
                    http_transport(
                        q.get("path").map(String::as_str),
                        split_hosts(q.get("host").map(String::as_str)),
                        None,
                    ),
                );
            }
        }
        "ws" => {
            let ed = q.get("ed").and_then(|s| s.parse::<i32>().ok());
            o.insert(
                "transport".into(),
                websocket_transport(
                    q.get("path").map(String::as_str),
                    q.get("host").map(String::as_str),
                    ed,
                    q.get("eh").map(String::as_str),
                ),
            );
        }
        "httpupgrade" => {
            let mut tr = Map::new();
            tr.insert("type".into(), t.into());
            tr.insert(
                "path".into(),
                q.get("path")
                    .map(String::as_str)
                    .unwrap_or("/")
                    .into(),
            );
            if let Some(h) = q.get("host").filter(|s| !s.is_empty()) {
                tr.insert("host".into(), h.clone().into());
            }
            o.insert("transport".into(), Value::Object(tr));
        }
        "http" | "h2" => {
            o.insert(
                "transport".into(),
                http_transport(
                    Some(q.get("path").map(String::as_str).unwrap_or("/")),
                    split_hosts(q.get("host").map(String::as_str)),
                    None,
                ),
            );
        }
        "grpc" => {
            let mut tr = Map::new();
            tr.insert("type".into(), "grpc".into());
            let svc = q
                .get("serviceName")
                .or_else(|| q.get("path"))
                .map(String::as_str)
                .unwrap_or("");
            tr.insert("service_name".into(), svc.into());
            o.insert("transport".into(), Value::Object(tr));
        }
        "xhttp" | "splithttp" => {
            let extra = match q.get("extra") {
                Some(raw) => checked_json_obj(raw)?,
                None => Map::new(),
            };
            o.insert("transport".into(), Value::Object(xhttp_transport(extra, q)?));
        }
        "quic" => {
            let mut tr = Map::new();
            tr.insert("type".into(), "quic".into());
            o.insert("transport".into(), Value::Object(tr));
        }
        other => {
            return Err(format!("暂不支持传输 {other}；可导入完整 sing-box JSON"));
        }
    }
    Ok(())
}

fn parse_shadowsocks(raw: &str) -> Result<ParsedNode, String> {
    let after = raw
        .split_once("://")
        .map(|(_, r)| r)
        .ok_or_else(|| "SS 链接无效".to_string())?;
    let (payload, fragment) = match after.split_once('#') {
        Some((p, f)) => (p, f),
        None => (after, ""),
    };
    let (before_q, after_q) = match payload.split_once('?') {
        Some((a, b)) => (a, Some(b)),
        None => (payload, None),
    };
    let expanded = if !before_q.contains('@') {
        let dec = decode_base64_utf8(before_q)?;
        match after_q {
            Some(q) => format!("{dec}?{q}"),
            None => dec,
        }
    } else {
        payload.to_string()
    };
    let uri = ShareUri::parse(&format!("ss://{expanded}"))?;
    let credentials = uri
        .raw_user_info
        .as_deref()
        .ok_or_else(|| "SS 缺少认证".to_string())?;
    let user = if credentials.contains(':') {
        percent_decode(credentials)
    } else {
        decode_base64_utf8(&percent_decode(credentials))?
    };
    let pieces: Vec<&str> = user.splitn(2, ':').collect();
    if pieces.len() != 2 {
        return Err("SS 认证无效".to_string());
    }
    if uri.port <= 0 {
        return Err("SS 端口无效".to_string());
    }
    let mut o = Map::new();
    o.insert("type".into(), "shadowsocks".into());
    o.insert("server".into(), uri.host.clone().into());
    o.insert("server_port".into(), uri.port.into());
    o.insert("method".into(), pieces[0].into());
    o.insert("password".into(), pieces[1].into());
    let q = uri.query();
    if let Some(plugin) = q.get("plugin") {
        let mut p = plugin.splitn(2, ';');
        if let Some(name) = p.next() {
            o.insert("plugin".into(), name.into());
        }
        if let Some(opts) = p.next() {
            o.insert("plugin_opts".into(), opts.into());
        }
    }
    let frag = percent_decode(fragment);
    let name = if frag.trim().is_empty() {
        format!("SS {}", uri.host)
    } else {
        frag
    };
    canonical(o, &name, raw)
}

fn check_allowed_keys(obj: &Map<String, Value>, allowed: &[&str], scope: &str) -> Result<(), String> {
    for k in obj.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(format!("XHTTP {scope} 暂不支持字段 {k}"));
        }
    }
    Ok(())
}

fn check_xhttp_text(obj: &Map<String, Value>, key: &str, fallback: &str) -> Result<String, String> {
    match obj.get(key) {
        None => Ok(fallback.to_string()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(format!("XHTTP downloadSettings.{key} 必须是字符串")),
    }
}

fn is_valid_xhttp_range(val: &Value) -> bool {
    match val {
        Value::String(s) => {
            let parts: Vec<&str> = s.split('-').collect();
            if !(1..=2).contains(&parts.len()) {
                return false;
            }
            match (parts[0].parse::<i32>(), parts.last().unwrap().parse::<i32>()) {
                (Ok(a), Ok(b)) => a <= b,
                _ => false,
            }
        }
        Value::Object(m) => {
            if m.keys().any(|k| k != "from" && k != "to") {
                return false;
            }
            let from = match m.get("from") {
                None => Some(0),
                Some(Value::Number(n)) => n.as_i64().and_then(|v| i32::try_from(v).ok()),
                _ => None,
            };
            let to = match m.get("to") {
                None => Some(0),
                Some(Value::Number(n)) => n.as_i64().and_then(|v| i32::try_from(v).ok()),
                _ => None,
            };
            matches!((from, to), (Some(a), Some(b)) if a <= b)
        }
        _ => false,
    }
}

fn check_xhttp_values(obj: &Map<String, Value>, ranges: &[&str]) -> Result<(), String> {
    for (k, v) in obj {
        let valid = if ranges.contains(&k.as_str()) {
            is_valid_xhttp_range(v)
        } else {
            match k.as_str() {
                "headers" => v
                    .as_object()
                    .is_some_and(|h| h.values().all(Value::is_string)),
                "xmux" => v.is_object(),
                "no_grpc_header" | "no_sse_header" => v.is_boolean(),
                "sc_max_buffered_posts" | "h_keep_alive_period" => {
                    v.as_i64().is_some()
                }
                _ => v.is_string(),
            }
        };
        if !valid {
            return Err(format!("XHTTP downloadSettings.{k} 格式无效"));
        }
    }
    Ok(())
}

fn xhttp_download(source: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    check_allowed_keys(
        source,
        &[
            "address",
            "port",
            "network",
            "security",
            "tlsSettings",
            "realitySettings",
            "xhttpSettings",
        ],
        "downloadSettings",
    )?;
    let net = check_xhttp_text(source, "network", "")?;
    if !matches!(net.as_str(), "xhttp" | "splithttp") {
        return Err("XHTTP downloadSettings.network 必须为 xhttp".to_string());
    }
    let address = check_xhttp_text(source, "address", "")?;
    if address.trim().is_empty() {
        return Err("XHTTP downloadSettings.address 不能为空".to_string());
    }
    let port = source
        .get("port")
        .map(value_as_string)
        .and_then(|s| s.parse::<i32>().ok());
    let Some(port) = port.filter(|p| (1..=65535).contains(p)) else {
        return Err("XHTTP downloadSettings.port 无效".to_string());
    };
    let settings = match source.get("xhttpSettings") {
        Some(Value::Object(m)) => m.clone(),
        None => Map::new(),
        Some(_) => return Err("XHTTP downloadSettings.xhttpSettings 必须是对象".to_string()),
    };
    let mut extra = match settings.get("extra") {
        Some(Value::Object(m)) => m.clone(),
        None => Map::new(),
        Some(_) => return Err("XHTTP downloadSettings.extra 必须是对象".to_string()),
    };
    let mut fields = IndexMap::new();
    for (k, v) in &settings {
        if k == "extra" {
            continue;
        }
        if matches!(k.as_str(), "host" | "path" | "mode") {
            fields.insert(k.clone(), check_xhttp_text(&settings, k, "")?);
        } else if !extra.contains_key(k) {
            extra.insert(k.clone(), v.clone());
        }
    }
    if extra.contains_key("downloadSettings") || extra.contains_key("download") {
        return Err("XHTTP downloadSettings 不允许嵌套下行".to_string());
    }
    if extra.contains_key("type") {
        return Err("XHTTP downloadSettings.xhttpSettings 暂不支持字段 type".to_string());
    }
    for k in ["host", "path", "mode"] {
        if !fields.contains_key(k) && extra.contains_key(k) {
            check_xhttp_text(&extra, k, "")?;
        }
    }
    let mut down = xhttp_transport(extra, &fields)?;
    down.shift_remove("type");
    check_allowed_keys(
        &down,
        &[
            "host",
            "path",
            "mode",
            "headers",
            "domain_strategy",
            "x_padding_bytes",
            "no_grpc_header",
            "no_sse_header",
            "sc_max_each_post_bytes",
            "sc_min_posts_interval_ms",
            "sc_max_buffered_posts",
            "sc_stream_up_server_secs",
            "xmux",
        ],
        "downloadSettings.xhttpSettings",
    )?;
    check_xhttp_values(
        &down,
        &[
            "x_padding_bytes",
            "sc_max_each_post_bytes",
            "sc_min_posts_interval_ms",
            "sc_stream_up_server_secs",
        ],
    )?;
    let mode = down
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !matches!(mode, "" | "auto" | "packet-up" | "stream-up" | "stream-one") {
        return Err("XHTTP downloadSettings.mode 无效".to_string());
    }
    if let Some(Value::Object(xmux)) = down.get("xmux") {
        check_allowed_keys(
            xmux,
            &[
                "max_concurrency",
                "max_connections",
                "c_max_reuse_times",
                "h_max_request_times",
                "h_max_reusable_secs",
                "h_keep_alive_period",
            ],
            "downloadSettings.xmux",
        )?;
        check_xhttp_values(
            xmux,
            &[
                "max_concurrency",
                "max_connections",
                "c_max_reuse_times",
                "h_max_request_times",
                "h_max_reusable_secs",
            ],
        )?;
    }
    down.insert("server".into(), address.clone().into());
    down.insert("server_port".into(), port.into());
    let security = check_xhttp_text(source, "security", "none")?;
    if !matches!(security.as_str(), "" | "none" | "tls" | "reality") {
        return Err("XHTTP downloadSettings.security 暂不支持".to_string());
    }
    for unused in ["tls", "reality"] {
        if unused != security && source.contains_key(&format!("{unused}Settings")) {
            return Err(format!("XHTTP downloadSettings.{unused}Settings 与 security 不符"));
        }
    }
    if matches!(security.as_str(), "tls" | "reality") {
        let key = format!("{security}Settings");
        let tls_settings = match source.get(&key) {
            Some(Value::Object(m)) => m.clone(),
            None => Map::new(),
            Some(_) => return Err(format!("XHTTP downloadSettings.{key} 必须是对象")),
        };
        let allowed_tls: &[&str] = if security == "tls" {
            &["serverName", "allowInsecure", "alpn", "fingerprint"]
        } else {
            &["serverName", "fingerprint", "publicKey", "shortId"]
        };
        check_allowed_keys(&tls_settings, allowed_tls, &format!("downloadSettings.{key}"))?;
        let sni = check_xhttp_text(&tls_settings, "serverName", "")?;
        let mut tls = Map::new();
        tls.insert("enabled".into(), true.into());
        tls.insert(
            "server_name".into(),
            if sni.trim().is_empty() {
                address.clone()
            } else {
                sni
            }
            .into(),
        );
        if let Some(insec) = tls_settings.get("allowInsecure") {
            let Some(b) = insec.as_bool() else {
                return Err("XHTTP downloadSettings.allowInsecure 必须是布尔值".to_string());
            };
            tls.insert("insecure".into(), b.into());
        }
        if let Some(alpn_val) = tls_settings.get("alpn") {
            let Some(arr) = alpn_val.as_array() else {
                return Err("XHTTP downloadSettings.alpn 必须是字符串列表".to_string());
            };
            if !arr.iter().all(Value::is_string) {
                return Err("XHTTP downloadSettings.alpn 必须是字符串列表".to_string());
            }
            tls.insert("alpn".into(), Value::Array(arr.clone()));
        }
        if security == "reality" {
            let pk = check_xhttp_text(&tls_settings, "publicKey", "")?;
            if pk.trim().is_empty() {
                return Err("XHTTP downloadSettings.publicKey 不能为空".to_string());
            }
            let sid = check_xhttp_text(&tls_settings, "shortId", "")?;
            let mut reality = Map::new();
            reality.insert("enabled".into(), true.into());
            reality.insert("public_key".into(), pk.into());
            reality.insert("short_id".into(), sid.into());
            tls.insert("reality".into(), Value::Object(reality));
        }
        let fp = check_xhttp_text(&tls_settings, "fingerprint", "")?;
        apply_utls(&mut tls, Some(&fp));
        down.insert("tls".into(), Value::Object(tls));
    }
    Ok(down)
}

fn normalize_xhttp_aliases(obj: &mut Map<String, Value>, keys: &[(&str, &str)], ranges: &[&str]) {
    for &(from, to) in keys {
        let dashed = to.replace('_', "-");
        for alias in [from, dashed.as_str()] {
            if let Some(val) = obj.shift_remove(alias) {
                if !obj.contains_key(to) {
                    obj.insert(to.to_string(), val);
                }
            }
        }
    }
    for &key in ranges {
        if let Some(val) = obj.get(key) {
            if val.is_number() {
                let s = value_as_string(val);
                obj.insert(key.to_string(), Value::String(s));
            }
        }
    }
}

fn xhttp_transport(
    mut extra: Map<String, Value>,
    fields: &IndexMap<String, String>,
) -> Result<Map<String, Value>, String> {
    normalize_xhttp_aliases(
        &mut extra,
        &[
            ("xPaddingBytes", "x_padding_bytes"),
            ("noGRPCHeader", "no_grpc_header"),
            ("noSSEHeader", "no_sse_header"),
            ("scMaxEachPostBytes", "sc_max_each_post_bytes"),
            ("scMinPostsIntervalMs", "sc_min_posts_interval_ms"),
            ("scMaxBufferedPosts", "sc_max_buffered_posts"),
            ("scStreamUpServerSecs", "sc_stream_up_server_secs"),
        ],
        &[
            "x_padding_bytes",
            "sc_max_each_post_bytes",
            "sc_min_posts_interval_ms",
            "sc_stream_up_server_secs",
        ],
    );
    if let Some(Value::Object(xmux)) = extra.get_mut("xmux") {
        normalize_xhttp_aliases(
            xmux,
            &[
                ("maxConcurrency", "max_concurrency"),
                ("maxConnections", "max_connections"),
                ("cMaxReuseTimes", "c_max_reuse_times"),
                ("hMaxRequestTimes", "h_max_request_times"),
                ("hMaxReusableSecs", "h_max_reusable_secs"),
                ("hKeepAlivePeriod", "h_keep_alive_period"),
            ],
            &[
                "max_concurrency",
                "max_connections",
                "c_max_reuse_times",
                "h_max_request_times",
                "h_max_reusable_secs",
            ],
        );
    }
    extra.insert("type".into(), "xhttp".into());
    for (key, fallback) in [("path", "/"), ("host", ""), ("mode", "auto")] {
        let val = fields
            .get(key)
            .cloned()
            .or_else(|| extra.get(key).and_then(Value::as_str).map(String::from))
            .unwrap_or_else(|| fallback.to_string());
        extra.insert(key.into(), val.into());
    }
    if let Some(ds_val) = extra.get("downloadSettings").cloned() {
        if extra.contains_key("download") {
            return Err("XHTTP downloadSettings 与 download 不能同时使用".to_string());
        }
        let ds_obj = ds_val
            .as_object()
            .ok_or_else(|| "XHTTP downloadSettings 必须是对象".to_string())?;
        let down = xhttp_download(ds_obj)?;
        extra.insert("download".into(), Value::Object(down));
        extra.shift_remove("downloadSettings");
    }
    if extra.contains_key("download") {
        let mode = extra
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !matches!(mode, "" | "auto" | "packet-up" | "stream-up") {
            return Err("XHTTP 独立下行不能使用 stream-one 或未知 mode".to_string());
        }
    }
    Ok(extra)
}

fn clash_integer(value: Option<&Value>, name: &str) -> Result<i32, String> {
    let Some(v) = value else {
        return Err(format!("{name} 须为整数"));
    };
    if let Some(i) = v.as_i64() {
        return i32::try_from(i).map_err(|_| format!("{name} 须为整数"));
    }
    if let Some(f) = v.as_f64() {
        if f.is_finite() && f.fract() == 0.0 && f >= i32::MIN as f64 && f <= i32::MAX as f64 {
            return Ok(f as i32);
        }
    }
    Err(format!("{name} 须为整数"))
}

fn first_string(val: Option<&Value>) -> Option<String> {
    match val {
        None | Some(Value::Null) => None,
        Some(Value::Array(arr)) => arr.first().map(value_as_string),
        Some(other) => Some(value_as_string(other)),
    }
}

fn string_list(val: Option<&Value>) -> Vec<String> {
    match val {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter(|v| !v.is_null())
            .map(value_as_string)
            .collect(),
        Some(other) => split_hosts(Some(&value_as_string(other))),
    }
}

fn clash_transport(o: &mut Map<String, Value>, m: &Map<String, Value>) -> Result<(), String> {
    let network = m
        .get("network")
        .map(value_as_string)
        .unwrap_or_else(|| "tcp".to_string());
    let opts = m
        .get(&format!("{network}-opts"))
        .and_then(Value::as_object);
    let headers = opts
        .and_then(|op| op.get("headers"))
        .and_then(Value::as_object);
    match network.as_str() {
        "tcp" | "" => {}
        "ws" => {
            let host = first_string(headers.and_then(|h| h.get("Host")));
            let path = opts.and_then(|op| op.get("path")).map(value_as_string);
            if opts.and_then(|op| op.get("v2ray-http-upgrade")).and_then(Value::as_bool) == Some(true) {
                let mut q = IndexMap::new();
                q.insert("type".into(), "httpupgrade".into());
                q.insert("path".into(), path.unwrap_or_else(|| "/".to_string()));
                q.insert("host".into(), host.unwrap_or_default());
                apply_transport(o, &q)?;
            } else {
                let ed = opts
                    .and_then(|op| op.get("max-early-data"))
                    .map(value_as_string)
                    .and_then(|s| s.parse::<i32>().ok());
                let eh = opts
                    .and_then(|op| op.get("early-data-header-name"))
                    .map(value_as_string);
                o.insert(
                    "transport".into(),
                    websocket_transport(path.as_deref(), host.as_deref(), ed, eh.as_deref()),
                );
            }
        }
        "http" => {
            let path = first_string(opts.and_then(|op| op.get("path")))
                .unwrap_or_else(|| "/".to_string());
            let hosts = string_list(headers.and_then(|h| h.get("Host")));
            let meth = opts.and_then(|op| op.get("method")).map(value_as_string);
            o.insert(
                "transport".into(),
                http_transport(Some(&path), hosts, meth.as_deref()),
            );
        }
        "h2" => {
            let path = first_string(opts.and_then(|op| op.get("path")))
                .unwrap_or_else(|| "/".to_string());
            let hosts = string_list(opts.and_then(|op| op.get("host")));
            o.insert("transport".into(), http_transport(Some(&path), hosts, None));
        }
        "grpc" => {
            let mut tr = Map::new();
            tr.insert("type".into(), "grpc".into());
            let svc = opts
                .and_then(|op| op.get("grpc-service-name"))
                .map(value_as_string)
                .unwrap_or_default();
            tr.insert("service_name".into(), svc.into());
            o.insert("transport".into(), Value::Object(tr));
        }
        "xhttp" | "splithttp" => {
            let mut extra = match opts.and_then(|op| op.get("extra")) {
                None | Some(Value::Null) => Map::new(),
                Some(Value::Object(map)) => map.clone(),
                Some(Value::String(raw)) => checked_json_obj(raw)?,
                _ => return Err("XHTTP extra 格式无效".to_string()),
            };
            if let Some(op) = opts {
                for (k, v) in op {
                    if k != "extra" && !v.is_null() {
                        extra.insert(k.clone(), v.clone());
                    }
                }
            }
            let host = first_string(opts.and_then(|op| op.get("host"))).or_else(|| {
                headers.and_then(|h| {
                    h.iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("Host"))
                        .and_then(|(_, v)| first_string(Some(v)))
                })
            });
            let mut fields = IndexMap::new();
            if let Some(h) = host {
                fields.insert("host".into(), h);
            }
            if let Some(p) = first_string(opts.and_then(|op| op.get("path"))) {
                fields.insert("path".into(), p);
            }
            if let Some(mode) = opts.and_then(|op| op.get("mode")).map(value_as_string) {
                fields.insert("mode".into(), mode);
            }
            o.insert(
                "transport".into(),
                Value::Object(xhttp_transport(extra, &fields)?),
            );
        }
        other => {
            let mut q = IndexMap::new();
            q.insert("type".into(), other.to_string());
            q.insert(
                "path".into(),
                first_string(opts.and_then(|op| op.get("path")))
                    .unwrap_or_else(|| "/".to_string()),
            );
            q.insert(
                "host".into(),
                first_string(headers.and_then(|h| h.get("Host"))).unwrap_or_default(),
            );
            apply_transport(o, &q)?;
        }
    }
    Ok(())
}

fn clash_map(m: &Map<String, Value>) -> Result<ParsedNode, String> {
    let s = |key: &str, fallback: &str| -> String {
        m.get(key)
            .filter(|v| !v.is_null())
            .map(value_as_string)
            .unwrap_or_else(|| fallback.to_string())
    };
    let raw_type = s("type", "");
    let kind = match raw_type.as_str() {
        "ss" => "shadowsocks",
        "socks5" => "socks",
        other => other,
    };
    if !matches!(
        kind,
        "shadowsocks"
            | "vmess"
            | "vless"
            | "trojan"
            | "socks"
            | "http"
            | "hysteria"
            | "hysteria2"
            | "tuic"
            | "anytls"
            | "wireguard"
            | "ssh"
            | "snell"
            | "juicity"
            | "ninja"
    ) {
        return Err(format!("Clash 协议 {kind} 暂不兼容；请使用 sing-box JSON"));
    }

    let port_str = s("port", &s("server-port", ""));
    let port: i32 = port_str
        .parse::<i32>()
        .ok()
        .filter(|p| (1..=65535).contains(p))
        .ok_or_else(|| "Clash 节点端口无效".to_string())?;
    let server = s("server", "");
    if server.trim().is_empty() {
        if kind == "ninja" {
            return Err("Ninja 节点缺少服务器".to_string());
        }
        return Err("Clash 节点缺少服务器".to_string());
    }
    let mut o = Map::new();
    o.insert("type".into(), kind.into());
    o.insert("server".into(), server.into());
    o.insert("server_port".into(), port.into());

    let user_key = if kind == "ssh" { "user" } else { "username" };
    for (from, to) in [
        ("password", "password"),
        ("uuid", "uuid"),
        ("flow", "flow"),
        ("username", user_key),
        ("udp-relay-mode", "udp_relay_mode"),
        ("congestion-controller", "congestion_control"),
    ] {
        if let Some(v) = m.get(from).filter(|v| !v.is_null()) {
            o.insert(to.into(), v.clone());
        }
    }

    if kind == "shadowsocks" {
        if let Some(v) = m.get("cipher").filter(|v| !v.is_null()) {
            o.insert("method".into(), v.clone());
        }
    }
    if kind == "vmess" {
        let cipher = s("cipher", "auto");
        o.insert(
            "security".into(),
            if cipher.trim().is_empty() {
                "auto"
            } else {
                &cipher
            }
            .into(),
        );
        let aid = s("alterId", "0").parse::<i32>().unwrap_or(0);
        o.insert("alter_id".into(), aid.into());
    }

    let tls_enabled = m.get("tls").and_then(Value::as_bool) == Some(true)
        || matches!(
            kind,
            "trojan" | "hysteria" | "hysteria2" | "tuic" | "anytls"
        );
    if tls_enabled {
        let sni = s("servername", &s("sni", &s("server", "")));
        let insecure = m.get("skip-cert-verify").and_then(Value::as_bool) == Some(true);
        let mut tls = Map::new();
        tls.insert("enabled".into(), true.into());
        tls.insert("server_name".into(), sni.into());
        tls.insert("insecure".into(), insecure.into());
        if let Some(Value::Array(alpn)) = m.get("alpn") {
            tls.insert("alpn".into(), Value::Array(alpn.clone()));
        }
        if let Some(Value::Object(ro)) = m.get("reality-opts") {
            let mut reality = Map::new();
            reality.insert("enabled".into(), true.into());
            if let Some(pk) = ro.get("public-key") {
                reality.insert("public_key".into(), pk.clone());
            }
            reality.insert(
                "short_id".into(),
                ro.get("short-id")
                    .cloned()
                    .unwrap_or_else(|| Value::String(String::new())),
            );
            tls.insert("reality".into(), Value::Object(reality));
        }
        let fp = m.get("client-fingerprint").map(value_as_string);
        apply_utls(&mut tls, fp.as_deref());
        o.insert("tls".into(), Value::Object(tls));
    }

    if kind == "hysteria" {
        o.insert("auth_str".into(), s("auth-str", &s("auth", "")).into());
        let up_digits: String = s("up", "100").chars().filter(char::is_ascii_digit).collect();
        let down_digits: String = s("down", "100")
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        let up: i32 = if up_digits.is_empty() {
            100
        } else {
            up_digits.parse().unwrap_or(100)
        };
        let down: i32 = if down_digits.is_empty() {
            100
        } else {
            down_digits.parse().unwrap_or(100)
        };
        o.insert("up_mbps".into(), up.into());
        o.insert("down_mbps".into(), down.into());
        if let Some(obfs) = m.get("obfs").filter(|v| !v.is_null()) {
            o.insert("obfs".into(), obfs.clone());
        }
    }

    if kind == "hysteria2" {
        if let Some(obfs_type) = m
            .get("obfs")
            .filter(|v| !v.is_null())
            .map(value_as_string)
            .map(|v| v.trim().to_ascii_lowercase())
            .filter(|v| !v.is_empty() && v != "none")
        {
            if !matches!(obfs_type.as_str(), "salamander" | "gecko") {
                return Err(format!("不支持的 Hysteria2 混淆类型：{obfs_type}"));
            }
            let pwd = s("obfs-password", "");
            if pwd.is_empty() {
                return Err("混淆缺少密码".to_string());
            }
            let mut obfs_obj = Map::new();
            obfs_obj.insert("type".into(), obfs_type.clone().into());
            obfs_obj.insert("password".into(), pwd.into());
            if obfs_type == "gecko" {
                let min_raw = m
                    .get("obfs-min-packet-size")
                    .or_else(|| m.get("min_packet_size"))
                    .filter(|v| !v.is_null())
                    .map(value_as_string);
                let max_raw = m
                    .get("obfs-max-packet-size")
                    .or_else(|| m.get("max_packet_size"))
                    .filter(|v| !v.is_null())
                    .map(value_as_string);
                apply_hysteria2_gecko_packet_sizes(
                    &mut obfs_obj,
                    min_raw.as_deref(),
                    max_raw.as_deref(),
                )?;
            }
            o.insert("obfs".into(), Value::Object(obfs_obj));
        }
    }

    if kind == "snell" {
        let ver: i32 = s("version", "4")
            .parse()
            .map_err(|_| "Snell 版本无效".to_string())?;
        o.insert("psk".into(), s("psk", "").into());
        o.insert("version".into(), ver.into());
        if let Some(Value::Object(ob)) = m.get("obfs-opts") {
            if let Some(mode) = ob.get("mode") {
                o.insert("obfs_mode".into(), mode.clone());
            }
            if let Some(host) = ob.get("host") {
                o.insert("obfs_host".into(), host.clone());
            }
        }
    }

    if kind == "wireguard" {
        let mut addrs = Vec::new();
        if let Some(ip) = m.get("ip").filter(|v| !v.is_null()) {
            addrs.push(Value::String(format!("{}/32", value_as_string(ip))));
        }
        if let Some(ip6) = m.get("ipv6").filter(|v| !v.is_null()) {
            addrs.push(Value::String(format!("{}/128", value_as_string(ip6))));
        }
        if addrs.is_empty() {
            return Err("WireGuard 缺少本地地址".to_string());
        }
        o.insert("address".into(), Value::Array(addrs));
        o.insert("private_key".into(), s("private-key", "").into());
        let mut peer = Map::new();
        peer.insert("address".into(), s("server", "").into());
        peer.insert("port".into(), port.into());
        peer.insert("public_key".into(), s("public-key", "").into());
        peer.insert(
            "allowed_ips".into(),
            Value::Array(vec!["0.0.0.0/0".into(), "::/0".into()]),
        );
        if let Some(psk) = m.get("pre-shared-key").filter(|v| !v.is_null()) {
            peer.insert("pre_shared_key".into(), psk.clone());
        }
        if let Some(Value::Array(res)) = m.get("reserved") {
            peer.insert("reserved".into(), Value::Array(res.clone()));
        }
        o.insert("peers".into(), Value::Array(vec![Value::Object(peer)]));
        o.shift_remove("server");
        o.shift_remove("server_port");
        if let Some(mtu) = m.get("mtu").filter(|v| !v.is_null()) {
            o.insert("mtu".into(), mtu.clone());
        }
    }

    if kind == "ninja" {
        if m.get("tls").and_then(Value::as_bool) != Some(true) {
            return Err("Ninja 仅支持 TLS".to_string());
        }
        let network = m
            .get("network")
            .map(value_as_string)
            .unwrap_or_else(|| "tcp".to_string());
        if !matches!(network.as_str(), "tcp" | "ws") {
            return Err("Ninja 仅支持 tcp/ws 传输".to_string());
        }
        let method = s("method", "");
        let password = s("password", "");
        let node_password = s("node_password", "");
        if method.trim().is_empty() {
            return Err("Ninja 节点缺少加密方法".to_string());
        }
        if password.trim().is_empty() {
            return Err("Ninja 节点缺少密码".to_string());
        }
        if node_password.trim().is_empty() {
            return Err("Ninja 节点缺少节点密码".to_string());
        }
        let Some(pass) = m.get("pass").and_then(Value::as_bool) else {
            return Err("Ninja pass 须为布尔值".to_string());
        };
        o.insert("method".into(), method.into());
        o.insert("password".into(), password.into());
        o.insert("node_password".into(), node_password.into());
        o.insert("pass".into(), pass.into());
        if pass {
            let opts = m
                .get("pass-opts")
                .and_then(Value::as_object)
                .ok_or_else(|| "Ninja pass 缺少 pass-opts".to_string())?;
            let pass_method = opts
                .get("method")
                .map(value_as_string)
                .unwrap_or_default();
            let pass_password = opts
                .get("password")
                .map(value_as_string)
                .unwrap_or_default();
            if pass_method.trim().is_empty() {
                return Err("Ninja pass 缺少加密方法".to_string());
            }
            if pass_password.trim().is_empty() {
                return Err("Ninja pass 缺少密码".to_string());
            }
            let pmin = clash_integer(opts.get("padding_min"), "Ninja pass padding_min")?;
            let pmax = clash_integer(
                opts.get("padding_random_max"),
                "Ninja pass padding_random_max",
            )?;
            if !(0..=65535).contains(&pmin) || !(0..=65535).contains(&pmax) {
                return Err("Ninja pass 填充值超出范围".to_string());
            }
            if pmin + pmax > 65535 {
                return Err("Ninja pass 填充总量超限".to_string());
            }
            let pmode = opts
                .get("padding_mode")
                .map(value_as_string)
                .unwrap_or_default();
            if !matches!(pmode.as_str(), "min" | "random" | "min_random") {
                return Err("Ninja pass 填充模式无效".to_string());
            }
            let mut po = Map::new();
            po.insert("method".into(), pass_method.into());
            po.insert("password".into(), pass_password.into());
            po.insert("padding_min".into(), pmin.into());
            po.insert("padding_random_max".into(), pmax.into());
            po.insert("padding_mode".into(), pmode.into());
            o.insert("pass_options".into(), Value::Object(po));
        }
        let uot = m.get("udp-over-tcp");
        let udp = m.get("udp");
        if let Some(v) = uot {
            if !v.is_null() && !v.is_boolean() {
                return Err("Ninja udp-over-tcp 须为布尔值".to_string());
            }
        }
        if let Some(v) = udp {
            if !v.is_null() && !v.is_boolean() {
                return Err("Ninja udp 须为布尔值".to_string());
            }
        }
        if uot.and_then(Value::as_bool) == Some(true)
            && udp.and_then(Value::as_bool) != Some(false)
        {
            let version = if !m.contains_key("udp-over-tcp-version") {
                2
            } else {
                clash_integer(m.get("udp-over-tcp-version"), "Ninja UDP over TCP 版本")?
            };
            if version != 2 {
                return Err("Ninja 仅支持 UDP over TCP 版本 2".to_string());
            }
            let mut uot_obj = Map::new();
            uot_obj.insert("enabled".into(), true.into());
            uot_obj.insert("version".into(), version.into());
            o.insert("udp_over_tcp".into(), Value::Object(uot_obj));
        }
    }

    if matches!(kind, "hysteria" | "hysteria2") {
        if let Some(ports) = m
            .get("ports")
            .or_else(|| m.get("mport"))
            .or_else(|| m.get("server_ports"))
            .filter(|v| !v.is_null())
        {
            apply_hysteria_ports(&mut o, &value_as_string(ports))?;
        }
        if let Some(hop) = m
            .get("hop-interval")
            .or_else(|| m.get("hop_interval"))
            .filter(|v| !v.is_null())
        {
            o.insert(
                "hop_interval".into(),
                hysteria_hop_interval(&value_as_string(hop))?.into(),
            );
        }
    }

    clash_transport(&mut o, m)?;

    if kind == "shadowsocks" {
        if let Some(plugin) = m.get("plugin").filter(|v| !v.is_null()) {
            o.insert("plugin".into(), plugin.clone());
        }
        if let Some(Value::Object(popts)) = m.get("plugin-opts") {
            let joined = popts
                .iter()
                .map(|(k, v)| format!("{k}={}", value_as_string(v)))
                .collect::<Vec<_>>()
                .join(";");
            o.insert("plugin_opts".into(), joined.into());
        }
    }

    let name = s("name", &s("server", ""));
    canonical(o, &name, "")
}

fn opt_str(m: &Map<String, Value>, key: &str) -> String {
    m.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn opt_nonempty_str(m: &Map<String, Value>, key: &str) -> Option<String> {
    m.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn value_as_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn yaml_to_json(v: &serde_yaml::Value) -> Result<Value, String> {
    match v {
        serde_yaml::Value::Null => Ok(Value::Null),
        serde_yaml::Value::Bool(b) => Ok(Value::Bool(*b)),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Number(i.into()))
            } else if let Some(u) = n.as_u64() {
                Ok(Value::Number(u.into()))
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(Value::Number)
                    .ok_or_else(|| "YAML 数值无效".to_string())
            } else {
                Err("YAML 数值无效".to_string())
            }
        }
        serde_yaml::Value::String(s) => Ok(Value::String(s.clone())),
        serde_yaml::Value::Sequence(seq) => {
            let mut out = Vec::with_capacity(seq.len());
            for item in seq {
                out.push(yaml_to_json(item)?);
            }
            Ok(Value::Array(out))
        }
        serde_yaml::Value::Mapping(map) => {
            let mut out = Map::new();
            for (k, val) in map {
                let key = match k {
                    serde_yaml::Value::String(s) => s.clone(),
                    serde_yaml::Value::Number(n) => n.to_string(),
                    serde_yaml::Value::Bool(b) => b.to_string(),
                    _ => return Err("Clash YAML 键格式无效".to_string()),
                };
                out.insert(key, yaml_to_json(val)?);
            }
            Ok(Value::Object(out))
        }
        serde_yaml::Value::Tagged(t) => yaml_to_json(&t.value),
    }
}

fn apply_hysteria2_gecko_packet_sizes(
    obfs: &mut Map<String, Value>,
    min_raw: Option<&str>,
    max_raw: Option<&str>,
) -> Result<(), String> {
    const GECKO_DEFAULT_MIN: i32 = 512;
    const GECKO_DEFAULT_MAX: i32 = 1200;
    const GECKO_MAX_ON_WIRE: i32 = 2048;

    let parse_size = |raw: Option<&str>, label: &str| -> Result<Option<i32>, String> {
        let Some(text) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let n = text
            .parse::<i32>()
            .map_err(|_| format!("Gecko {label}须为 1–{GECKO_MAX_ON_WIRE} 整数"))?;
        if !(1..=GECKO_MAX_ON_WIRE).contains(&n) {
            return Err(format!("Gecko {label}须为 1–{GECKO_MAX_ON_WIRE} 整数"));
        }
        Ok(Some(n))
    };
    let min_size = parse_size(min_raw, "最小包长")?;
    let max_size = parse_size(max_raw, "最大包长")?;
    let effective_min = min_size.unwrap_or(GECKO_DEFAULT_MIN);
    let effective_max = max_size.unwrap_or(GECKO_DEFAULT_MAX);
    if effective_min > effective_max {
        return Err("Gecko 有效包长范围无效（默认 512–1200，上限 2048）".to_string());
    }
    if let Some(min) = min_size {
        obfs.insert("min_packet_size".into(), min.into());
    }
    if let Some(max) = max_size {
        obfs.insert("max_packet_size".into(), max.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vless_reality_and_hy2() {
        let input = "vless://11111111-1111-1111-1111-111111111111@1.2.3.4:443?security=reality&sni=example.com&pbk=PUBKEY&sid=ab#VLESS1\nhy2://pass@5.6.7.8:443?sni=hy.example.com&mport=20000-20010&hop_interval=30#HY2";
        let report = parse_report(input).unwrap();
        assert_eq!(report.nodes.len(), 2);
        assert_eq!(report.skipped, 0);
        let v: Value = serde_json::from_str(&report.nodes[0].outbound).unwrap();
        assert_eq!(v["tls"]["utls"]["fingerprint"], "chrome");
        let h: Value = serde_json::from_str(&report.nodes[1].outbound).unwrap();
        assert_eq!(h["hop_interval"], "30s");
        assert_eq!(h["server_ports"][0], "20000:20010");
    }

    #[test]
    fn parses_hysteria2_gecko_bare_uri_and_clash_yaml() {
        let bare = "\
hysteria2://pw1@198.51.100.1:443/?sni=hy2.example.com&alpn=h3&insecure=0&obfs=gecko&obfs-password=gpass1#HY2-Gecko-V4\n\
hysteria2://pw2@[2001:db8::1]:443/?sni=hy2.example.com&alpn=h3&insecure=0&obfs=gecko&obfs-password=gpass2&obfs-min-packet-size=600&obfs-max-packet-size=1100#HY2-Gecko-V6\n";
        let report = parse_report(bare).unwrap();
        assert_eq!(report.nodes.len(), 2);
        assert_eq!(report.skipped, 0);

        let n0: Value = serde_json::from_str(&report.nodes[0].outbound).unwrap();
        assert_eq!(n0["obfs"]["type"], "gecko");
        assert_eq!(n0["obfs"]["password"], "gpass1");
        assert!(n0["obfs"].get("min_packet_size").is_none());
        assert!(n0["obfs"].get("max_packet_size").is_none());

        let n1: Value = serde_json::from_str(&report.nodes[1].outbound).unwrap();
        assert_eq!(n1["server"], "2001:db8::1");
        assert_eq!(n1["obfs"]["type"], "gecko");
        assert_eq!(n1["obfs"]["password"], "gpass2");
        assert_eq!(n1["obfs"]["min_packet_size"], 600);
        assert_eq!(n1["obfs"]["max_packet_size"], 1100);

        let missing_pwd = "hysteria2://pw@198.51.100.1:443/?sni=hy2.example.com&obfs=gecko#MissingPwd";
        assert!(parse_report(missing_pwd).is_err());

        let yaml = "\
proxies:
  - name: HY2-Gecko-YAML
    type: hysteria2
    server: hy2.example.com
    port: 443
    password: pw3
    sni: hy2.example.com
    obfs: gecko
    obfs-password: gpass3
    obfs-min-packet-size: 512
    obfs-max-packet-size: 1200
";
        let yaml_report = parse_report(yaml).unwrap();
        assert_eq!(yaml_report.nodes.len(), 1);
        let y0: Value = serde_json::from_str(&yaml_report.nodes[0].outbound).unwrap();
        assert_eq!(y0["obfs"]["type"], "gecko");
        assert_eq!(y0["obfs"]["password"], "gpass3");
        assert_eq!(y0["obfs"]["min_packet_size"], 512);
        assert_eq!(y0["obfs"]["max_packet_size"], 1200);
    }
}
