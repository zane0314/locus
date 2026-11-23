use crate::backup::{backup_selection, read_log_tail_file, validate_app_data, validate_backup_json, BackupScope};
use crate::chain_routing::{
    chain_path_labels, chain_unavailable_reason, create_derived_chain_group,
    create_derived_chain_node, derived_group_unavailable_reason, effective_chain_targets,
    group_chain_targets, node_chain_targets, sync_derived_chains, validate_chain_settings,
    validate_chain_target_syntax, validate_derived_chains, ChainTargets,
};
use crate::config_builder::{
    build_config, merge_json, normalize_advanced, normalize_transport_host, target_warnings,
    validate_config, warnings, Purpose,
};
use crate::json_depth::require_json_depth;
use crate::models::{AppData, Group, Node};
use crate::multicore_planner::{extract_core_kind, plan_extra_core, with_core_kind};
use crate::node_identity::node_identity_key;
use crate::route_math::subtract_routes as subtract_cidrs;
use crate::route_rules::{
    build_rule_previews, effective_smart_packages, factory_route_rules, filter_vpn_packages,
    geo_asset_code, parse_app_paste, reorder_smart_policies, rule_compatibility_warning_types,
    rule_set_format, smart_policy_keys, smart_target, with_factory_route_defaults,
};
use crate::runtime_logic::{
    active_url_test_groups, chain_parent_probes, chain_probe_keys, node_test_label,
    parse_traffic_samples, read_chain_probe_plans, reorder_visible_ids,
    restorable_selector_choices, routed_chain_nodes, sampling_tracked_tags, saved_choice_delay,
    smart_lowest_node, smart_target_node_ids, target_latency_label, target_plan_create,
    target_plan_create_batch, traffic_domain,
};
use crate::share_uri::{hysteria_hop_interval, percent_decode, socks_user_info, ShareUri};
use crate::subscription_ops::{
    node_removal_ids, repair_legacy_socks_credentials, subscription_apply, subscription_begin,
    subscription_dns_bootstrapped, subscription_dns_server, subscription_due_at,
    subscription_next_at, subscription_record, subscription_scheduled, subscription_should_update,
    SubscriptionOptions,
};
use crate::subscription_parser::{parse_report, ParsedNode};
use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn read_jstring(env: &mut JNIEnv, s: JString) -> Result<String, String> {
    env.get_string(&s)
        .map(|js| js.into())
        .map_err(|e| format!("JNI 字符串读取失败: {e}"))
}

