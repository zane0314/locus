pub mod backup;
pub mod chain_routing;
pub mod config_builder;
pub mod jni_bridge;
pub mod json_depth;
pub mod models;
pub mod multicore_planner;
pub mod node_identity;
pub mod route_math;
pub mod route_rules;
pub mod runtime_logic;
pub mod share_uri;
pub mod subscription_ops;
pub mod subscription_parser;

#[cfg(test)]
mod integration_tests {
    use crate::backup::{backup_selection, validate_app_data, BackupScope};
    use crate::chain_routing::{create_derived_chain_group, ChainTargets};
    use crate::config_builder::{build_config, Purpose};
    use crate::models::{AppData, Group, Node};
    use crate::multicore_planner::{plan_extra_core_with_allocator, CORE_MIHOMO, CORE_XRAY};
    use crate::subscription_parser::parse_report;
    use serde_json::Value;

    #[test]
    fn end_to_end_parse_build_validate_and_multicore() {
        let links = "\
vless://11111111-1111-1111-1111-111111111111@1.2.3.4:443?security=reality&sni=www.microsoft.com&pbk=PUBKEY&sid=abcd&fp=chrome&type=xhttp&host=cdn.example.com&path=/xh#VLESS-Reality\n\
hysteria2://pw@hy2.example.com:443?sni=hy2.example.com&insecure=1&obfs=salamander&obfs-password=ob#HY2-Node\n";
        let report = parse_report(links).unwrap();
        assert_eq!(report.nodes.len(), 2);

        let mut data = AppData {
            groups: vec![Group {
                id: 1,
                name: "Default".into(),
                subscription_url: "https://sub.example.com".into(),
                enabled: true,
                order: 0,
                updated_at: 0,
                user_info: String::new(),
                front_proxy: 0,
                landing_proxy: 0,
                options: "{}".into(),
            }],
            nodes: vec![
                Node {
                    id: 10,
                    group_id: 1,
                    name: report.nodes[0].name.clone(),
                    outbound: report.nodes[0].outbound.clone(),
                    share_link: report.nodes[0].share_link.clone(),
                    ping: 42,
                    tx: 0,
                    rx: 0,
                    order: 0,
                    status: 0,
                    metadata: report.nodes[0].metadata.clone(),
                },
                Node {
                    id: 11,
                    group_id: 1,
                    name: report.nodes[1].name.clone(),
                    outbound: report.nodes[1].outbound.clone(),
                    share_link: report.nodes[1].share_link.clone(),
                    ping: 65,
                    tx: 0,
                    rx: 0,
                    order: 1,
                    status: 0,
                    metadata: report.nodes[1].metadata.clone(),
                },
            ],
            ..AppData::default()
        };
        data.settings.insert("selectedNodeId".into(), "10".into());
        data.settings.insert("selectedGroupId".into(), "1".into());
        validate_app_data(&data, None).unwrap();

        // sing-box config builds cleanly
        let sb_cfg = build_config(&data, Purpose::Main, 0, "sec", None).unwrap();
        let sb_val: Value = serde_json::from_str(&sb_cfg).unwrap();
        assert!(sb_val.get("links_experimental_core").is_none());

        // Xray test build on VLESS Reality + XHTTP
        let mut xray_data = data.clone();
        xray_data
            .settings
            .insert("proxyCore".into(), CORE_XRAY.into());
        let xray_cfg = build_config(&xray_data, Purpose::Test, 10, "", None).unwrap();
        let mut p = 21000u16;
        let plan = plan_extra_core_with_allocator(&xray_cfg, &mut || {
            let cur = p;
            p += 1;
            Ok(cur)
        })
        .unwrap()
        .unwrap();
        assert_eq!(plan.kind, CORE_XRAY);

        // mihomo test build on Hysteria2
        let mut mihomo_data = data.clone();
        mihomo_data
            .settings
            .insert("proxyCore".into(), CORE_MIHOMO.into());
        let mihomo_cfg = build_config(&mihomo_data, Purpose::Test, 11, "", None).unwrap();
        let mut p2 = 22000u16;
        let mplan = plan_extra_core_with_allocator(&mihomo_cfg, &mut || {
            let cur = p2;
            p2 += 1;
            Ok(cur)
        })
        .unwrap()
        .unwrap();
        assert_eq!(mplan.kind, CORE_MIHOMO);

        // Derived chain group & backup selection
        let chained = create_derived_chain_group(
            &data,
            1,
            "Chained",
            &ChainTargets {
                front: "none".into(),
                landing: "none".into(),
            },
        )
        .unwrap();
        assert_eq!(chained.groups.len(), 2);
        let backed = backup_selection(
            &chained,
            &BackupScope {
                configurations: true,
                rules: true,
                settings: true,
            },
        )
        .unwrap();
        assert_eq!(backed.groups.len(), 2);
    }

