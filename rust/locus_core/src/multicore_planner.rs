use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::net::{TcpListener, UdpSocket};

pub const CORE_SING_BOX: &str = "sing-box";
pub const CORE_XRAY: &str = "xray";
pub const CORE_MIHOMO: &str = "mihomo";
pub const EXPERIMENTAL_CORE_KEY: &str = "links_experimental_core";
pub const DNS_BRIDGE_TAG: &str = "links-extra-core-dns";

const PASSTHROUGH_OUTBOUNDS: &[&str] = &["direct", "block", "dns", "selector", "urltest"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtraCorePlan {
    pub kind: String,
    #[serde(rename = "singBoxConfig")]
    pub sing_box_config: String,
    #[serde(rename = "extraCoreConfig")]
    pub extra_core_config: String,
    pub ports: Vec<u16>,
}

pub fn normalize_core_kind(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "xray" => CORE_XRAY,
        "mihomo" => CORE_MIHOMO,
        _ => CORE_SING_BOX,
    }
}

pub fn extract_core_kind(config: &str) -> Result<String, String> {
    let root: Value = serde_json::from_str(config).map_err(|e| format!("配置 JSON 无效: {e}"))?;
    let Some(obj) = root.as_object() else {
        return Err("配置根节点必须是对象".to_string());
    };
    let kind = obj
        .get(EXPERIMENTAL_CORE_KEY)
        .and_then(Value::as_str)
        .unwrap_or(CORE_SING_BOX);
    match kind {
        CORE_SING_BOX | CORE_XRAY | CORE_MIHOMO => Ok(kind.to_string()),
        other => Err(format!("不支持的代理内核: {other}")),
    }
}

pub fn with_core_kind(config: &str, core_kind: &str) -> Result<String, String> {
    let normalized = normalize_core_kind(core_kind);
    let mut root: Value =
        serde_json::from_str(config).map_err(|e| format!("配置 JSON 无效: {e}"))?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "配置根节点必须是对象".to_string())?;
    if normalized == CORE_SING_BOX {
        obj.shift_remove(EXPERIMENTAL_CORE_KEY);
    } else {
        obj.insert(
            EXPERIMENTAL_CORE_KEY.into(),
            Value::String(normalized.to_string()),
        );
    }
    serde_json::to_string_pretty(&root).map_err(|e| e.to_string())
}

pub fn allocate_loopback_port(used: &mut HashSet<u16>) -> Result<u16, String> {
    for _ in 0..64 {
        let tcp = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|e| format!("分配本地端口失败: {e}"))?;
        let port = tcp
            .local_addr()
            .map_err(|e| format!("读取本地端口失败: {e}"))?
            .port();
        if used.contains(&port) {
            continue;
        }
        if UdpSocket::bind(("127.0.0.1", port)).is_ok() {
            used.insert(port);
            return Ok(port);
        }
    }
    Err("无法分配可用本地端口".to_string())
}

pub fn plan_extra_core(config: &str) -> Result<Option<ExtraCorePlan>, String> {
    let mut used = HashSet::new();
    plan_extra_core_with_allocator(config, &mut || allocate_loopback_port(&mut used))
}

