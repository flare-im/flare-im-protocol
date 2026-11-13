//! Web bundles (L3) without the archive reader: the manifest declaration,
//! the catalog entry, `bundle.json`, the entry document rules, the script
//! scan, Core's selection with bundles, and the bridge protocol.
use std::collections::BTreeSet;

use flare_plugin_protocol::{
    PluginManifest,
    bridge::*,
    bundle::*,
    manifest::{ClientPlatform, UiContribution},
    ui::{BundleRuntime, ClientPlugin, ClientUiDescriptor, NativeCapability, engine_vocabulary},
};
use serde_json::{Value, json};

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn decl() -> WebBundleDecl {
    serde_json::from_value(json!({"id": "dashboard", "sha256": DIGEST, "size": 4096, "bridge": 1}))
        .unwrap()
}

/// The poll fixture with a dashboard page whose first variant is the
/// `dashboard` bundle, a transient `pay` command and the bundle declared.
fn manifest_value() -> Value {
    let mut manifest: Value =
        serde_json::from_slice(include_bytes!("fixtures/plugin-ui-v2-manifest.json")).unwrap();
    let operations = manifest["capabilities"][0]["operations"]
        .as_array_mut()
        .unwrap();
    operations.push(json!({
        "id": "dash", "idempotent": true, "inputSchema": {"type": "object"},
        "outputSchema": {"type": "object"}, "permissionIds": ["example.poll.read"],
        "timeoutMs": 5000, "mode": "query"
    }));
    operations.push(json!({
        "id": "pay", "idempotent": true, "inputSchema": {"type": "object"},
        "outputSchema": {"type": "object"}, "permissionIds": ["example.poll.read"],
        "timeoutMs": 5000, "transient": true
    }));
    manifest["clientExtensions"]["contributions"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "kind": "page", "id": "example.poll.dashboard", "schemaVersion": 1,
            "requiredPermissions": ["example.poll.read"],
            "schema": {"title": "Dashboard"},
            "variants": [
                {"id": "bundle", "requires": {"bundle": {"id": "dashboard", "bridge": 1}},
                 "bundle": {"id": "dashboard", "route": "/"}},
                {"id": "basic", "schema": {"title": "Dashboard", "fields": [
                    {"label": "Open", "path": "/open"}]},
                 "source": {"operation": "example.poll.dash"}}
            ]
        }));
    manifest["clientExtensions"]["nativeCapabilities"] = json!(["selectImage"]);
    manifest["clientExtensions"]["webBundles"] = json!([serde_json::to_value(decl()).unwrap()]);
    manifest
}

fn parse(manifest: &Value) -> Result<PluginManifest, flare_plugin_protocol::ProtocolError> {
    PluginManifest::from_json(&serde_json::to_vec(manifest).unwrap())
}

fn manifest() -> PluginManifest {
    parse(&manifest_value()).unwrap()
}

fn bundle_json() -> Value {
    json!({
        "id": "dashboard", "entry": "index.html", "bridge": 1,
        "permissions": ["plugin.query", "plugin.command", "ui.navigate", "storage"],
        "operations": ["example.poll.dash", "example.poll.vote"],
        "capabilities": ["selectImage"],
        "locales": ["zh-CN", "en"]
    })
}

fn bundle_manifest(raw: &Value) -> Result<BundleManifest, BundleError> {
    BundleManifest::from_json(&serde_json::to_vec(raw).unwrap())
}

// ---- declaration ----

#[test]
fn a_declaration_is_an_id_a_hex_digest_a_bounded_size_and_a_known_bridge() {
    assert_eq!(decl().validate_declared(), Ok(()));
    let refused = |edit: &dyn Fn(&mut WebBundleDecl)| {
        let mut decl = decl();
        edit(&mut decl);
        decl.validate_declared().is_err()
    };
    assert!(refused(&|d| d.id = "Dashboard".into()));
    assert!(refused(&|d| d.sha256 = format!("sha256:{DIGEST}")));
    assert!(refused(&|d| d.sha256 = DIGEST.to_uppercase()));
    assert!(refused(&|d| d.sha256 = DIGEST[1..].into()));
    assert!(refused(&|d| d.size = 0));
    assert!(refused(&|d| d.size = MAX_WEB_BUNDLE_BYTES + 1));
    assert!(!refused(&|d| d.size = MAX_WEB_BUNDLE_BYTES));
    assert!(refused(&|d| d.bridge = 0));
    assert!(refused(&|d| d.bridge = BRIDGE_VERSION + 1));
    // Catalog fields are Core's.
    assert!(refused(
        &|d| d.urls = vec!["https://m1.example/a.fwb".into()]
    ));
    assert!(refused(&|d| d.files = Some("https://m1.example/a/".into())));
    assert!(refused(&|d| {
        d.permissions.insert(BridgePermission::Storage);
    }));
    // Unknown fields do not parse.
    assert!(
        serde_json::from_value::<WebBundleDecl>(json!({
            "id": "dashboard", "sha256": DIGEST, "size": 1, "bridge": 1, "entry": "x"
        }))
        .is_err()
    );
}