    #[test]
    fn hybrid_ninja_smart_group_and_power_mode() {
        use crate::runtime_logic::{active_url_test_groups, target_plan_create};

        let clash_missing_srv = "proxies:\n  - name: bad\n    type: ninja\n    port: 443\n    tls: true\n";
        let err = parse_report(clash_missing_srv).unwrap_err();
        assert!(err.contains("Ninja 节点缺少服务器"), "got: {err}");

        let clash_mixed = "\
proxies:
  - name: VLESS-1
    type: vless
    server: 1.2.3.4
    port: 443
    uuid: 11111111-1111-1111-1111-111111111111
    tls: true
    servername: vless.example.com
  - name: Ninja-1
    type: ninja
    server: ninja.example.com
    port: 443
    method: aes-128-gcm
    password: userpw
    node_password: node.example.com
    pass: false
    tls: true
    servername: ninja.example.com
";
        let report = parse_report(clash_mixed).unwrap();
        assert_eq!(report.nodes.len(), 2);

        let mut data = AppData {
            groups: vec![Group {
                id: 1,
                name: "Mixed".into(),
                subscription_url: "https://sub.example.com".into(),
                enabled: true,
                order: 0,
                updated_at: 0,
                user_info: String::new(),
                front_proxy: 0,
                landing_proxy: 0,
                options: "{}".into(),
            }],
            nodes: vec![
                Node {
                    id: 10,
                    group_id: 1,
                    name: report.nodes[0].name.clone(),
                    outbound: report.nodes[0].outbound.clone(),
                    share_link: report.nodes[0].share_link.clone(),
                    ping: 30,
                    tx: 0,
                    rx: 0,
                    order: 0,
                    status: 0,
                    metadata: report.nodes[0].metadata.clone(),
                },
                Node {
                    id: 11,
                    group_id: 1,
                    name: report.nodes[1].name.clone(),
                    outbound: report.nodes[1].outbound.clone(),
                    share_link: report.nodes[1].share_link.clone(),
                    ping: 25,
                    tx: 0,
                    rx: 0,
                    order: 1,
                    status: 0,
                    metadata: report.nodes[1].metadata.clone(),
                },
            ],
            ..AppData::default()
        };
        data.settings.insert("selectedNodeId".into(), "11".into());
        data.settings.insert("selectedGroupId".into(), "1".into());
        data.settings.insert("proxyCore".into(), CORE_XRAY.into());
        data.settings.insert("autoTestPowerMode".into(), "saving".into());
        data.settings.insert("smart.ai.target".into(), "group:1".into());
        data.settings.insert("smartRules.ai".into(), "DOMAIN-SUFFIX,openai.com".into());
        validate_app_data(&data, None).unwrap();

        let main_cfg = build_config(&data, Purpose::Main, 0, "sec", None).unwrap();
        let mut port = 23000u16;
        let plan = plan_extra_core_with_allocator(&main_cfg, &mut || {
            let cur = port;
            port += 1;
            Ok(cur)
        })
        .unwrap()
        .unwrap();
        let sb: Value = serde_json::from_str(&plan.sing_box_config).unwrap();
        let outs = sb["outbounds"].as_array().unwrap();
        assert!(outs.iter().any(|o| o["tag"] == "node-10" && o["type"] == "socks"));
        assert!(outs.iter().any(|o| o["tag"] == "node-11" && o["type"] == "ninja"));
        assert!(outs.iter().any(|o| o["tag"] == "smart-group-1" && o["type"] == "urltest"));

        let active_ut = active_url_test_groups(&plan.sing_box_config).unwrap();
        assert!(active_ut.contains(&"smart-group-1".to_string()));

        let tplan = target_plan_create(&data, "group:1", "ai").unwrap();
        assert_eq!(tplan.group.as_ref().unwrap().tag, "smart-group-1");
        assert!(tplan.group.as_ref().unwrap().url_test);

        // Single-node test on Ninja while proxyCore is Xray returns None cleanly
        let ninja_test_cfg = build_config(&data, Purpose::Test, 11, "", None).unwrap();
        let ninja_plan = plan_extra_core_with_allocator(&ninja_test_cfg, &mut || Ok(24000)).unwrap();
        assert!(ninja_plan.is_none());
    }