pub fn plan_extra_core_with_allocator(
    config: &str,
    alloc_port: &mut dyn FnMut() -> Result<u16, String>,
) -> Result<Option<ExtraCorePlan>, String> {
    let kind = extract_core_kind(config)?;
    if kind == CORE_SING_BOX {
        return Ok(None);
    }
    let mut root: Value =
        serde_json::from_str(config).map_err(|e| format!("配置 JSON 无效: {e}"))?;
    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| "配置根节点必须是对象".to_string())?;
    root_obj.shift_remove(EXPERIMENTAL_CORE_KEY);

    if let Some(Value::Array(ep)) = root_obj.get("endpoints") {
        if !ep.is_empty() {
            return Err("暂不支持 endpoints 节点（包括 WireGuard）".to_string());
        }
    }

    let default_resolver_strategy = {
        let route_obj = root_obj
            .get("route")
            .and_then(Value::as_object)
            .ok_or_else(|| "缺少 route 配置".to_string())?;
        match route_obj.get("default_domain_resolver") {
            Some(Value::Object(o)) => {
                check_allowed_keys(o, &["server", "strategy"], "default_domain_resolver")?;
                if o.get("server").and_then(Value::as_str) != Some("dns-direct") {
                    return Err("节点 DNS 服务器必须为 dns-direct".to_string());
                }
                o.get("strategy")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            }
            Some(Value::String(s)) => {
                if s != "dns-direct" {
                    return Err("节点 DNS 服务器必须为 dns-direct".to_string());
                }
                String::new()
            }
            None => String::new(),
            _ => return Err("节点 DNS 服务器必须为 dns-direct".to_string()),
        }
    };
    if !matches!(
        default_resolver_strategy.as_str(),
        "" | "ipv4_only" | "ipv6_only"
    ) {
        return Err(format!(
            "外核暂不支持此节点 DNS 优先级: {default_resolver_strategy}"
        ));
    }

    let orig_outbounds = root_obj
        .get("outbounds")
        .and_then(Value::as_array)
        .ok_or_else(|| "缺少 outbounds 配置".to_string())?
        .clone();

    let mut new_sing_outbounds: Vec<Value> = Vec::with_capacity(orig_outbounds.len());
    let mut extra_outbounds: Vec<Value> = Vec::new();
    let mut extra_inbounds: Vec<Value> = Vec::new();
    let mut xray_rules: Vec<Value> = Vec::new();
    let mut ports: Vec<u16> = Vec::new();
    let mut seen_tags: HashSet<String> = HashSet::new();

    for (idx, item) in orig_outbounds.into_iter().enumerate() {
        let obj = item
            .as_object()
            .ok_or_else(|| "outbound 必须是对象".to_string())?;
        let otype = obj
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "outbound 缺少 type".to_string())?;
        let tag = obj
            .get("tag")
            .and_then(Value::as_str)
            .ok_or_else(|| "outbound 缺少 tag".to_string())?;
        if tag.trim().is_empty() || !seen_tags.insert(tag.to_string()) {
            return Err("outbound tag 无效或重复".to_string());
        }

        if PASSTHROUGH_OUTBOUNDS.contains(&otype) {
            new_sing_outbounds.push(item);
            continue;
        }

        let extra_out = translate_outbound(&kind, obj)?;
        let port = alloc_port()?;
        ports.push(port);

        new_sing_outbounds.push(json!({
            "type": "socks",
            "tag": tag,
            "server": "127.0.0.1",
            "server_port": port,
            "version": "5",
        }));
        extra_outbounds.push(extra_out);

        let in_tag = format!("links-extra-{idx}");
        if kind == CORE_XRAY {
            extra_inbounds.push(json!({
                "tag": in_tag,
                "listen": "127.0.0.1",
                "port": port,
                "protocol": "socks",
                "settings": { "udp": true },
            }));
            xray_rules.push(json!({
                "type": "field",
                "inboundTag": [in_tag],
                "outboundTag": tag,
            }));
        } else {
            extra_inbounds.push(json!({
                "name": in_tag,
                "type": "socks",
                "listen": "127.0.0.1",
                "port": port,
                "udp": true,
                "proxy": tag,
            }));
        }
    }

    if extra_outbounds.is_empty() {
        return Err("没有外核支持的节点".to_string());
    }
    root_obj.insert("outbounds".into(), Value::Array(new_sing_outbounds));

    // Attach DNS bridge inbound to sing-box
    let inbounds = root_obj
        .entry("inbounds".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let inbounds_arr = inbounds
        .as_array_mut()
        .ok_or_else(|| "inbounds 无效".to_string())?;
    if inbounds_arr
        .iter()
        .any(|v| v.get("tag").and_then(Value::as_str) == Some(DNS_BRIDGE_TAG))
    {
        return Err("DNS 桥 tag 冲突".to_string());
    }
    let dns_port = alloc_port()?;
    ports.push(dns_port);
    inbounds_arr.push(json!({
        "type": "direct",
        "tag": DNS_BRIDGE_TAG,
        "listen": "127.0.0.1",
        "listen_port": dns_port,
    }));

    // Prepend hijack-dns route rule for DNS bridge
    let route_obj = root_obj
        .get_mut("route")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "缺少 route 配置".to_string())?;
    let existing_route_rules = route_obj
        .get("rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut new_route_rules = Vec::with_capacity(existing_route_rules.len() + 1);
    new_route_rules.push(json!({
        "inbound": [DNS_BRIDGE_TAG],
        "action": "hijack-dns",
    }));
    new_route_rules.extend(existing_route_rules);
    route_obj.insert("rules".into(), Value::Array(new_route_rules));

    // Route DNS bridge traffic to dns-direct (preserving leading dns-hosts rules)
    let dns_obj = root_obj
        .get_mut("dns")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "缺少 dns 配置".to_string())?;
    let has_dns_direct = dns_obj
        .get("servers")
        .and_then(Value::as_array)
        .is_some_and(|servers| {
            servers
                .iter()
                .any(|s| s.get("tag").and_then(Value::as_str) == Some("dns-direct"))
        });
    if !has_dns_direct {
        return Err("缺少 dns-direct".to_string());
    }
    let orig_dns_rules = dns_obj
        .get("rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut new_dns_rules = Vec::with_capacity(orig_dns_rules.len() + 2);
    let mut split_idx = 0;
    while split_idx < orig_dns_rules.len()
        && orig_dns_rules[split_idx]
            .get("server")
            .and_then(Value::as_str)
            == Some("dns-hosts")
    {
        new_dns_rules.push(orig_dns_rules[split_idx].clone());
        split_idx += 1;
    }
    if !default_resolver_strategy.is_empty() {
        let qtype = if default_resolver_strategy == "ipv4_only" {
            "AAAA"
        } else {
            "A"
        };
        new_dns_rules.push(json!({
            "inbound": [DNS_BRIDGE_TAG],
            "query_type": [qtype],
            "action": "predefined",
            "rcode": "NOERROR",
        }));
    }
    new_dns_rules.push(json!({
        "inbound": [DNS_BRIDGE_TAG],
        "action": "route",
        "server": "dns-direct",
    }));
    new_dns_rules.extend_from_slice(&orig_dns_rules[split_idx..]);
    dns_obj.insert("rules".into(), Value::Array(new_dns_rules));

    let core_config = if kind == CORE_XRAY {
        json!({
            "inbounds": extra_inbounds,
            "outbounds": extra_outbounds,
            "routing": { "rules": xray_rules },
        })
    } else {
        json!({
            "listeners": extra_inbounds,
            "proxies": extra_outbounds,
            "mode": "rule",
            "rules": [],
        })
    };

    let extra_envelope = json!({
        "dnsServer": format!("127.0.0.1:{dns_port}"),
        "coreConfig": core_config,
    });

    Ok(Some(ExtraCorePlan {
        kind,
        sing_box_config: root.to_string(),
        extra_core_config: extra_envelope.to_string(),
        ports,
    }))
}