fn return_jstring(env: &mut JNIEnv, out: Result<Value, String>) -> jstring {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    let payload = match out {
        Ok(v) => json!({ "ok": v }).to_string(),
        Err(msg) => json!({ "err": msg }).to_string(),
    };
    env.new_string(payload)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn run_jni<F>(env: &mut JNIEnv, f: F) -> jstring
where
    F: FnOnce(&mut JNIEnv) -> Result<Value, String>,
{
    let result = catch_unwind(AssertUnwindSafe(|| f(env)));
    match result {
        Ok(r) => return_jstring(env, r),
        Err(_) => return_jstring(env, Err("Rust 内核发生内部异常".to_string())),
    }
}

/// Unified JSON-RPC style JNI entrypoint: `RustCore.invoke(op: String, payloadJson: String): String`.
/// Keeps the JNI ABI tiny, 100% panic-safe, and easy to evolve across all 30+ Rust capabilities.
#[no_mangle]
pub extern "system" fn Java_com_zane_zanebox_core_RustCore_nativeInvoke(
    mut env: JNIEnv,
    _class: JClass,
    op: JString,
    payload: JString,
) -> jstring {
    run_jni(&mut env, |env| {
        let op_name = read_jstring(env, op)?;
        let body = read_jstring(env, payload)?;
        dispatch(&op_name, &body)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_zane_zanebox_core_RustCore_nativeGeoAssetCode(
    mut env: JNIEnv,
    _class: JClass,
    kind: JString,
    bytes: jni::objects::JByteArray,
) -> jstring {
    run_jni(&mut env, |env| {
        let kind_str = read_jstring(env, kind)?;
        let raw = env
            .convert_byte_array(bytes)
            .map_err(|e| format!("读取字节数组失败: {e}"))?;
        let code = geo_asset_code(&kind_str, &raw)?;
        Ok(Value::String(code))
    })
}

fn parse_req(body: &str) -> Result<Value, String> {
    serde_json::from_str(body).map_err(|e| format!("请求 JSON 无效: {e}"))
}

fn req_str<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("缺少字符串参数: {key}"))
}

fn req_app_data(v: &Value, key: &str) -> Result<AppData, String> {
    let raw = match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => return Err(format!("缺少数据参数: {key}")),
    };
    AppData::from_json(&raw)
}

fn dispatch(op: &str, body: &str) -> Result<Value, String> {
    match op {
        "version" => Ok(json!({ "name": "locus_core", "version": env!("CARGO_PKG_VERSION") })),

        // JsonDepth & Backup
        "requireJsonDepth" => {
            let req = parse_req(body)?;
            let text = req_str(&req, "text")?;
            require_json_depth(text)?;
            Ok(Value::Bool(true))
        }
        "validateBackupJson" => {
            let req = parse_req(body)?;
            validate_backup_json(req_str(&req, "text")?)?;
            Ok(Value::Bool(true))
        }
        "backupSelection" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let scope: BackupScope = serde_json::from_value(req["scope"].clone())
                .map_err(|e| format!("scope 无效: {e}"))?;
            let selected = backup_selection(&data, &scope)?;
            Ok(Value::String(selected.to_json_raw()?))
        }
        "validateAppData" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let prev = match req.get("previous") {
                Some(Value::Null) | None => None,
                Some(_) => Some(req_app_data(&req, "previous")?),
            };
            validate_app_data(&data, prev.as_ref())?;
            Ok(Value::Bool(true))
        }
        "withoutStaleNodeSettings" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            Ok(Value::String(data.without_stale_node_settings().to_json_raw()?))
        }

        // RouteMath
        "routeMathSubtract" => {
            let req = parse_req(body)?;
            let base: Vec<String> =
                serde_json::from_value(req["base"].clone()).map_err(|e| e.to_string())?;
            let excluded: Vec<String> =
                serde_json::from_value(req["excluded"].clone()).map_err(|e| e.to_string())?;
            Ok(json!(subtract_cidrs(&base, &excluded)?))
        }

        // NodeIdentity & ShareUri & Hysteria
        "nodeIdentityKey" => {
            let req = parse_req(body)?;
            Ok(Value::String(node_identity_key(req_str(&req, "outbound")?)?))
        }
        "shareUriParse" => {
            let req = parse_req(body)?;
            let uri = ShareUri::parse(req_str(&req, "raw")?)?;
            serde_json::to_value(uri).map_err(|e| e.to_string())
        }
        "shareUriDecode" => {
            let req = parse_req(body)?;
            Ok(Value::String(percent_decode(req_str(&req, "value")?)))
        }
        "shareUriSocksUserInfo" => {
            let req = parse_req(body)?;
            let raw = req.get("raw").and_then(Value::as_str);
            Ok(Value::String(socks_user_info(raw)))
        }
        "hysteriaHopInterval" => {
            let req = parse_req(body)?;
            Ok(Value::String(hysteria_hop_interval(req_str(&req, "raw")?)?))
        }

        // SubscriptionParser & SubscriptionOps
        "parseSubscriptionReport" => {
            let req = parse_req(body)?;
            let report = parse_report(req_str(&req, "text")?)?;
            serde_json::to_value(report).map_err(|e| e.to_string())
        }
        "parseSubscriptionOptions" => {
            let req = parse_req(body)?;
            let opts = SubscriptionOptions::parse(req_str(&req, "text")?)?;
            serde_json::to_value(opts).map_err(|e| e.to_string())
        }
        "selectSubscriptionNodes" => {
            let req = parse_req(body)?;
            let opts: SubscriptionOptions =
                serde_json::from_value(req["options"].clone()).map_err(|e| e.to_string())?;
            let nodes: Vec<ParsedNode> =
                serde_json::from_value(req["nodes"].clone()).map_err(|e| e.to_string())?;
            let selected = opts.select(&nodes)?;
            serde_json::to_value(selected).map_err(|e| e.to_string())
        }
        "subscriptionSignature" => {
            let req = parse_req(body)?;
            Ok(Value::String(SubscriptionOptions::signature(req_str(&req, "text")?)?))
        }
        "subscriptionDnsServer" => {
            let req = parse_req(body)?;
            subscription_dns_server(req_str(&req, "address")?, req_str(&req, "tag")?)
        }
        "subscriptionDnsBootstrapped" => {
            let req = parse_req(body)?;
            subscription_dns_bootstrapped(&req["server"], req_str(&req, "address")?)
        }
        "subscriptionDueAt" => {
            let req = parse_req(body)?;
            let group: Group = serde_json::from_value(req["group"].clone()).map_err(|e| e.to_string())?;
            Ok(json!(subscription_due_at(&group)?))
        }
        "subscriptionNextAt" => {
            let req = parse_req(body)?;
            let group: Group = serde_json::from_value(req["group"].clone()).map_err(|e| e.to_string())?;
            Ok(json!(subscription_next_at(&group)?))
        }
        "subscriptionShouldUpdate" => {
            let req = parse_req(body)?;
            let group: Group = serde_json::from_value(req["group"].clone()).map_err(|e| e.to_string())?;
            let now = req["now"].as_i64().ok_or_else(|| "缺少 now".to_string())?;
            let connected = req["connected"].as_bool().unwrap_or(false);
            Ok(Value::Bool(subscription_should_update(&group, now, connected)?))
        }
        "subscriptionScheduled" => {
            let req = parse_req(body)?;
            let group: Group = serde_json::from_value(req["group"].clone()).map_err(|e| e.to_string())?;
            Ok(Value::Bool(subscription_scheduled(&group)?))
        }
        "subscriptionBegin" => {
            let req = parse_req(body)?;
            let current = req_app_data(&req, "current")?;
            let expected: Group = serde_json::from_value(req["expected"].clone()).map_err(|e| e.to_string())?;
            let request = req_str(&req, "request")?;
            let now = req["now"].as_i64().ok_or_else(|| "缺少 now".to_string())?;
            let automatic = req.get("automatic").and_then(Value::as_bool).unwrap_or(false);
            let next = subscription_begin(&current, &expected, request, now, automatic)?;
            Ok(Value::String(next.to_json_raw()?))
        }
        "subscriptionApply" => {
            let req = parse_req(body)?;
            let current = req_app_data(&req, "current")?;
            let baseline: Group = serde_json::from_value(req["baseline"].clone()).map_err(|e| e.to_string())?;
            let parsed: Vec<ParsedNode> = serde_json::from_value(req["parsed"].clone()).map_err(|e| e.to_string())?;
            let user_info = req_str(&req, "userInfo")?;
            let now = req["now"].as_i64().ok_or_else(|| "缺少 now".to_string())?;
            let next_id_start = req.get("nextIdStart").and_then(Value::as_i64);
            let next = subscription_apply(&current, &baseline, &parsed, user_info, now, next_id_start)?;
            Ok(Value::String(next.to_json_raw()?))
        }
        "subscriptionRecord" => {
            let req = parse_req(body)?;
            let current = req_app_data(&req, "current")?;
            let group_id = req["groupId"].as_i64().ok_or_else(|| "缺少 groupId".to_string())?;
            let state = req_str(&req, "state")?;
            let now = req["now"].as_i64().ok_or_else(|| "缺少 now".to_string())?;
            let error = req.get("error").and_then(Value::as_str).unwrap_or("");
            let next_attempt = req.get("nextAttempt").and_then(Value::as_i64).unwrap_or(0);
            let request = req.get("request").and_then(Value::as_str);
            let next = subscription_record(&current, group_id, state, now, error, next_attempt, request)?;
            Ok(Value::String(next.to_json_raw()?))
        }
        "nodeRemovalIds" => {
            let req = parse_req(body)?;
            let nodes: Vec<Node> = serde_json::from_value(req["nodes"].clone()).map_err(|e| e.to_string())?;
            let ids: HashSet<i64> = serde_json::from_value(req["ids"].clone()).map_err(|e| e.to_string())?;
            let out: Vec<i64> = node_removal_ids(&nodes, &ids).into_iter().collect();
            Ok(json!(out))
        }
        "repairLegacySocksCredentials" => {
            let req = parse_req(body)?;
            let node: Node = serde_json::from_value(req["node"].clone()).map_err(|e| e.to_string())?;
            serde_json::to_value(repair_legacy_socks_credentials(&node)).map_err(|e| e.to_string())
        }

        // ChainRouting & DerivedChains
        "groupChainTargets" => {
            let req = parse_req(body)?;
            let group: Group = serde_json::from_value(req["group"].clone()).map_err(|e| e.to_string())?;
            serde_json::to_value(group_chain_targets(&group)).map_err(|e| e.to_string())
        }
        "nodeChainTargets" => {
            let req = parse_req(body)?;
            let node: Node = serde_json::from_value(req["node"].clone()).map_err(|e| e.to_string())?;
            serde_json::to_value(node_chain_targets(&node)).map_err(|e| e.to_string())
        }
        "effectiveChainTargets" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let node: Node = serde_json::from_value(req["node"].clone()).map_err(|e| e.to_string())?;
            serde_json::to_value(effective_chain_targets(&data, &node)).map_err(|e| e.to_string())
        }
        "validateChainTargetSyntax" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            validate_chain_target_syntax(&data)?;
            Ok(Value::Bool(true))
        }
        "validateChainSettings" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            validate_chain_settings(&data)?;
            Ok(Value::Bool(true))
        }
        "validateDerivedChains" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            validate_derived_chains(&data)?;
            Ok(Value::Bool(true))
        }
        "chainUnavailableReason" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let node: Node = serde_json::from_value(req["node"].clone()).map_err(|e| e.to_string())?;
            Ok(json!(chain_unavailable_reason(&data, &node)))
        }
        "derivedGroupUnavailableReason" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let group: Group = serde_json::from_value(req["group"].clone()).map_err(|e| e.to_string())?;
            Ok(json!(derived_group_unavailable_reason(&data, &group)))
        }
        "chainPathLabels" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let node_id = req["nodeId"].as_i64().ok_or_else(|| "缺少 nodeId".to_string())?;
            Ok(json!(chain_path_labels(&data, node_id)))
        }
        "createDerivedChainNode" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let source_id = req["sourceId"].as_i64().ok_or_else(|| "缺少 sourceId".to_string())?;
            let name = req_str(&req, "name")?;
            let targets: ChainTargets = serde_json::from_value(req["targets"].clone()).map_err(|e| e.to_string())?;
            let next = create_derived_chain_node(&data, source_id, name, &targets)?;
            Ok(Value::String(next.to_json_raw()?))
        }
        "createDerivedChainGroup" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let source_group_id = req["sourceGroupId"].as_i64().ok_or_else(|| "缺少 sourceGroupId".to_string())?;
            let name = req_str(&req, "name")?;
            let targets: ChainTargets = serde_json::from_value(req["targets"].clone()).map_err(|e| e.to_string())?;
            let next = create_derived_chain_group(&data, source_group_id, name, &targets)?;
            Ok(Value::String(next.to_json_raw()?))
        }
        "syncDerivedChains" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let next = sync_derived_chains(&data)?;
            Ok(Value::String(next.to_json_raw()?))
        }

        // RouteDefaults & AppSelection & RulePreview & RouteAssets
        "factoryRouteRules" => {
            let req = parse_req(body)?;
            let rules = factory_route_rules(req_str(&req, "country")?);
            serde_json::to_value(rules).map_err(|e| e.to_string())
        }
        "withFactoryRouteDefaults" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let next = with_factory_route_defaults(&data, req_str(&req, "country")?)?;
            Ok(Value::String(next.to_json_raw()?))
        }
        "smartPolicyKeys" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            Ok(json!(smart_policy_keys(&data)))
        }
        "reorderSmartPolicies" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let requested: Vec<String> = serde_json::from_value(req["requested"].clone()).map_err(|e| e.to_string())?;
            let next = reorder_smart_policies(&data, &requested)?;
            Ok(Value::String(next.to_json_raw()?))
        }
        "smartTarget" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            Ok(Value::String(smart_target(&data, req_str(&req, "key")?)))
        }
        "ruleSetFormat" => {
            let req = parse_req(body)?;
            Ok(Value::String(rule_set_format(req_str(&req, "url")?).to_string()))
        }
        "ruleCompatibilityWarningTypes" => {
            let req = parse_req(body)?;
            Ok(json!(rule_compatibility_warning_types(req_str(&req, "rules")?)))
        }
        "filterVpnPackages" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let pkgs: Vec<String> = serde_json::from_value(req["packages"].clone()).map_err(|e| e.to_string())?;
            let res: Vec<String> = filter_vpn_packages(&data, pkgs.iter().map(String::as_str)).into_iter().collect();
            Ok(json!(res))
        }
        "effectiveSmartPackages" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let key = req_str(&req, "key")?;
            let installed: Option<HashSet<String>> = match req.get("installed") {
                Some(Value::Null) | None => None,
                Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| e.to_string())?),
            };
            let res: Vec<String> = effective_smart_packages(&data, key, installed.as_ref()).into_iter().collect();
            Ok(json!(res))
        }
        "parseAppPaste" => {
            let req = parse_req(body)?;
            let text = req_str(&req, "text")?;
            let installed: HashSet<String> = serde_json::from_value(req["installed"].clone()).map_err(|e| e.to_string())?;
            let scope = match req.get("scope") {
                Some(Value::Null) | None => None,
                Some(_) => Some(req_app_data(&req, "scope")?),
            };
            serde_json::to_value(parse_app_paste(text, &installed, scope.as_ref())).map_err(|e| e.to_string())
        }
        "buildRulePreviews" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let geoip = req.get("geoipPresent").and_then(Value::as_bool).unwrap_or(false);
            let geosite = req.get("geositePresent").and_then(Value::as_bool).unwrap_or(false);
            let builtin: HashMap<String, String> = match req.get("builtinTexts") {
                Some(Value::Null) | None => HashMap::new(),
                Some(v) => serde_json::from_value(v.clone()).map_err(|e| e.to_string())?,
            };
            let installed: Option<HashSet<String>> = match req.get("installed") {
                Some(Value::Null) | None => None,
                Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| e.to_string())?),
            };
            serde_json::to_value(build_rule_previews(&data, geoip, geosite, &builtin, installed.as_ref()))
                .map_err(|e| e.to_string())
        }

        // ConfigBuilder
        "buildConfig" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let purpose = Purpose::from_str(req.get("purpose").and_then(Value::as_str).unwrap_or("MAIN"))?;
            let test_node_id = req.get("testNodeId").and_then(Value::as_i64).unwrap_or(0);
            let secret = req.get("runtimeSecret").and_then(Value::as_str).unwrap_or("");
            let installed: Option<HashSet<String>> = match req.get("installedPackages") {
                Some(Value::Null) | None => None,
                Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| e.to_string())?),
            };
            let cfg = build_config(&data, purpose, test_node_id, secret, installed.as_ref())?;
            Ok(Value::String(cfg))
        }
        "validateConfig" => {
            let req = parse_req(body)?;
            validate_config(req_str(&req, "config")?)?;
            Ok(Value::Bool(true))
        }
        "configWarnings" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            Ok(json!(warnings(&data)))
        }
        "configTargetWarnings" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            Ok(json!(target_warnings(&data)))
        }
        "smartTargetNodeIds" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            Ok(json!(smart_target_node_ids(&data, req_str(&req, "target")?)))
        }
        "normalizeTransportHost" => {
            let req = parse_req(body)?;
            let mut obj = req["outbound"]
                .as_object()
                .cloned()
                .ok_or_else(|| "outbound 必须是对象".to_string())?;
            normalize_transport_host(&mut obj);
            Ok(Value::Object(obj))
        }
        "normalizeAdvanced" => {
            let req = parse_req(body)?;
            let mut obj = req["rule"]
                .as_object()
                .cloned()
                .ok_or_else(|| "rule 必须是对象".to_string())?;
            normalize_advanced(&mut obj)?;
            Ok(Value::Object(obj))
        }
        "mergeJson" => {
            let req = parse_req(body)?;
            let mut base = req["base"]
                .as_object()
                .cloned()
                .ok_or_else(|| "base 必须是对象".to_string())?;
            let custom = req["custom"]
                .as_object()
                .ok_or_else(|| "custom 必须是对象".to_string())?;
            merge_json(&mut base, custom);
            Ok(Value::Object(base))
        }

        // MultiCorePlanner (sing-box / Xray / mihomo)
        "extractCoreKind" => {
            let req = parse_req(body)?;
            Ok(Value::String(extract_core_kind(req_str(&req, "config")?)?))
        }
        "withCoreKind" => {
            let req = parse_req(body)?;
            Ok(Value::String(with_core_kind(
                req_str(&req, "config")?,
                req_str(&req, "coreKind")?,
            )?))
        }
        "planExtraCore" => {
            let req = parse_req(body)?;
            let plan = plan_extra_core(req_str(&req, "config")?)?;
            serde_json::to_value(plan).map_err(|e| e.to_string())
        }

        // RuntimeLogic
        "samplingTrackedTags" => {
            let req = parse_req(body)?;
            Ok(json!(sampling_tracked_tags(req_str(&req, "config")?)?))
        }
        "activeUrlTestGroups" => {
            let req = parse_req(body)?;
            Ok(json!(active_url_test_groups(req_str(&req, "config")?)?))
        }
        "trafficDomain" => {
            let req = parse_req(body)?;
            Ok(Value::String(traffic_domain(req_str(&req, "value")?)))
        }
        "parseTrafficSamples" => {
            let req = parse_req(body)?;
            let samples = parse_traffic_samples(req_str(&req, "text")?)?;
            serde_json::to_value(samples).map_err(|e| e.to_string())
        }
        "readLogTail" => {
            let req = parse_req(body)?;
            let path = req_str(&req, "path")?;
            let max_bytes = req
                .get("maxBytes")
                .and_then(Value::as_u64)
                .unwrap_or(256 * 1024) as usize;
            Ok(json!(read_log_tail_file(path, max_bytes)?))
        }
        "readChainProbePlans" => {
            let req = parse_req(body)?;
            serde_json::to_value(read_chain_probe_plans(req_str(&req, "config")?)?)
                .map_err(|e| e.to_string())
        }
        "chainProbeKeys" => {
            let req = parse_req(body)?;
            serde_json::to_value(chain_probe_keys(req_str(&req, "config")?)?)
                .map_err(|e| e.to_string())
        }
        "chainParentProbes" => {
            let req = parse_req(body)?;
            serde_json::to_value(chain_parent_probes(req_str(&req, "config")?)?)
                .map_err(|e| e.to_string())
        }
        "routedChainNodes" => {
            let req = parse_req(body)?;
            Ok(json!(routed_chain_nodes(req_str(&req, "config")?)?))
        }
        "savedChoiceDelay" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let tag = req_str(&req, "tag")?;
            let ping = req["nodePing"].as_i64().unwrap_or(-1) as i32;
            Ok(json!(saved_choice_delay(&data, tag, ping)))
        }
        "restorableSelectorChoices" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            serde_json::to_value(restorable_selector_choices(&data, req_str(&req, "config")?)?)
                .map_err(|e| e.to_string())
        }
        "targetPlanCreate" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let target = req_str(&req, "target")?;
            let service_key = req.get("serviceKey").and_then(Value::as_str).unwrap_or("");
            serde_json::to_value(target_plan_create(&data, target, service_key))
                .map_err(|e| e.to_string())
        }
        "targetPlanCreateBatch" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let targets: Vec<String> =
                serde_json::from_value(req["targets"].clone()).map_err(|e| e.to_string())?;
            let service_key = req.get("serviceKey").and_then(Value::as_str).unwrap_or("");
            serde_json::to_value(target_plan_create_batch(&data, &targets, service_key))
                .map_err(|e| e.to_string())
        }
        "reorderVisibleIds" => {
            let req = parse_req(body)?;
            let current: Vec<i64> = serde_json::from_value(req["current"].clone()).map_err(|e| e.to_string())?;
            let requested: Vec<i64> = serde_json::from_value(req["requested"].clone()).map_err(|e| e.to_string())?;
            Ok(json!(reorder_visible_ids(&current, &requested)?))
        }
        "nodeTestLabel" => {
            let req = parse_req(body)?;
            let status = req["status"].as_i64().unwrap_or(0) as i32;
            let ping = req["ping"].as_i64().unwrap_or(-1) as i32;
            Ok(Value::String(node_test_label(status, ping)))
        }
        "targetLatencyLabel" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let target = req_str(&req, "target")?;
            let testing: HashSet<i64> = match req.get("testing") {
                Some(Value::Null) | None => HashSet::new(),
                Some(v) => serde_json::from_value(v.clone()).map_err(|e| e.to_string())?,
            };
            Ok(json!(target_latency_label(&data, target, &testing)))
        }
        "smartLowestNode" => {
            let req = parse_req(body)?;
            let data = req_app_data(&req, "data")?;
            let value = req_str(&req, "value")?;
            let testing: HashSet<i64> = match req.get("testing") {
                Some(Value::Null) | None => HashSet::new(),
                Some(v) => serde_json::from_value(v.clone()).map_err(|e| e.to_string())?,
            };
            Ok(json!(smart_lowest_node(&data, value, &testing)))
        }

        other => Err(format!("未知 RustCore 操作: {other}")),
    }
}