#[test]
fn a_manifest_declares_every_bundle_its_variants_open_and_no_other() {
    let manifest = manifest();
    assert_eq!(
        manifest.client_extensions.web_bundle("dashboard"),
        Some(&decl())
    );
    let raw = serde_json::to_value(&manifest).unwrap();
    assert_eq!(raw["clientExtensions"]["webBundles"][0]["sha256"], DIGEST);
    assert!(
        raw["clientExtensions"]["webBundles"][0]
            .get("urls")
            .is_none()
    );
    assert!(
        raw["clientExtensions"]["webBundles"][0]
            .get("digitalGoods")
            .is_none()
    );

    let rejected = |edit: &dyn Fn(&mut Value)| {
        let mut manifest = manifest_value();
        edit(&mut manifest);
        parse(&manifest).is_err()
    };
    // A variant opens an undeclared bundle.
    assert!(rejected(
        &|m| m["clientExtensions"]["webBundles"] = json!([])
    ));
    assert!(rejected(&|m| {
        m["clientExtensions"]["webBundles"][0]["id"] = json!("other");
    }));
    // A declared bundle no variant opens.
    assert!(rejected(&|m| {
        let other = json!({"id": "other", "sha256": DIGEST, "size": 1, "bridge": 1});
        m["clientExtensions"]["webBundles"]
            .as_array_mut()
            .unwrap()
            .push(other);
    }));
    // Duplicates, invalid declarations, catalog fields.
    assert!(rejected(&|m| {
        let again = m["clientExtensions"]["webBundles"][0].clone();
        m["clientExtensions"]["webBundles"]
            .as_array_mut()
            .unwrap()
            .push(again);
    }));
    assert!(rejected(
        &|m| m["clientExtensions"]["webBundles"][0]["size"] = json!(0)
    ));
    assert!(rejected(&|m| {
        m["clientExtensions"]["webBundles"][0]["urls"] = json!(["https://m1.example/a.fwb"]);
    }));
    // At most eight.
    assert!(rejected(&|m| {
        let bundles = m["clientExtensions"]["webBundles"].as_array_mut().unwrap();
        for index in 0..MAX_WEB_BUNDLES {
            bundles
                .push(json!({"id": format!("b{index}"), "sha256": DIGEST, "size": 1, "bridge": 1}));
        }
    }));
    assert!(!rejected(&|m| {
        m["clientExtensions"]["webBundles"][0]["digitalGoods"] = json!(true);
    }));
}

#[test]
fn a_catalog_entry_has_https_mirrors_and_an_https_files_base() {
    let mut entry = decl();
    entry.urls = vec![
        "https://m1.example/plugins/example.poll/x.fwb".into(),
        "https://m2.example/plugins/example.poll/x.fwb".into(),
    ];
    entry.files = Some(format!("https://m1.example/plugins/example.poll/{DIGEST}/"));
    entry.permissions = BTreeSet::from([BridgePermission::PluginQuery]);
    assert_eq!(entry.validate_catalog(), Ok(()));
    assert_eq!(entry.declared(), decl());
    let refused = |edit: &dyn Fn(&mut WebBundleDecl)| {
        let mut entry = entry.clone();
        edit(&mut entry);
        entry.validate_catalog().is_err()
    };
    assert!(refused(&|e| e.urls.clear()));
    assert!(refused(&|e| e.urls[0] = "http://m1.example/x.fwb".into()));
    assert!(refused(
        &|e| e.urls[0] = "https://user:pw@m1.example/x.fwb".into()
    ));
    assert!(refused(&|e| e.urls[0] = "https://m1.example/a b.fwb".into()));
    assert!(refused(&|e| e.urls[0] = "https:///x.fwb".into()));
    assert!(refused(&|e| e.urls = vec!["https://m.example/x".into(); 9]));
    assert!(refused(
        &|e| e.files = Some("https://m1.example/no-slash".into())
    ));
    assert!(refused(&|e| e.files = Some("file:///tmp/".into())));
    // A catalog may name a bridge above this protocol's.
    assert!(!refused(&|e| e.bridge = BRIDGE_VERSION + 1));
}