fn check_allowed_keys(obj: &Map<String, Value>, allowed: &[&str], scope: &str) -> Result<(), String> {
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{scope} 暂不支持字段: {key}"));
        }
    }
    Ok(())
}

fn copy_field(src: &Map<String, Value>, dst: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(v) = src.get(from) {
        if !v.is_null() {
            dst.insert(to.to_string(), v.clone());
        }
    }
}

fn translate_outbound(kind: &str, obj: &Map<String, Value>) -> Result<Value, String> {
    let otype = obj.get("type").and_then(Value::as_str).unwrap_or("");
    let tag = obj.get("tag").and_then(Value::as_str).unwrap_or("");

    let extra_allowed: &[&str] = match otype {
        "shadowsocks" => &["method", "password"],
        "trojan" => &["password", "tls", "transport"],
        "vless" => &["uuid", "flow", "tls", "transport", "packet_encoding"],
        "vmess" => &["uuid", "security", "alter_id", "tls", "transport"],
        "socks" => &["version", "username", "password"],
        "http" => &["username", "password", "tls"],
        "hysteria2" => {
            if kind == CORE_XRAY {
                return Err("xray 暂不支持协议: hysteria2".to_string());
            }
            &["password", "up_mbps", "down_mbps", "obfs", "tls"]
        }
        "anytls" => {
            if kind == CORE_XRAY {
                return Err("xray 暂不支持协议: anytls".to_string());
            }
            &["password", "tls"]
        }
        other => return Err(format!("{kind} 暂不支持协议: {other}")),
    };

    let mut allowed = vec!["type", "tag", "server", "server_port"];
    allowed.extend_from_slice(extra_allowed);
    check_allowed_keys(obj, &allowed, &format!("{kind} {otype}"))?;

    for sub in ["tls", "transport", "obfs"] {
        if let Some(v) = obj.get(sub) {
            if !v.is_object() {
                return Err(format!("{sub} 必须是对象"));
            }
        }
    }

    if kind == CORE_MIHOMO && matches!(otype, "trojan" | "hysteria2" | "anytls") {
        let tls_enabled = obj
            .get("tls")
            .and_then(Value::as_object)
            .and_then(|t| t.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !tls_enabled {
            return Err(format!("mihomo {otype} 必须启用 TLS"));
        }
    }

    let server = obj
        .get("server")
        .and_then(Value::as_str)
        .ok_or_else(|| "服务器无效".to_string())?;
    let port = obj
        .get("server_port")
        .and_then(Value::as_i64)
        .ok_or_else(|| "端口无效".to_string())?;
    if server.trim().is_empty() || !(1..=65535).contains(&port) {
        return Err("节点服务器或端口无效".to_string());
    }

    if matches!(otype, "socks" | "http") && obj.contains_key("password") {
        let user = obj.get("username").and_then(Value::as_str).unwrap_or("");
        if user.is_empty() {
            return Err("代理密码要求同时设置用户名".to_string());
        }
    }

    let required_non_empty: &[&str] = match otype {
        "shadowsocks" => &["method", "password"],
        "trojan" | "hysteria2" | "anytls" => &["password"],
        "vless" | "vmess" => &["uuid"],
        _ => &[],
    };
    for field in required_non_empty {
        let ok = obj
            .get(*field)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty());
        if !ok {
            return Err(format!("{otype} 缺少必填字段: {field}"));
        }
    }

    if otype == "socks" {
        let ver = obj.get("version").and_then(Value::as_str).unwrap_or("5");
        if ver != "5" {
            return Err("仅支持 SOCKS5".to_string());
        }
    }

    if kind == CORE_MIHOMO && otype == "vless" {
        if let Some(pe) = obj.get("packet_encoding") {
            let pe_str = pe
                .as_str()
                .ok_or_else(|| "mihomo 无法保留此 VLESS packet_encoding".to_string())?;
            if !matches!(pe_str, "xudp" | "packetaddr") {
                return Err("mihomo 无法保留此 VLESS packet_encoding".to_string());
            }
        }
    }

    if kind == CORE_XRAY {
        translate_xray_outbound(tag, otype, server, port, obj)
    } else {
        translate_mihomo_outbound(tag, otype, server, port, obj)
    }
}

