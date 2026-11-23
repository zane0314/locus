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
}