// ---- bundle.json ----

#[test]
fn bundle_json_asks_for_manifest_operations_and_declared_capabilities() {
    let manifest = manifest();
    let good = bundle_manifest(&bundle_json()).unwrap();
    assert_eq!(good.validate_for(&decl(), &manifest), Ok(()));
    // The entry defaults to index.html.
    let mut raw = bundle_json();
    raw.as_object_mut().unwrap().remove("entry");
    assert_eq!(bundle_manifest(&raw).unwrap().entry, DEFAULT_BUNDLE_ENTRY);

    let check = |edit: &dyn Fn(&mut Value)| {
        let mut raw = bundle_json();
        edit(&mut raw);
        bundle_manifest(&raw).and_then(|bundle| bundle.validate_for(&decl(), &manifest))
    };
    assert_eq!(
        check(&|b| b["id"] = json!("other")),
        Err(BundleError::DeclarationMismatch("id"))
    );
    assert_eq!(
        check(&|b| b["bridge"] = json!(2)),
        Err(BundleError::UnsupportedBridge(2))
    );
    // An admin operation is not a user operation.
    assert_eq!(
        check(&|b| b["operations"] = json!(["example.poll.list"])),
        Err(BundleError::UndeclaredOperation("example.poll.list".into()))
    );
    assert_eq!(
        check(&|b| b["operations"] = json!(["example.poll.unknown"])),
        Err(BundleError::UndeclaredOperation(
            "example.poll.unknown".into()
        ))
    );
    assert_eq!(
        check(&|b| b["operations"] = json!(["example.poll.pay"])),
        Err(BundleError::TransientOperation("example.poll.pay".into()))
    );
    assert_eq!(
        check(&|b| b["capabilities"] = json!(["camera"])),
        Err(BundleError::UndeclaredCapability("camera".into()))
    );
    // Strict fields and names.
    assert!(check(&|b| b["permissions"] = json!(["network"])).is_err());
    assert!(check(&|b| b["script"] = json!("x")).is_err());
    assert!(check(&|b| b["entry"] = json!("index.htm")).is_err());
    assert!(check(&|b| b["entry"] = json!("../index.html")).is_err());
    assert!(check(&|b| b["locales"] = json!(["en", "EN"])).is_err());
    assert!(check(&|b| b["operations"] = json!(["bad op"])).is_err());
    assert!(
        BundleManifest::from_json(&vec![b' '; MAX_BUNDLE_MANIFEST_BYTES + 1]).is_err(),
        "bundle.json is bounded"
    );
}

// ---- paths and types ----