fn translate_xray_outbound(
    tag: &str,
    otype: &str,
    server: &str,
    port: i64,
    obj: &Map<String, Value>,
) -> Result<Value, String> {
    let mut settings = Map::new();
    settings.insert("address".into(), Value::String(server.to_string()));
    settings.insert("port".into(), Value::Number(port.into()));

    match otype {
        "shadowsocks" => {
            copy_field(obj, &mut settings, "method", "method");
            copy_field(obj, &mut settings, "password", "password");
        }
        "trojan" => {
            copy_field(obj, &mut settings, "password", "password");
        }
        "http" | "socks" => {
            copy_field(obj, &mut settings, "username", "user");
            copy_field(obj, &mut settings, "password", "pass");
        }
        "vless" => {
            copy_field(obj, &mut settings, "uuid", "id");
            settings.insert("encryption".into(), Value::String("none".to_string()));
            copy_field(obj, &mut settings, "flow", "flow");
            if let Some(pe) = obj.get("packet_encoding") {
                if pe.as_str() != Some("xudp") {
                    return Err("Xray 暂不支持此 packet_encoding".to_string());
                }
            }
        }
        "vmess" => {
            copy_field(obj, &mut settings, "uuid", "id");
            let sec = obj
                .get("security")
                .and_then(Value::as_str)
                .unwrap_or("auto");
            settings.insert("security".into(), Value::String(sec.to_string()));
            if obj.get("alter_id").and_then(Value::as_i64).unwrap_or(0) != 0 {
                return Err("Xray 暂不支持 VMess alter_id".to_string());
            }
        }
        _ => {}
    }

    let mut stream = Map::new();
    if let Some(transport) = obj.get("transport").and_then(Value::as_object) {
        let ttype = transport
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "transport 缺少 type".to_string())?;
        let mut tsettings = Map::new();
        match ttype {
            "ws" | "httpupgrade" => {
                check_allowed_keys(
                    transport,
                    &["type", "host", "path", "headers"],
                    &format!("Xray {ttype}"),
                )?;
                for f in ["host", "path", "headers"] {
                    copy_field(transport, &mut tsettings, f, f);
                }
                stream.insert("network".into(), Value::String(ttype.to_string()));
                let key = if ttype == "ws" {
                    "wsSettings"
                } else {
                    "httpupgradeSettings"
                };
                stream.insert(key.into(), Value::Object(tsettings));
            }
            "grpc" => {
                check_allowed_keys(transport, &["type", "service_name"], "Xray grpc")?;
                copy_field(transport, &mut tsettings, "service_name", "serviceName");
                stream.insert("network".into(), Value::String("grpc".to_string()));
                stream.insert("grpcSettings".into(), Value::Object(tsettings));
            }
            "xhttp" => {
                check_allowed_keys(
                    transport,
                    &["type", "host", "path", "mode", "headers"],
                    "Xray xhttp",
                )?;
                for f in ["host", "path", "mode", "headers"] {
                    copy_field(transport, &mut tsettings, f, f);
                }
                stream.insert("network".into(), Value::String("xhttp".to_string()));
                stream.insert("xhttpSettings".into(), Value::Object(tsettings));
            }
            other => return Err(format!("Xray 暂不支持传输: {other}")),
        }
    }

    if let Some(tls) = obj.get("tls").and_then(Value::as_object) {
        check_allowed_keys(
            tls,
            &["enabled", "server_name", "insecure", "alpn", "utls", "reality"],
            "Xray TLS",
        )?;
        for sub in ["utls", "reality"] {
            if let Some(v) = tls.get(sub) {
                if !v.is_object() {
                    return Err(format!("{sub} 必须是对象"));
                }
            }
        }
        let enabled = tls.get("enabled").and_then(Value::as_bool).unwrap_or(false);
        if !enabled && (tls.len() > 1 || (tls.len() == 1 && !tls.contains_key("enabled"))) {
            return Err("TLS 关闭时不能设置其他 TLS 字段".to_string());
        }
        if enabled {
            let mut tls_out = Map::new();
            copy_field(tls, &mut tls_out, "server_name", "serverName");
            copy_field(tls, &mut tls_out, "insecure", "allowInsecure");
            copy_field(tls, &mut tls_out, "alpn", "alpn");
            if let Some(utls) = tls.get("utls").and_then(Value::as_object) {
                check_allowed_keys(utls, &["enabled", "fingerprint"], "uTLS")?;
                if utls.get("enabled").and_then(Value::as_bool).unwrap_or(false) {
                    copy_field(utls, &mut tls_out, "fingerprint", "fingerprint");
                } else if utls.contains_key("fingerprint") {
                    return Err("uTLS 已关闭但指定 fingerprint".to_string());
                }
            }
            if let Some(reality) = tls.get("reality").and_then(Value::as_object) {
                if reality.get("enabled").and_then(Value::as_bool).unwrap_or(false) {
                    check_allowed_keys(
                        reality,
                        &["enabled", "public_key", "short_id"],
                        "REALITY",
                    )?;
                    if tls.get("insecure").and_then(Value::as_bool).unwrap_or(false)
                        || tls.contains_key("alpn")
                    {
                        return Err("REALITY 不支持 insecure 或 alpn".to_string());
                    }
                    let net = stream.get("network").and_then(Value::as_str).unwrap_or("");
                    if !matches!(net, "" | "tcp" | "xhttp" | "grpc") {
                        return Err("Xray REALITY 不支持此传输".to_string());
                    }
                    tls_out.shift_remove("allowInsecure");
                    copy_field(reality, &mut tls_out, "public_key", "publicKey");
                    copy_field(reality, &mut tls_out, "short_id", "shortId");
                    stream.insert("security".into(), Value::String("reality".to_string()));
                    stream.insert("realitySettings".into(), Value::Object(tls_out));
                } else {
                    if reality.len() != 1 || !reality.contains_key("enabled") {
                        return Err("REALITY 已关闭但指定其他字段".to_string());
                    }
                    stream.insert("security".into(), Value::String("tls".to_string()));
                    stream.insert("tlsSettings".into(), Value::Object(tls_out));
                }
            } else {
                stream.insert("security".into(), Value::String("tls".to_string()));
                stream.insert("tlsSettings".into(), Value::Object(tls_out));
            }
        }
    }

    let mut out = Map::new();
    out.insert("tag".into(), Value::String(tag.to_string()));
    out.insert("protocol".into(), Value::String(otype.to_string()));
    out.insert("settings".into(), Value::Object(settings));
    if !stream.is_empty() {
        out.insert("streamSettings".into(), Value::Object(stream));
    }
    if otype == "vless" {
        out.insert(
            "mux".into(),
            json!({
                "enabled": true,
                "concurrency": -1,
                "xudpConcurrency": 1,
                "xudpProxyUDP443": "allow",
            }),
        );
    }
    Ok(Value::Object(out))
}