    #[test]
    fn smart_panel_summary_matches_individual_helpers() {
        use crate::models::MergeGroup;
        use crate::route_rules::{smart_policy_keys, smart_target};
        use crate::runtime_logic::{
            smart_lowest_node, smart_panel_summary, smart_target_node_ids, target_latency_label,
        };
        use std::collections::HashSet;

        let mut data = AppData {
            groups: vec![
                Group {
                    id: 1,
                    name: "SubA".into(),
                    subscription_url: "https://example.test/a".into(),
                    enabled: true,
                    order: 0,
                    updated_at: 0,
                    user_info: String::new(),
                    front_proxy: 0,
                    landing_proxy: 0,
                    options: "{}".into(),
                },
                Group {
                    id: 2,
                    name: "LocalB".into(),
                    subscription_url: String::new(),
                    enabled: true,
                    order: 1,
                    updated_at: 0,
                    user_info: String::new(),
                    front_proxy: 0,
                    landing_proxy: 0,
                    options: "{}".into(),
                },
                Group {
                    id: 3,
                    name: "DisabledC".into(),
                    subscription_url: "https://example.test/c".into(),
                    enabled: false,
                    order: 2,
                    updated_at: 0,
                    user_info: String::new(),
                    front_proxy: 0,
                    landing_proxy: 0,
                    options: "{}".into(),
                },
            ],
            nodes: vec![
                Node {
                    id: 1,
                    group_id: 1,
                    name: "HK-1".into(),
                    outbound: r#"{"type":"socks","server":"127.0.0.1","server_port":10001}"#.into(),
                    share_link: String::new(),
                    ping: 90,
                    tx: 0,
                    rx: 0,
                    order: 0,
                    status: 3,
                    metadata: "{}".into(),
                },
                Node {
                    id: 2,
                    group_id: 1,
                    name: "HK-2".into(),
                    outbound: r#"{"type":"socks","server":"127.0.0.1","server_port":10002}"#.into(),
                    share_link: String::new(),
                    ping: 60,
                    tx: 0,
                    rx: 0,
                    order: 1,
                    status: 3,
                    metadata: "{}".into(),
                },
                Node {
                    id: 3,
                    group_id: 2,
                    name: "Local-Fast".into(),
                    outbound: r#"{"type":"socks","server":"127.0.0.1","server_port":10003}"#.into(),
                    share_link: String::new(),
                    ping: 20,
                    tx: 0,
                    rx: 0,
                    order: 2,
                    status: 3,
                    metadata: "{}".into(),
                },
                Node {
                    id: 4,
                    group_id: 1,
                    name: "CustomOutbound".into(),
                    outbound: r#"{"type":"custom"}"#.into(),
                    share_link: String::new(),
                    ping: 5,
                    tx: 0,
                    rx: 0,
                    order: 3,
                    status: 3,
                    metadata: "{}".into(),
                },
                Node {
                    id: 5,
                    group_id: 3,
                    name: "Disabled-Fast".into(),
                    outbound: r#"{"type":"socks","server":"127.0.0.1","server_port":10005}"#.into(),
                    share_link: String::new(),
                    ping: 1,
                    tx: 0,
                    rx: 0,
                    order: 4,
                    status: 3,
                    metadata: "{}".into(),
                },
            ],
            merges: vec![MergeGroup {
                id: 10,
                name: "M1".into(),
                node_ids: vec![1, 3],
                group_ids: vec![],
                mode: "urltest".into(),
                selected_id: 1,
            }],
            ..AppData::default()
        };
        data.settings.insert("selectedNodeId".into(), "1".into());
        data.settings.insert("smart.speed.target".into(), "auto".into());
        data.settings.insert("smart.youtube.target".into(), "group:1".into());
        data.settings.insert("smart.netflix.target".into(), "merge:10".into());
        data.settings.insert("smart.x.target".into(), "node:2".into());
        data.settings.insert("smart.telegram.target".into(), "direct".into());
        data.settings.insert("smart.disney.target".into(), "off".into());
        data.settings.insert("smartRules.custom".into(), "DOMAIN,example.test".into());
        data.settings.insert("smartCustom.c1.name".into(), "PayPal".into());
        data.settings.insert("smart.c1.target".into(), "group:2".into());
        data.settings.insert("smartPolicyOrder".into(), "youtube\nspeed\nc1".into());

        for testing in [HashSet::new(), HashSet::from([2i64])] {
            let summary = smart_panel_summary(&data, &testing);
            assert_eq!(summary.policy_order, smart_policy_keys(&data));
            assert_eq!(summary.auto_node_count, smart_target_node_ids(&data, "auto").len());
            assert_eq!(summary.policies.len(), summary.policy_order.len());
            for item in &summary.policies {
                let expected_target = smart_target(&data, &item.key);
                assert_eq!(item.target, expected_target, "key={}", item.key);
                assert_eq!(
                    item.lowest_node,
                    smart_lowest_node(&data, &expected_target, &testing),
                    "lowestNode key={}",
                    item.key
                );
                assert_eq!(
                    item.latency_label,
                    target_latency_label(&data, &expected_target, &testing),
                    "latencyLabel key={}",
                    item.key
                );
            }
        }
    }
}
