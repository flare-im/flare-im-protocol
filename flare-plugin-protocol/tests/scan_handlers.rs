use flare_plugin_protocol::{PluginManifest, manifest::UiContribution, ui::UiActionTarget};
use serde_json::{Value, json};

fn scan_manifest() -> Value {
    let mut manifest: Value =
        serde_json::from_slice(include_bytes!("fixtures/plugin-ui-v2-manifest.json")).unwrap();
    manifest["capabilities"][0]["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id":"scan", "mode":"query", "idempotent":true,
            "inputSchema":{"type":"object"}, "outputSchema":{"type":"object"},
            "permissionIds":["example.poll.read"], "timeoutMs":5000
        }));
    manifest["clientExtensions"]["contributions"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!({"kind":"page", "id":"example.poll.scan-page", "schemaVersion":1,
            "requiredPermissions":[], "source":{"operation":"example.poll.scan","refreshMs":5000},
            "schema":{"title":"Scan"}}),
            json!({"kind":"scan-handler", "id":"example.poll.scan-handler", "schemaVersion":1,
            "requiredPermissions":["example.poll.read"], "schema":{"title":"Example"},
            "scan":{"routeKey":"demo", "resolve":"example.poll.scan",
                "pages":["example.poll.scan-page"], "sources":["camera","album"]}}),
        ]);
    manifest
}

fn parse(value: &Value) -> Result<PluginManifest, flare_plugin_protocol::ProtocolError> {
    PluginManifest::from_json(&serde_json::to_vec(value).unwrap())
}

fn handler(value: &mut Value) -> &mut Value {
    value["clientExtensions"]["contributions"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()
}

fn page(value: &mut Value) -> &mut Value {
    value["clientExtensions"]["contributions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == "example.poll.scan-page")
        .unwrap()
}

#[test]
fn scan_contract_is_query_only_and_claims_a_permanent_exact_route() {
    let manifest = parse(&scan_manifest()).unwrap();
    assert!(
        manifest
            .client_operation_refs()
            .contains("example.poll.scan")
    );
    assert!(manifest.claimed_namespaces().contains("scan-route.demo"));
    let mut contribution = manifest
        .contribution("example.poll.scan-handler")
        .unwrap()
        .clone();
    contribution.retain_targets(|target| {
        !matches!(target,
        UiActionTarget::Operation { operation } if operation == "example.poll.scan")
    });
    assert!(contribution.scan.is_none());
    let mut command = scan_manifest();
    command["capabilities"][0]["operations"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["mode"] = json!("command");
    assert!(parse(&command).is_err());
}

#[test]
fn scan_routes_pages_sources_and_single_handler_are_bounded() {
    for route in ["a", "abcdef", "Ab", "1a", "a-", "éa", "a.b"] {
        let mut value = scan_manifest();
        handler(&mut value)["scan"]["routeKey"] = json!(route);
        assert!(parse(&value).is_err(), "route {route}");
    }
    for pages in [
        json!([]),
        json!(["example.poll.other"]),
        json!(["example.poll.scan-page", "example.poll.scan-page"]),
    ] {
        let mut value = scan_manifest();
        handler(&mut value)["scan"]["pages"] = pages;
        assert!(parse(&value).is_err());
    }
    let mut value = scan_manifest();
    handler(&mut value)["scan"]["sources"] = json!(["camera", "camera"]);
    assert!(parse(&value).is_err());
    let mut value = scan_manifest();
    let mut extra = handler(&mut value).clone();
    extra["id"] = json!("example.poll.other-handler");
    value["clientExtensions"]["contributions"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    assert!(parse(&value).is_err());
    let mut value = scan_manifest();
    handler(&mut value)["kind"] = json!("page");
    assert!(parse(&value).is_err());
}

#[test]
fn only_approved_idle_pages_can_refresh() {
    let manifest = parse(&scan_manifest()).unwrap();
    let page = manifest.contribution("example.poll.scan-page").unwrap();
    let view = page.resolve_page(&json!({}), "en", "viewer").unwrap();
    assert_eq!(view.refresh_ms, Some(5000));
    for interval in [0, 2999, 60001] {
        let mut value = scan_manifest();
        self::page(&mut value)["source"]["refreshMs"] = json!(interval);
        assert!(parse(&value).is_err());
    }
    let mut value = scan_manifest();
    self::page(&mut value)["source"]["cursorPath"] = json!("/cursor");
    assert!(parse(&value).is_err());
    for renderer in ["payment.confirm", "redPacket.pay"] {
        let mut value = scan_manifest();
        self::page(&mut value)["schema"]["presentation"] = json!({"renderer":renderer});
        assert!(parse(&value).is_err());
    }
    let mut value = scan_manifest();
    self::page(&mut value)["schema"]["inputs"] = json!([
        {"id":"password", "kind":"password", "label":"Password"}
    ]);
    assert!(parse(&value).is_err());
}

#[test]
fn scan_handler_serialization_contains_no_unapproved_navigation() {
    let value = scan_manifest();
    let mut contribution: UiContribution = serde_json::from_value(
        value["clientExtensions"]["contributions"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .clone(),
    )
    .unwrap();
    contribution.retain_targets(|_| false);
    assert!(
        serde_json::to_value(contribution)
            .unwrap()
            .get("scan")
            .is_none()
    );
}