fn translate_mihomo_outbound(
    tag: &str,
    otype: &str,
    server: &str,
    port: i64,
    obj: &Map<String, Value>,
) -> Result<Value, String> {
    let mihomo_type = match otype {
        "socks" => "socks5",
        "shadowsocks" => "ss",
        other => other,
    };
    let mut out = Map::new();
    out.insert("name".into(), Value::String(tag.to_string()));
    out.insert("type".into(), Value::String(mihomo_type.to_string()));
    out.insert("server".into(), Value::String(server.to_string()));
    out.insert("port".into(), Value::Number(port.into()));

    if matches!(
        otype,
        "socks" | "shadowsocks" | "vmess" | "vless" | "trojan" | "anytls"
    ) {
        out.insert("udp".into(), Value::Bool(true));
    }

    match otype {
        "shadowsocks" => {
            copy_field(obj, &mut out, "method", "cipher");
            copy_field(obj, &mut out, "password", "password");
        }
        "trojan" | "anytls" => {
            copy_field(obj, &mut out, "password", "password");
        }
        "hysteria2" => {
            copy_field(obj, &mut out, "password", "password");
            if let Some(up) = obj.get("up_mbps").and_then(Value::as_i64) {
                out.insert("up".into(), Value::String(format!("{up} Mbps")));
            }
            if let Some(down) = obj.get("down_mbps").and_then(Value::as_i64) {
                out.insert("down".into(), Value::String(format!("{down} Mbps")));
            }
            if let Some(obfs) = obj.get("obfs").and_then(Value::as_object) {
                check_allowed_keys(obfs, &["type", "password"], "Hysteria2 obfs")?;
                copy_field(obfs, &mut out, "type", "obfs");
                copy_field(obfs, &mut out, "password", "obfs-password");
            }
        }
        "http" | "socks" => {
            copy_field(obj, &mut out, "username", "username");
            copy_field(obj, &mut out, "password", "password");
        }
        "vless" => {
            copy_field(obj, &mut out, "uuid", "uuid");
            copy_field(obj, &mut out, "flow", "flow");
            copy_field(obj, &mut out, "packet_encoding", "packet-encoding");
        }
        "vmess" => {
            copy_field(obj, &mut out, "uuid", "uuid");
            let alter = obj.get("alter_id").and_then(Value::as_i64).unwrap_or(0);
            let sec = obj
                .get("security")
                .and_then(Value::as_str)
                .unwrap_or("auto");
            out.insert("alterId".into(), Value::Number(alter.into()));
            out.insert("cipher".into(), Value::String(sec.to_string()));
        }
        _ => {}
    }

    if let Some(transport) = obj.get("transport").and_then(Value::as_object) {
        let ttype = transport
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "transport 缺少 type".to_string())?;
        if ttype == "ws" {
            check_allowed_keys(
                transport,
                &[
                    "type",
                    "host",
                    "path",
                    "headers",
                    "max_early_data",
                    "early_data_header_name",
                ],
                "mihomo ws",
            )?;
            let mut ws_opts = Map::new();
            copy_field(transport, &mut ws_opts, "path", "path");
            copy_field(transport, &mut ws_opts, "max_early_data", "max-early-data");
            copy_field(
                transport,
                &mut ws_opts,
                "early_data_header_name",
                "early-data-header-name",
            );
            let mut headers = transport
                .get("headers")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if let Some(host) = transport.get("host").and_then(Value::as_str) {
                if headers.keys().any(|k| k.eq_ignore_ascii_case("host")) {
                    return Err("WS host 重复".to_string());
                }
                headers.insert("Host".into(), Value::String(host.to_string()));
            }
            if !headers.is_empty() {
                ws_opts.insert("headers".into(), Value::Object(headers));
            }
            out.insert("network".into(), Value::String("ws".to_string()));
            out.insert("ws-opts".into(), Value::Object(ws_opts));
        } else if ttype == "grpc" {
            check_allowed_keys(transport, &["type", "service_name"], "mihomo grpc")?;
            let mut grpc_opts = Map::new();
            copy_field(transport, &mut grpc_opts, "service_name", "grpc-service-name");
            out.insert("network".into(), Value::String("grpc".to_string()));
            out.insert("grpc-opts".into(), Value::Object(grpc_opts));
        } else {
            return Err(format!("mihomo 暂不支持传输: {ttype}"));
        }
    }

    if let Some(tls) = obj.get("tls").and_then(Value::as_object) {
        check_allowed_keys(
            tls,
            &["enabled", "server_name", "insecure", "alpn", "utls", "reality"],
            "mihomo TLS",
        )?;
        for sub in ["utls", "reality"] {
            if let Some(v) = tls.get(sub) {
                if !v.is_object() {
                    return Err(format!("{sub} 必须是对象"));
                }
            }
        }
        let enabled = tls.get("enabled").and_then(Value::as_bool).unwrap_or(false);
        if !enabled && (tls.len() > 1 || (tls.len() == 1 && !tls.contains_key("enabled"))) {
            return Err("TLS 关闭时不能设置其他 TLS 字段".to_string());
        }
        if enabled {
            if !matches!(
                otype,
                "http" | "vmess" | "vless" | "trojan" | "hysteria2" | "anytls"
            ) {
                return Err("此协议暂不支持 TLS".to_string());
            }
            if matches!(otype, "http" | "vmess" | "vless") {
                out.insert("tls".into(), Value::Bool(true));
            }
            let sni_key = if matches!(otype, "vmess" | "vless") {
                "servername"
            } else {
                "sni"
            };
            copy_field(tls, &mut out, "server_name", sni_key);
            copy_field(tls, &mut out, "insecure", "skip-cert-verify");
            if tls.contains_key("alpn") {
                if otype == "http" {
                    return Err("HTTP 暂不支持 ALPN".to_string());
                }
                copy_field(tls, &mut out, "alpn", "alpn");
            }
            if let Some(utls) = tls.get("utls").and_then(Value::as_object) {
                check_allowed_keys(utls, &["enabled", "fingerprint"], "uTLS")?;
                if utls.get("enabled").and_then(Value::as_bool).unwrap_or(false) {
                    if !matches!(otype, "vmess" | "vless" | "trojan" | "anytls") {
                        return Err("此协议暂不支持 uTLS".to_string());
                    }
                    copy_field(utls, &mut out, "fingerprint", "client-fingerprint");
                } else if utls.contains_key("fingerprint") {
                    return Err("uTLS 已关闭但指定 fingerprint".to_string());
                }
            }
            if let Some(reality) = tls.get("reality").and_then(Value::as_object) {
                check_allowed_keys(
                    reality,
                    &["enabled", "public_key", "short_id"],
                    "REALITY",
                )?;
                if reality.get("enabled").and_then(Value::as_bool).unwrap_or(false) {
                    if !matches!(otype, "vmess" | "vless" | "trojan") {
                        return Err("此协议暂不支持 REALITY".to_string());
                    }
                    let mut ropts = Map::new();
                    copy_field(reality, &mut ropts, "public_key", "public-key");
                    copy_field(reality, &mut ropts, "short_id", "short-id");
                    out.insert("reality-opts".into(), Value::Object(ropts));
                } else if reality.len() > 1
                    || (reality.len() == 1 && !reality.contains_key("enabled"))
                {
                    return Err("REALITY 已关闭但指定其他字段".to_string());
                }
            }
        }
    }

    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_xray_vless_reality_and_xhttp() {
        let cfg = json!({
            "links_experimental_core": "xray",
            "inbounds": [],
            "outbounds": [
                {
                    "type": "vless",
                    "tag": "node-1",
                    "server": "1.2.3.4",
                    "server_port": 443,
                    "uuid": "11111111-1111-1111-1111-111111111111",
                    "flow": "xtls-rprx-vision",
                    "transport": { "type": "xhttp", "host": "cdn.example.com", "path": "/xh", "mode": "auto" },
                    "tls": {
                        "enabled": true,
                        "server_name": "www.microsoft.com",
                        "utls": { "enabled": true, "fingerprint": "chrome" },
                        "reality": { "enabled": true, "public_key": "pub", "short_id": "abcd" }
                    }
                },
                { "type": "direct", "tag": "direct" },
                { "type": "selector", "tag": "proxy", "outbounds": ["node-1"], "default": "node-1" }
            ],
            "route": { "rules": [], "final": "proxy", "default_domain_resolver": "dns-direct" },
            "dns": {
                "servers": [
                    { "type": "local", "tag": "dns-direct" },
                    { "type": "https", "tag": "dns-remote", "server": "1.1.1.1" }
                ],
                "rules": []
            }
        });
        let mut next_port = 20000u16;
        let plan = plan_extra_core_with_allocator(&cfg.to_string(), &mut || {
            let p = next_port;
            next_port += 1;
            Ok(p)
        })
        .unwrap()
        .unwrap();
        assert_eq!(plan.kind, "xray");
        assert_eq!(plan.ports, vec![20000, 20001]);

        let sb: Value = serde_json::from_str(&plan.sing_box_config).unwrap();
        assert!(sb.get("links_experimental_core").is_none());
        assert_eq!(sb["outbounds"][0]["type"], "socks");
        assert_eq!(sb["outbounds"][0]["server_port"], 20000);
        assert_eq!(sb["inbounds"][0]["tag"], DNS_BRIDGE_TAG);
        assert_eq!(sb["inbounds"][0]["listen_port"], 20001);

        let extra: Value = serde_json::from_str(&plan.extra_core_config).unwrap();
        assert_eq!(extra["dnsServer"], "127.0.0.1:20001");
        let xout = &extra["coreConfig"]["outbounds"][0];
        assert_eq!(xout["protocol"], "vless");
        assert_eq!(xout["streamSettings"]["network"], "xhttp");
        assert_eq!(xout["streamSettings"]["security"], "reality");
        assert_eq!(xout["streamSettings"]["realitySettings"]["publicKey"], "pub");
        assert_eq!(xout["mux"]["xudpConcurrency"], 1);
    }

    #[test]
    fn plans_mihomo_hysteria2_and_anytls() {
        let cfg = json!({
            "links_experimental_core": "mihomo",
            "inbounds": [],
            "outbounds": [
                {
                    "type": "hysteria2",
                    "tag": "node-2",
                    "server": "hy2.example.com",
                    "server_port": 443,
                    "password": "secret",
                    "up_mbps": 50,
                    "down_mbps": 200,
                    "obfs": { "type": "salamander", "password": "obfs" },
                    "tls": { "enabled": true, "server_name": "hy2.example.com", "insecure": true }
                },
                {
                    "type": "anytls",
                    "tag": "node-3",
                    "server": "any.example.com",
                    "server_port": 8443,
                    "password": "pw",
                    "tls": { "enabled": true, "server_name": "any.example.com", "utls": { "enabled": true, "fingerprint": "firefox" } }
                },
                { "type": "direct", "tag": "direct" }
            ],
            "route": { "rules": [], "final": "node-2", "default_domain_resolver": "dns-direct" },
            "dns": {
                "servers": [{ "type": "local", "tag": "dns-direct" }],
                "rules": []
            }
        });
        let mut p = 30000u16;
        let plan = plan_extra_core_with_allocator(&cfg.to_string(), &mut || {
            let cur = p;
            p += 1;
            Ok(cur)
        })
        .unwrap()
        .unwrap();
        assert_eq!(plan.kind, "mihomo");
        let extra: Value = serde_json::from_str(&plan.extra_core_config).unwrap();
        let proxies = extra["coreConfig"]["proxies"].as_array().unwrap();
        assert_eq!(proxies.len(), 2);
        assert_eq!(proxies[0]["type"], "hysteria2");
        assert_eq!(proxies[0]["up"], "50 Mbps");
        assert_eq!(proxies[0]["obfs-password"], "obfs");
        assert_eq!(proxies[1]["type"], "anytls");
        assert_eq!(proxies[1]["client-fingerprint"], "firefox");
        assert_eq!(extra["coreConfig"]["listeners"].as_array().unwrap().len(), 2);
    }
}