#[test]
fn paths_and_types_follow_the_allowlists() {
    for path in [
        "index.html",
        "bundle.json",
        "assets/index-4f2a9c.js",
        "assets/fonts/inter.woff2",
        "assets/logo.svg",
        "assets/data/zh-cn.json",
    ] {
        assert_eq!(is_allowed_file(path, "index.html"), Ok(()), "{path}");
    }
    for path in [
        "../index.html",
        "/index.html",
        "assets//a.js",
        "assets/./a.js",
        "assets/../a.js",
        "assets/Index.js",
        "assets/.hidden.js",
        "assets/a b.js",
        "",
    ] {
        assert!(
            matches!(
                is_allowed_file(path, "index.html"),
                Err(BundleError::InvalidPath(_))
            ),
            "{path}"
        );
    }
    for path in [
        "assets/engine.wasm",
        "assets/page.html",
        "assets/run.mjs",
        "assets/readme",
        "assets/photo.jpg",
    ] {
        assert!(
            matches!(
                is_allowed_file(path, "index.html"),
                Err(BundleError::FileType(_))
            ),
            "{path}"
        );
    }
    assert!(!is_bundle_path(&format!("assets/{}.js", "a".repeat(250))));
    assert_eq!(
        content_type("assets/a.js"),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(content_type("assets/a.wasm"), None);
}

// ---- the entry document ----

const VITE_INDEX: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Dashboard</title>
    <!-- a <script>alert(1)</script> in a comment does not run -->
    <script type="module" crossorigin src="./assets/index-4f2a9c.js"></script>
    <link rel="modulepreload" crossorigin href="./assets/vendor-8c1d.js">
    <link rel="stylesheet" crossorigin href="assets/index-77aa.css">
  </head>
  <body>
    <div id="app" data-x='1 < 2'></div>
    <a href="https://help.example.com/">help</a>
  </body>
</html>
"#;

#[test]
fn the_entry_document_loads_only_bundle_scripts() {
    assert_eq!(check_entry_html(VITE_INDEX.as_bytes()), Ok(()));
    let refused = |html: &str| check_entry_html(html.as_bytes()).unwrap_err();
    assert_eq!(
        refused("<script>alert(1)</script>"),
        HtmlViolation::InlineScript
    );
    assert_eq!(
        refused("<SCRIPT TYPE=module>import './a.js'</SCRIPT>"),
        HtmlViolation::InlineScript
    );
    assert_eq!(
        refused(r#"<script src="./a.js">alert(1)</script>"#),
        HtmlViolation::InlineScript
    );
    assert_eq!(
        refused(r#"<script type="importmap">{"imports":{}}</script>"#),
        HtmlViolation::InlineScript
    );
    assert_eq!(
        refused(r#"<svg><script href="./a.js"></script></svg>"#),
        HtmlViolation::InlineScript
    );
    for src in [
        "https://cdn.example.com/vue.js",
        "//cdn.example.com/vue.js",
        "/assets/a.js",
        "\\\\cdn.example.com\\a.js",
        "data:text/javascript,alert(1)",
        "HTTPS://cdn.example.com/a.js",
        "ht&#x74;ps://cdn.example.com/a.js",
    ] {
        assert!(
            matches!(
                refused(&format!(r#"<script src="{src}"></script>"#)),
                HtmlViolation::ExternalScript(_)
            ),
            "{src}"
        );
    }
    assert!(matches!(
        refused(r#"<link rel="stylesheet" href="https://fonts.example.com/a.css">"#),
        HtmlViolation::ExternalLink(_)
    ));
    assert!(matches!(
        refused(r#"<img src="./a.png" onerror="alert(1)">"#),
        HtmlViolation::EventAttribute(_)
    ));
    assert!(matches!(
        refused(r#"<body ONLOAD=go()>"#),
        HtmlViolation::EventAttribute(_)
    ));
    for href in [
        "javascript:alert(1)",
        "JavaScript:alert(1)",
        "java\tscript:alert(1)",
        "&#106;avascript:alert(1)",
        "&#x6A;avascript&colon;alert(1)",
        " vbscript:x",
    ] {
        assert!(
            matches!(
                refused(&format!(r#"<a href="{href}">x</a>"#)),
                HtmlViolation::ScriptUrl(_)
            ),
            "{href}"
        );
    }
    for element in [
        r#"<base href="https://evil.example/">"#,
        r#"<meta http-equiv="refresh" content="0;url=https://evil.example">"#,
        r#"<iframe src="./a.html"></iframe>"#,
        r#"<object data="./a.svg"></object>"#,
        r#"<embed src="./a.svg">"#,
    ] {
        assert!(
            matches!(refused(element), HtmlViolation::ForbiddenElement(_)),
            "{element}"
        );
    }
    // Unterminated constructs are refused rather than guessed at.
    assert_eq!(refused("<script src='./a.js'"), HtmlViolation::Malformed);
    assert_eq!(refused("<!-- open"), HtmlViolation::Malformed);
    assert_eq!(refused("<script src='./a.js'>"), HtmlViolation::Malformed);
    assert_eq!(refused("<div title=\"open>"), HtmlViolation::Malformed);
    assert_eq!(
        check_entry_html(&[0xff, 0xfe]),
        Err(HtmlViolation::Malformed)
    );
}

#[test]
fn scripts_are_scanned_for_dynamic_code() {
    assert_eq!(
        scan_script("const x = eval('1+1')"),
        [BundleWarningCode::Eval]
    );
    assert_eq!(
        scan_script("const f = new Function ('a', 'return a')"),
        [BundleWarningCode::NewFunction]
    );
    assert_eq!(
        scan_script(r#"await import("https://cdn.example.com/x.js")"#),
        [BundleWarningCode::ExternalImport]
    );
    assert_eq!(
        scan_script("import(`//cdn.example.com/x.js`)"),
        [BundleWarningCode::ExternalImport]
    );
    // Lookalikes are not findings.
    assert!(scan_script("retrieval(x); obj.eval(y); import('./chunk.js')").is_empty());
}

// ---- selection with bundles ----

fn dashboard() -> UiContribution {
    manifest()
        .contribution("example.poll.dashboard")
        .unwrap()
        .clone()
}

fn descriptor(
    platform: ClientPlatform,
    distribution: &str,
    engine: &str,
    bridge: u32,
) -> ClientUiDescriptor {
    ClientUiDescriptor::new(
        &engine_vocabulary(),
        platform,
        100,
        distribution,
        Some(BundleRuntime {
            bridge,
            engine: engine.into(),
            engine_major: 120,
        }),
    )
}

fn chosen(bundles: &[WebBundleDecl], descriptor: &ClientUiDescriptor) -> Option<String> {
    dashboard()
        .select_with_bundles(descriptor, bundles)
        .into_contribution()
        .variant
}

#[test]
fn core_selects_a_bundle_variant_only_for_a_usable_bundle() {
    let android = descriptor(ClientPlatform::Android, "play", "webview", 1);
    assert_eq!(chosen(&[decl()], &android).as_deref(), Some("bundle"));
    // Not approved, withdrawn: Core passes no declaration.
    assert_eq!(chosen(&[], &android).as_deref(), Some("basic"));
    // The declaration's bridge is above the host's.
    let mut newer = decl();
    newer.bridge = 2;
    assert_eq!(chosen(&[newer.clone()], &android).as_deref(), Some("basic"));
    assert_eq!(
        chosen(
            &[newer],
            &descriptor(ClientPlatform::Android, "play", "webview", 2)
        )
        .as_deref(),
        Some("bundle")
    );
    // Digital goods never reach a client under App Store rules.
    let mut goods = decl();
    goods.digital_goods = true;
    assert_eq!(
        chosen(&[goods.clone()], &android).as_deref(),
        Some("bundle")
    );
    for apple in [
        descriptor(ClientPlatform::Ios, "appstore", "wkwebview", 1),
        descriptor(ClientPlatform::Flutter, "testflight", "wkwebview", 1),
        descriptor(ClientPlatform::Tauri, "appstore", "webview", 1),
    ] {
        assert_eq!(chosen(&[goods.clone()], &apple).as_deref(), Some("basic"));
        assert!(under_app_store_rules(&apple));
    }
    // Without bundles in the descriptor (L3 off, old WebView) nothing changes.
    let mut off = android.clone();
    off.bundle = None;
    assert_eq!(chosen(&[decl()], &off).as_deref(), Some("basic"));
    // The bundle-unaware selection keeps its behaviour.
    assert_eq!(
        dashboard()
            .select(&android)
            .into_contribution()
            .variant
            .as_deref(),
        Some("bundle")
    );
}

// ---- bridge ----

fn request(raw: Value) -> Vec<u8> {
    serde_json::to_vec(&raw).unwrap()
}

#[test]
fn names_are_the_wire_names() {
    for permission in BridgePermission::ALL {
        assert_eq!(
            serde_json::to_value(permission).unwrap(),
            json!(permission.name())
        );
        assert_eq!(
            BridgePermission::parse(permission.name()),
            Some(*permission)
        );
    }
    assert_eq!(BRIDGE_PERMISSIONS.len(), BridgePermission::ALL.len());
    for method in BridgeMethod::ALL {
        assert_eq!(BridgeMethod::parse(method.name()), Some(*method));
        assert!(method.since() <= BRIDGE_VERSION);
    }
    for event in BridgeEvent::ALL {
        assert_eq!(serde_json::to_value(event).unwrap(), json!(event.name()));
        assert_eq!(BridgeEvent::parse(event.name()), Some(*event));
    }
    let keys: BTreeSet<_> = BridgeErrorCode::ALL
        .iter()
        .map(|c| c.message_key())
        .collect();
    assert_eq!(keys.len(), BridgeErrorCode::ALL.len());
    for code in BridgeErrorCode::ALL {
        assert_eq!(serde_json::to_value(code).unwrap(), json!(code.code()));
    }
}

#[test]
fn envelopes_are_bounded_strict_requests() {
    let good = request(json!({"bridge": 1, "id": "r-17", "type": "request",
        "method": "plugin.query", "params": {"operation": "example.poll.dash", "input": {}}}));
    let envelope = BridgeEnvelope::parse(&good, BRIDGE_VERSION).unwrap();
    assert_eq!(envelope.id.as_deref(), Some("r-17"));
    assert!(matches!(
        envelope.request_body(),
        Ok(BridgeRequest::PluginQuery(PluginQueryParams { .. }))
    ));

    let failed = |raw: Value| BridgeEnvelope::parse(&request(raw), BRIDGE_VERSION).unwrap_err();
    assert_eq!(
        BridgeEnvelope::parse(b"not json", 1).unwrap_err(),
        (None, BridgeErrorCode::InvalidEnvelope)
    );
    assert_eq!(
        BridgeEnvelope::parse(&vec![b' '; MAX_BRIDGE_ENVELOPE_BYTES + 1], 1).unwrap_err(),
        (None, BridgeErrorCode::PayloadTooLarge)
    );
    // The id comes back when it was readable, so the host can answer.
    assert_eq!(
        failed(json!({"bridge": 1, "id": "r-1", "type": "request", "method": "x", "extra": 1})),
        (Some("r-1".into()), BridgeErrorCode::InvalidEnvelope)
    );
    assert_eq!(
        failed(json!({"bridge": 2, "id": "r-1", "type": "request", "method": "ui.close"})),
        (Some("r-1".into()), BridgeErrorCode::UnsupportedBridge)
    );
    assert_eq!(
        failed(json!({"bridge": 0, "id": "r-1", "type": "request", "method": "ui.close"})).1,
        BridgeErrorCode::UnsupportedBridge
    );
    // A bundle sends requests only, with an id and an object of params.
    assert_eq!(
        failed(json!({"bridge": 1, "id": "r-1", "type": "response", "result": {}})).1,
        BridgeErrorCode::InvalidEnvelope
    );
    assert_eq!(
        failed(json!({"bridge": 1, "type": "request", "method": "ui.close"})),
        (None, BridgeErrorCode::InvalidEnvelope)
    );
    assert_eq!(
        failed(json!({"bridge": 1, "id": "r 1", "type": "request", "method": "ui.close"})),
        (None, BridgeErrorCode::InvalidEnvelope)
    );
    assert_eq!(
        failed(
            json!({"bridge": 1, "id": "r-1", "type": "request", "method": "ui.close",
            "params": []})
        )
        .1,
        BridgeErrorCode::InvalidEnvelope
    );
    assert_eq!(
        failed(
            json!({"bridge": 1, "id": "r-1", "type": "request", "method": "ui.close",
            "result": {}})
        )
        .1,
        BridgeErrorCode::InvalidEnvelope
    );

    // Host messages round trip and validate.
    let response = BridgeEnvelope::failure(
        Some("r-1".into()),
        BridgeError::new(BridgeErrorCode::PermissionDenied),
    );
    assert_eq!(
        serde_json::to_value(&response).unwrap(),
        json!({"bridge": 1, "id": "r-1", "type": "response",
            "error": {"code": "permission_denied", "messageKey": "plugin.bridge.permissionDenied"}})
    );
    assert_eq!(response.validate(), Ok(()));
    let event = BridgeEnvelope::event(
        BridgeEvent::ThemeChanged,
        json!({"mode": "dark", "tokens": {}}),
    );
    assert_eq!(event.validate(), Ok(()));
    assert_eq!(
        serde_json::to_value(&event).unwrap()["event"],
        "theme.changed"
    );
    assert_eq!(
        BridgeEnvelope::success("r-2", json!({"data": {}})).validate(),
        Ok(())
    );
    // Only a plain plugin code passes through.
    assert_eq!(
        BridgeError::operation_failed(Some("task.not_found"))
            .plugin_code
            .as_deref(),
        Some("task.not_found")
    );
    assert_eq!(
        BridgeError::operation_failed(Some("db error: relation tasks does not exist")).plugin_code,
        None
    );
}

fn body(method: &str, params: Value) -> Result<BridgeRequest, BridgeErrorCode> {
    let raw = request(json!({"bridge": 1, "id": "r-1", "type": "request",
        "method": method, "params": params}));
    BridgeEnvelope::parse(&raw, BRIDGE_VERSION)
        .map_err(|(_, code)| code)?
        .request_body()
}

#[test]
fn parameters_are_read_strictly_and_bounded() {
    assert_eq!(
        body("plugin.delete", json!({})),
        Err(BridgeErrorCode::MethodNotFound)
    );
    let big = "x".repeat(16 * 1024);
    assert_eq!(
        body(
            "plugin.query",
            json!({"operation": "example.poll.dash", "input": {"q": big}})
        ),
        Err(BridgeErrorCode::PayloadTooLarge)
    );
    assert_eq!(
        body(
            "plugin.query",
            json!({"operation": "example.poll.dash", "input": []})
        ),
        Err(BridgeErrorCode::InvalidParams)
    );
    assert_eq!(
        body(
            "plugin.query",
            json!({"operation": "example.poll.dash", "token": "x"})
        ),
        Err(BridgeErrorCode::InvalidParams)
    );
    assert!(body("plugin.query", json!({"operation": "example.poll.dash"})).is_ok());
    assert!(
        body(
            "plugin.command",
            json!({"operation": "example.poll.vote",
        "commandId": "c-1"})
        )
        .is_ok()
    );
    assert_eq!(
        body(
            "plugin.command",
            json!({"operation": "example.poll.vote", "commandId": "c 1"})
        ),
        Err(BridgeErrorCode::InvalidParams)
    );
    // ui.navigate: a page or a route of this bundle, not both.
    assert!(body("ui.navigate", json!({"page": "example.poll.list"})).is_ok());
    assert!(body("ui.navigate", json!({"bundleRoute": "/tasks/1"})).is_ok());
    assert!(body("ui.navigate", json!({})).is_err());
    assert!(body("ui.navigate", json!({"page": "p", "bundleRoute": "/"})).is_err());
    assert!(body("ui.navigate", json!({"bundleRoute": "/../x"})).is_err());
    // ui.openExternal: https only (D-13).
    assert!(
        body(
            "ui.openExternal",
            json!({"url": "https://help.example.com/a"})
        )
        .is_ok()
    );
    for url in [
        "http://help.example.com/",
        "javascript:alert(1)",
        "https://user@help.example.com/",
        "flare://open",
    ] {
        assert_eq!(
            body("ui.openExternal", json!({"url": url})),
            Err(BridgeErrorCode::InvalidParams),
            "{url}"
        );
    }
    assert!(body("ui.resize", json!({"height": 0})).is_err());
    assert!(body("ui.resize", json!({"height": MAX_BRIDGE_RESIZE_HEIGHT + 1})).is_err());
    assert!(
        body(
            "ui.toast",
            json!({"message": "x".repeat(MAX_BRIDGE_MESSAGE_BYTES + 1)})
        )
        .is_err()
    );
    assert!(body("ui.setTitle", json!({"title": "a\u{0007}b"})).is_err());
    let users: Vec<String> = (0..=MAX_BRIDGE_PEOPLE).map(|i| format!("u{i}")).collect();
    assert!(body("people.resolve", json!({"userIds": users})).is_err());
    assert!(body("people.resolve", json!({"userIds": []})).is_err());
    assert!(
        body(
            "storage.set",
            json!({"key": "draft", "value": "x".repeat(MAX_BRIDGE_STORAGE_VALUE_BYTES)})
        )
        .is_err()
    );
    assert!(body("storage.set", json!({"key": "draft", "value": null})).is_err());
    assert!(body("storage.set", json!({"key": "a b", "value": 1})).is_err());
    assert!(body("storage.get", json!({"key": "draft"})).is_ok());
    assert!(body("events.subscribe", json!({"topics": ["theme.changed"]})).is_ok());
    assert!(body("events.subscribe", json!({"topics": ["network.changed"]})).is_err());
    assert!(body("host.capability", json!({"name": "selectImage"})).is_ok());
    assert!(body("host.capability", json!({"name": "shell"})).is_err());
    assert!(
        body(
            "diag.log",
            json!({"level": "info", "message": "x".repeat(MAX_BRIDGE_DIAG_BYTES + 1)})
        )
        .is_err()
    );
}

/// The catalog plugin of the fixture: the bundle approved for query,
/// command and storage, `pay` transient, `dash` a query.
fn catalog_plugin(approved: &[&str]) -> ClientPlugin {
    serde_json::from_value(json!({
        "pluginId": "example.poll", "version": "0.2.0", "manifestDigest": "sha256:00",
        "grantRevision": 3,
        "extensions": {
            "platforms": ["android"], "contributions": [], "nativeCapabilities": ["selectImage", "camera"],
            "webBundles": [{"id": "dashboard", "sha256": DIGEST, "size": 4096, "bridge": 1,
                "urls": ["https://m1.example/x.fwb"], "permissions": approved}]
        },
        "operations": ["example.poll.dash", "example.poll.vote", "example.poll.pay"],
        "queryOperations": ["example.poll.dash"],
        "transientOperations": ["example.poll.pay"]
    }))
    .unwrap()
}

fn grant(bundle: Value, approved: &[&str], host: &[&str]) -> BridgeGrant {
    let plugin = catalog_plugin(approved);
    let bundle = bundle_manifest(&bundle).unwrap();
    let host: BTreeSet<String> = host.iter().map(|n| n.to_string()).collect();
    BridgeGrant::new(
        &bundle,
        plugin.web_bundle("dashboard").unwrap(),
        &plugin,
        &host,
    )
}

fn allowed(grant: &BridgeGrant, method: &str, params: Value) -> Result<(), BridgeErrorCode> {
    grant.authorize(&body(method, params).unwrap())
}

#[test]
fn the_grant_is_the_intersection_and_every_overreach_is_denied() {
    let mut raw = bundle_json();
    raw["operations"] = json!([
        "example.poll.dash",
        "example.poll.vote",
        "example.poll.close"
    ]);
    raw["capabilities"] = json!(["selectImage"]);
    let full = grant(
        raw.clone(),
        &["plugin.query", "plugin.command", "storage", "clipboard"],
        &["selectImage", "camera"],
    );
    assert_eq!(
        full.permissions,
        BTreeSet::from([
            BridgePermission::PluginQuery,
            BridgePermission::PluginCommand,
            BridgePermission::Storage
        ]),
        "clipboard is approved but not asked for; ui.navigate asked but not approved"
    );
    // close is not in the catalog's operations.
    assert_eq!(
        full.operations,
        BTreeSet::from(["example.poll.dash", "example.poll.vote"].map(String::from))
    );
    assert_eq!(
        full.query_operations,
        BTreeSet::from(["example.poll.dash".to_string()])
    );

    let ok = Ok(());
    let denied = Err(BridgeErrorCode::PermissionDenied);
    assert_eq!(
        allowed(
            &full,
            "plugin.query",
            json!({"operation": "example.poll.dash"})
        ),
        ok
    );
    assert_eq!(
        allowed(
            &full,
            "plugin.command",
            json!({"operation": "example.poll.vote"})
        ),
        ok
    );
    assert_eq!(allowed(&full, "storage.get", json!({"key": "k"})), ok);
    assert_eq!(allowed(&full, "ui.toast", json!({"message": "hi"})), ok);
    assert_eq!(
        allowed(&full, "host.capability", json!({"name": "selectImage"})),
        ok
    );
    // Not in bundle.json.
    assert_eq!(
        allowed(
            &full,
            "plugin.command",
            json!({"operation": "example.poll.create"})
        ),
        denied
    );
    // In bundle.json, not authorized by the catalog.
    assert_eq!(
        allowed(
            &full,
            "plugin.command",
            json!({"operation": "example.poll.close"})
        ),
        denied
    );
    // A command through plugin.query.
    assert_eq!(
        allowed(
            &full,
            "plugin.query",
            json!({"operation": "example.poll.vote"})
        ),
        denied
    );
    // Asked for, not approved by the tenant.
    assert_eq!(allowed(&full, "ui.navigate", json!({"page": "p"})), denied);
    // Approved, not asked for.
    assert_eq!(
        allowed(&full, "clipboard.write", json!({"text": "x"})),
        denied
    );
    // Never asked for or approved.
    assert_eq!(
        allowed(&full, "people.resolve", json!({"userIds": ["u1"]})),
        denied
    );
    assert_eq!(
        allowed(&full, "media.read", json!({"fileId": "f1"})),
        denied
    );
    assert_eq!(
        allowed(
            &full,
            "ui.openExternal",
            json!({"url": "https://a.example/"})
        ),
        denied
    );
    // A capability the plugin allows but the bundle did not list.
    assert_eq!(
        allowed(&full, "host.capability", json!({"name": "camera"})),
        denied
    );

    // A transient operation never reaches plugin.command, even when the
    // bundle lists it (the platform refuses such a bundle; the grant does
    // not rely on that).
    let mut transient = raw.clone();
    transient["operations"] = json!(["example.poll.pay"]);
    let paid = grant(transient, &["plugin.command"], &[]);
    assert!(paid.operations.is_empty());
    assert_eq!(
        allowed(
            &paid,
            "plugin.command",
            json!({"operation": "example.poll.pay"})
        ),
        denied
    );
    // A capability the host does not offer.
    let bare = grant(raw.clone(), &["plugin.query"], &[]);
    assert!(bare.capabilities.is_empty());
    assert_eq!(
        allowed(&bare, "host.capability", json!({"name": "selectImage"})),
        denied
    );
    // Nothing approved: nothing but the permission-free methods.
    let none = grant(raw, &[], &["selectImage"]);
    assert!(none.permissions.is_empty());
    assert_eq!(
        allowed(
            &none,
            "plugin.query",
            json!({"operation": "example.poll.dash"})
        ),
        denied
    );
    assert_eq!(allowed(&none, "ui.close", json!({})), ok);
    assert_eq!(
        allowed(
            &none,
            "events.subscribe",
            json!({"topics": ["locale.changed"]})
        ),
        ok
    );
}

#[test]
fn a_catalog_plugin_reads_its_bundle_entry() {
    let plugin = catalog_plugin(&["plugin.query"]);
    let entry = plugin.web_bundle("dashboard").unwrap();
    assert_eq!(entry.validate_catalog(), Ok(()));
    assert_eq!(
        entry.permissions,
        BTreeSet::from([BridgePermission::PluginQuery])
    );
    assert!(plugin.web_bundle("other").is_none());
    assert_eq!(NativeCapability::SelectImage.name(), "selectImage");
}
