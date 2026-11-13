//! Web bundles in the platform's projection: what each application's clients fetch and the
//! bridge permissions the application approved.
use flare_plugin_protocol::authorization::TenantPluginGrant;
use flare_plugin_protocol::bridge::BridgePermission;
use flare_plugin_protocol::runtime::{
    PluginProjection, ProjectedLifecycle, ProjectedPlugin, ProjectedWebBundle,
};
use serde_json::json;

fn bundle() -> ProjectedWebBundle {
    ProjectedWebBundle {
        id: "board".into(),
        sha256: "ab".repeat(32),
        urls: vec!["https://m1.example.com/b/board.fwb".into()],
        files: Some("https://m1.example.com/b/files/".into()),
        permissions: [BridgePermission::PluginQuery, BridgePermission::Storage].into(),
    }
}

fn projection(bundles: Vec<ProjectedWebBundle>) -> PluginProjection {
    let now = 1_000_000;
    PluginProjection {
        tenant_id: "201".into(),
        organization_id: None,
        version: 1,
        issued_at_ms: now,
        valid_until_ms: now + 60_000,
        plugins: vec![ProjectedPlugin {
            grant: TenantPluginGrant {
                grant_id: "grant-1".into(),
                revision: 1,
                tenant_id: "201".into(),
                plugin_id: "example.board".into(),
                plugin_version: "1.0.0".into(),
                manifest_digest: format!("sha256:{}", "a".repeat(64)),
                approved_operations: Default::default(),
                approved_permissions: Default::default(),
                approved_data_scopes: Default::default(),
                enabled: true,
                revoked: false,
                suspended: false,
                valid_from_ms: now,
                valid_until_ms: now + 60_000,
            },
            lifecycle: ProjectedLifecycle::Installed,
            export_until_ms: None,
            features: Default::default(),
            web_bundles: bundles,
        }],
        bindings: Vec::new(),
    }
}

#[test]
fn the_wire_shape_is_camel_case_and_omitted_when_empty() {
    let raw = serde_json::to_value(&projection(vec![bundle()]).plugins[0]).unwrap();
    assert_eq!(
        raw["webBundles"],
        json!([{"id": "board", "sha256": "ab".repeat(32),
            "urls": ["https://m1.example.com/b/board.fwb"],
            "files": "https://m1.example.com/b/files/",
            "permissions": ["plugin.query", "storage"]}])
    );
    let raw = serde_json::to_value(&projection(vec![]).plugins[0]).unwrap();
    assert!(raw.get("webBundles").is_none());
    assert!(
        serde_json::from_value::<ProjectedWebBundle>(json!({"id": "board",
            "sha256": "ab".repeat(32), "urls": ["https://m/x.fwb"], "size": 1}))
        .is_err(),
        "unknown fields are refused"
    );
}

#[test]
fn a_projection_with_valid_bundles_validates() {
    assert!(projection(vec![bundle()]).validate(1_000_001).is_ok());
}

#[test]
fn invalid_projected_bundles_refuse_the_projection() {
    type Breaker = Box<dyn Fn(&mut ProjectedWebBundle)>;
    let cases: Vec<(&str, Breaker)> = vec![
        ("id", Box::new(|b| b.id = "Board".into())),
        ("digest", Box::new(|b| b.sha256 = "sha256:ab".into())),
        ("no urls", Box::new(|b| b.urls.clear())),
        (
            "http url",
            Box::new(|b| b.urls = vec!["http://m/x.fwb".into()]),
        ),
        (
            "too many urls",
            Box::new(|b| b.urls = vec!["https://m/x.fwb".into(); 9]),
        ),
        (
            "files without slash",
            Box::new(|b| b.files = Some("https://m/files".into())),
        ),
        (
            "files http",
            Box::new(|b| b.files = Some("http://m/files/".into())),
        ),
    ];
    for (name, change) in cases {
        let mut broken = bundle();
        change(&mut broken);
        assert!(
            projection(vec![broken]).validate(1_000_001).is_err(),
            "{name}"
        );
    }
    assert!(
        projection(vec![bundle(), bundle()])
            .validate(1_000_001)
            .is_err(),
        "duplicate ids"
    );
    let many = (0..9)
        .map(|i| ProjectedWebBundle {
            id: format!("b{i}"),
            ..bundle()
        })
        .collect();
    assert!(projection(many).validate(1_000_001).is_err(), "over 8");
}
