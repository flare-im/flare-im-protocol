//! Client hot delivery: the UI vocabulary and the client descriptor, the
//! contribution variants Core selects with it, the upgrade placeholder, and
//! the tolerant reading of a catalog contribution.
use flare_plugin_protocol::{
    PluginManifest,
    manifest::{ClientPlatform, UiContribution, UiContributionKind},
    ui::{lenient::Prepared, *},
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}
fn contribution(raw: Value) -> UiContribution {
    serde_json::from_value(raw).unwrap()
}
fn bundle_runtime() -> BundleRuntime {
    BundleRuntime {
        bridge: 1,
        engine: "webview".into(),
        engine_major: 128,
    }
}
/// An Android client of build `build` rendering with `vocabulary`.
fn client(vocabulary: &UiVocabulary, build: u64, bundle: bool) -> ClientUiDescriptor {
    ClientUiDescriptor::new(
        vocabulary,
        ClientPlatform::Android,
        build,
        "play",
        bundle.then(bundle_runtime),
    )
}
fn without_blocks(mut vocabulary: UiVocabulary, blocks: &[&str]) -> UiVocabulary {
    for block in blocks {
        vocabulary.blocks.remove(*block);
    }
    vocabulary
}

// ---- vocabulary and descriptor ----

#[test]
fn the_engine_vocabulary_is_read_from_the_enums() {
    let engine = engine_vocabulary();
    assert_eq!(engine.version, UI_VOCABULARY_VERSION);
    assert_eq!(engine.blocks.len(), UiBlock::KINDS.len());
    for block in [
        "text",
        "protected-media",
        "section",
        "stats",
        "chart",
        "image",
        "qr",
        "hero",
        "steps",
        "banner",
        "timeline",
    ] {
        assert!(engine.blocks.contains(block), "{block}");
    }
    assert_eq!(engine.inputs.len(), UiInputKind::ALL.len());
    assert!(
        engine
            .inputs
            .is_superset(&set(&["money", "image", "document", "file"]))
    );
    assert_eq!(
        engine.formats,
        set(&[
            "money",
            "enum",
            "person",
            "reference",
            "time",
            "percent",
            "duration",
            "count",
            "bytes"
        ])
    );
    assert_eq!(engine.actions, set(&["operation", "page", "native"]));
    assert_eq!(engine.native.len(), NativeCapability::ALL.len());
    assert!(engine.native.contains("documentCapture"));
    assert!(
        engine.renderers.is_empty(),
        "the engine renders no kit component"
    );
    assert_eq!(engine.kinds.len(), UiContributionKind::ALL.len());
    // Version 1 is part of every later vocabulary.
    let base = base_vocabulary();
    assert_eq!(base.version, 1);
    assert!(engine.contains(&base));
    assert!(!base.contains(&engine));
    engine.validate().unwrap();
}

#[test]
fn the_descriptor_is_the_engine_and_host_intersection() {
    let engine = engine_vocabulary();
    let mut kit = without_blocks(engine.clone(), &["chart"]);
    kit.renderers = set(&["redPacket.card"]);
    kit.native = set(&["camera", "documentCapture", "rtcSession"]);
    kit.blocks.insert("futureBlock".into());
    let allowlist: BTreeSet<_> = [NativeCapability::Camera, NativeCapability::DocumentCapture]
        .into_iter()
        .collect();
    let offered = engine.for_host(Some(&kit), &allowlist);
    assert!(!offered.blocks.contains("chart"), "the kit cannot draw it");
    assert!(
        !offered.blocks.contains("futureBlock"),
        "the engine cannot read it"
    );
    assert_eq!(offered.renderers, set(&["redPacket.card"]));
    assert_eq!(offered.native, set(&["camera", "documentCapture"]));
    // Without a host profile nothing the host did not offer is reported.
    let bare = engine.for_host(None, &allowlist);
    assert!(bare.native.is_empty());
    assert!(bare.renderers.is_empty());
    assert_eq!(bare.blocks, engine.blocks);
}

#[test]
fn the_descriptor_hash_is_canonical() {
    let raw = json!({
        "vocabulary": 2, "platform": "android", "appBuild": 41020, "distribution": "play",
        "blocks": ["text", "stats", "chart"], "inputs": ["text"], "kinds": ["page"],
        "bundle": {"bridge": 1, "engine": "webview", "engineMajor": 128}
    });
    let mut reordered = raw.clone();
    reordered["blocks"] = json!(["chart", "text", "stats", "text"]);
    reordered["futureField"] = json!({"anything": true});
    let a: ClientUiDescriptor = serde_json::from_value(raw).unwrap();
    let b: ClientUiDescriptor = serde_json::from_value(reordered).unwrap();
    a.validate().unwrap();
    assert_eq!(a.hash(), b.hash());
    assert!(a.hash().starts_with("sha256:"));
    assert_eq!(a.hash().len(), "sha256:".len() + 64);
    let mut newer = a.clone();
    newer.app_build += 1;
    assert_ne!(a.hash(), newer.hash());
}

#[test]
fn descriptor_bounds() {
    let valid = client(&engine_vocabulary(), 1, true);
    valid.validate().unwrap();
    let refused = |edit: &dyn Fn(&mut ClientUiDescriptor)| {
        let mut descriptor = valid.clone();
        edit(&mut descriptor);
        descriptor.validate().is_err()
    };
    assert!(refused(&|d| d.vocabulary = 0));
    assert!(refused(&|d| d.distribution = "Play Store".into()));
    assert!(refused(&|d| d.distribution = String::new()));
    assert!(refused(&|d| {
        d.blocks.insert("bad name".into());
    }));
    assert!(refused(&|d| {
        d.blocks.insert("x".repeat(65));
    }));
    assert!(refused(&|d| d.bundle.as_mut().unwrap().bridge = 0));
    assert!(refused(
        &|d| d.bundle.as_mut().unwrap().engine = "Web View".into()
    ));
    assert!(refused(&|d| {
        d.inputs = (0..129).map(|i| format!("input{i}")).collect();
    }));
    // Within every per-set bound, still over 4 KiB.
    assert!(refused(&|d| {
        d.renderers = (0..100).map(|i| format!("{i:0>60}")).collect();
    }));
}

// ---- variants and selection ----

/// The task dashboard of the plan: web bundle, then charts, then rows.
fn dashboard(min_client: Option<Value>) -> UiContribution {
    let mut raw = json!({
        "kind": "page", "id": "example.poll.dashboard", "schemaVersion": 1,
        "requiredPermissions": ["example.poll.read"], "icon": "chart", "order": 7,
        "schema": {"title": "看板", "translations": {"en-US": {"title": "Dashboard"}}},
        "variants": [
            {"id": "bundle", "requires": {"bundle": {"id": "dashboard", "bridge": 1}},
             "bundle": {"id": "dashboard", "route": "/"}},
            {"id": "charts", "requires": {"blocks": ["stats", "chart", "section"]},
             "schema": {"title": "看板", "blocks": [
                {"kind": "section", "label": "本周", "blocks": [
                    {"kind": "stats", "items": [
                        {"label": "完成", "valuePath": "/done"},
                        {"label": "进行中", "valuePath": "/open"}]},
                    {"kind": "chart", "chartKind": "bar", "seriesPath": "/days",
                     "labelPath": "/day", "valuePath": "/count"}]}]},
             "source": {"operation": "example.poll.dash"}},
            {"id": "basic",
             "schema": {"title": "看板", "blocks": [
                {"kind": "rows", "source": "example.poll.mine", "path": "/items",
                 "keyPath": "/id", "columns": [{"label": "任务", "path": "/title"}]}]},
             "source": {"operation": "example.poll.mine"}}
        ]
    });
    if let Some(min_client) = min_client {
        raw["minClient"] = min_client;
    }
    contribution(raw)
}

#[test]
fn core_selects_the_first_variant_the_client_satisfies() {
    let page = dashboard(None);
    let engine = engine_vocabulary();
    let cases = [
        (client(&engine, 1, true), "bundle", "variant"),
        (client(&engine, 1, false), "charts", "fallback"),
        (
            client(&without_blocks(engine.clone(), &["chart"]), 1, false),
            "basic",
            "fallback",
        ),
        (client(&base_vocabulary(), 1, false), "basic", "fallback"),
    ];
    for (descriptor, variant, outcome) in cases {
        let selected = page.select(&descriptor);
        assert_eq!(selected.outcome(), outcome, "{variant}");
        let chosen = selected.contribution();
        assert_eq!(chosen.variant.as_deref(), Some(variant));
        assert!(chosen.variants.is_empty() && chosen.min_client.is_empty());
        assert!(chosen.upgrade.is_none());
        // Identity is the base contribution's whichever variant renders.
        assert_eq!(chosen.id, page.id);
        assert_eq!(chosen.required_permissions, page.required_permissions);
        assert_eq!((chosen.icon.as_deref(), chosen.order), (Some("chart"), 7));
    }
    let bundle = page.select(&client(&engine, 1, true)).into_contribution();
    assert_eq!(
        bundle.bundle,
        Some(UiBundleRef {
            id: "dashboard".into(),
            route: "/".into()
        })
    );
    let charts = page.select(&client(&engine, 1, false)).into_contribution();
    assert!(charts.bundle.is_none(), "a bundle is never inherited");
    assert_eq!(charts.source.unwrap().operation, "example.poll.dash");
}

#[test]
fn nothing_fits_or_min_client_gives_an_upgrade_placeholder() {
    // Without the rows block not even the basic variant reads.
    let old = client(&without_blocks(base_vocabulary(), &["rows"]), 1, false);
    let Selected::Upgrade(placeholder) = dashboard(None).select(&old) else {
        panic!("expected an upgrade placeholder");
    };
    assert_eq!(placeholder.upgrade, Some(UiUpgrade { min_build: None }));
    // minClient blocks older builds whatever they can render.
    let floored = dashboard(Some(json!({"android": 41020, "ios": 50000})));
    let engine = engine_vocabulary();
    let selected = floored.select(&client(&engine, 41019, true));
    assert_eq!(selected.outcome(), "upgrade");
    let placeholder = selected.into_contribution();
    assert_eq!(placeholder.upgrade.unwrap().min_build, Some(41020));
    assert_eq!(placeholder.schema.title, "看板");
    assert_eq!(placeholder.schema.translations["en-US"].title, "Dashboard");
    assert!(placeholder.schema.blocks.is_empty() && placeholder.schema.actions.is_empty());
    assert!(placeholder.source.is_none() && placeholder.bundle.is_none());
    assert_eq!(placeholder.required_permissions, ["example.poll.read"]);
    placeholder.schema.validate().unwrap();
    // At the floor the variants apply; nothing fitting then names the floor.
    assert_eq!(
        floored.select(&client(&engine, 41020, true)).outcome(),
        "variant"
    );
    let Selected::Upgrade(placeholder) = floored.select(&client(
        &without_blocks(base_vocabulary(), &["rows"]),
        41020,
        false,
    )) else {
        panic!("expected an upgrade placeholder");
    };
    assert_eq!(placeholder.upgrade.unwrap().min_build, Some(41020));
    // Another platform has its own floor, or none.
    let mut web = client(&engine, 1, true);
    web.platform = ClientPlatform::Web;
    assert_eq!(floored.select(&web).outcome(), "variant");
}

#[test]
fn a_variant_cannot_override_identity() {
    for field in [
        json!({"kind": "page"}),
        json!({"requiredPermissions": []}),
        json!({"entityPath": "/id"}),
        json!({"entityLifecycle": {"unbound": "example.poll.close"}}),
        json!({"forwardable": true}),
        json!({"audience": {}}),
        json!({"order": 1}),
        json!({"icon": "poll"}),
        json!({"minClient": {"web": 1}}),
    ] {
        let mut variant = json!({"id": "v"});
        for (key, value) in field.as_object().unwrap() {
            variant[key] = value.clone();
        }
        assert!(
            serde_json::from_value::<UiContributionVariant>(variant.clone()).is_err(),
            "{variant}"
        );
    }
}

#[test]
fn a_variant_needing_a_native_capability_the_client_lacks_is_skipped() {
    let photo = json!({"title": "t", "actions": [
        {"id": "photo", "label": "拍照", "target": {"kind": "native", "capability": "camera"}},
        {"id": "vote", "label": "投票",
         "target": {"kind": "operation", "operation": "example.poll.vote"}}],
        "translations": {"en": {"title": "t", "actions": {"photo": "Photo", "vote": "Vote"}}}});
    let plain = json!({"title": "t", "actions": [
        {"id": "vote", "label": "投票",
         "target": {"kind": "operation", "operation": "example.poll.vote"}}]});
    let card = contribution(json!({
        "kind": "message-content", "id": "example.poll.card", "schemaVersion": 1,
        "requiredPermissions": [], "schema": photo,
        "variants": [{"id": "photo"}, {"id": "plain", "schema": plain}]
    }));
    let mut vocabulary = engine_vocabulary();
    vocabulary.native.clear();
    // Without the camera the photo variant is skipped, not stripped.
    let selected = card.select(&client(&vocabulary, 1, false));
    assert_eq!(selected.outcome(), "fallback");
    let chosen = selected.into_contribution();
    assert_eq!(chosen.variant.as_deref(), Some("plain"));
    assert_eq!(chosen.schema.actions.len(), 1);
    // With it, the first variant comes whole, its native action included.
    vocabulary.native.insert("camera".into());
    let kept = card.select(&client(&vocabulary, 1, false));
    assert_eq!(kept.outcome(), "variant");
    let kept = kept.into_contribution();
    assert_eq!(kept.schema.actions.len(), 2);
    assert!(kept.schema.translations["en"].actions.contains_key("photo"));
    // A contribution whose only presentation needs it gets a placeholder.
    let only = contribution(json!({
        "kind": "message-content", "id": "example.poll.card", "schemaVersion": 1,
        "requiredPermissions": [], "schema": photo
    }));
    vocabulary.native.clear();
    assert_eq!(
        only.select(&client(&vocabulary, 1, false)).outcome(),
        "upgrade"
    );
    vocabulary.native.insert("camera".into());
    let whole = only
        .select(&client(&vocabulary, 1, false))
        .into_contribution();
    // Without variants the contribution itself is selected, unnamed.
    assert_eq!(whole.variant, None);
    assert_eq!(whole.schema.actions.len(), 2);
}

#[test]
fn derived_requires_name_every_kind_used() {
    let page = contribution(json!({
        "kind": "page", "id": "example.poll.all", "schemaVersion": 1, "requiredPermissions": [],
        "schema": {
            "title": "t",
            "presentation": {"renderer": "flare.qr"},
            "fields": [{"label": "比例", "path": "/p", "format": {"kind": "percent"}}],
            "blocks": [
                {"kind": "section", "blocks": [
                    {"kind": "chart", "chartKind": "line", "seriesPath": "/s", "labelPath": "/l",
                     "valuePath": "/v", "format": {"kind": "count"}},
                    {"kind": "fields", "fields": [{"label": "大小", "path": "/b",
                                                   "format": {"kind": "bytes"}}]}]},
                {"kind": "stats", "items": [
                    {"label": "a", "valuePath": "/a", "format": {"kind": "duration"}},
                    {"label": "b", "valuePath": "/b"}]},
                {"kind": "hero", "amountPath": "/x", "format": {"kind": "money", "scale": 2}},
                {"kind": "rows", "source": "example.poll.list", "path": "/r", "keyPath": "/k",
                 "columns": [{"label": "c", "path": "/c", "valueFormat": {"kind": "time"}}]}
            ],
            "inputs": [
                {"id": "amount", "kind": "money", "label": "金额", "money": {"currency": "CNY"}},
                {"id": "note", "kind": "text", "label": "备注"}
            ],
            "actions": [
                {"id": "pay", "label": "付", "form": ["amount", "note"],
                 "target": {"kind": "operation", "operation": "example.poll.pay"}},
                {"id": "scan", "label": "扫", "target": {"kind": "native", "capability": "camera"}}
            ]
        }
    }));
    let requires = page.derived_requires();
    assert_eq!(
        requires.blocks,
        set(&["section", "chart", "fields", "stats", "hero", "rows"])
    );
    assert_eq!(
        requires.formats,
        set(&["percent", "count", "bytes", "duration", "money", "time"])
    );
    assert_eq!(requires.inputs, set(&["money", "text"]));
    assert_eq!(requires.actions, set(&["operation", "native"]));
    assert_eq!(requires.renderers, set(&["flare.qr"]));
    assert_eq!(requires.kinds, set(&["page"]));
    assert_eq!(requires.native, set(&["camera"]));
    assert!(requires.bundle.is_none());
    // Beyond version 1, only the new names are left to write.
    let beyond = requires.beyond(&base_vocabulary());
    assert_eq!(beyond.blocks, set(&["section", "chart", "stats", "hero"]));
    assert_eq!(
        beyond.formats,
        set(&["percent", "count", "bytes", "duration"])
    );
    assert_eq!(beyond.inputs, set(&["money"]));
    assert!(beyond.actions.is_empty() && beyond.kinds.is_empty());
    assert_eq!(beyond.renderers, set(&["flare.qr"]));
    assert!(beyond.native.is_empty(), "camera is version 1");
}

// ---- publication ----

fn manifest_value() -> Value {
    let mut manifest: Value =
        serde_json::from_slice(include_bytes!("fixtures/plugin-ui-v2-manifest.json")).unwrap();
    let operations = manifest["capabilities"][0]["operations"]
        .as_array_mut()
        .unwrap();
    for id in ["dash", "mine"] {
        operations.push(json!({
            "id": id, "idempotent": true, "inputSchema": {"type": "object"},
            "outputSchema": {"type": "object"}, "permissionIds": ["example.poll.read"],
            "timeoutMs": 5000, "mode": "query"
        }));
    }
    let page = serde_json::to_value(dashboard(None)).unwrap();
    manifest["clientExtensions"]["contributions"]
        .as_array_mut()
        .unwrap()
        .push(page);
    manifest
}
fn parse(manifest: &Value) -> Result<PluginManifest, flare_plugin_protocol::ProtocolError> {
    PluginManifest::from_json(&serde_json::to_vec(manifest).unwrap())
}
fn dashboard_raw(manifest: &mut Value) -> &mut Value {
    manifest["clientExtensions"]["contributions"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()
}
fn rejected(edit: &dyn Fn(&mut Value)) -> bool {
    let mut manifest = manifest_value();
    edit(dashboard_raw(&mut manifest));
    parse(&manifest).is_err()
}

#[test]
fn a_manifest_with_variants_publishes_and_lists_every_variant_operation() {
    let manifest = parse(&manifest_value()).unwrap();
    let refs = manifest.client_operation_refs();
    assert!(refs.contains("example.poll.dash") && refs.contains("example.poll.mine"));
    let page = manifest.contribution("example.poll.dashboard").unwrap();
    assert_eq!(page.variants.len(), 3);
    for variant in &page.variants {
        assert!(variant.missing_requires(page).is_empty(), "{}", variant.id);
    }
    // The round trip keeps variants and drops nothing.
    let raw = serde_json::to_value(page).unwrap();
    assert_eq!(raw["variants"][0]["bundle"]["route"], "/");
    assert!(raw.get("variant").is_none() && raw.get("upgrade").is_none());
}

#[test]
fn publication_refuses_a_variant_that_states_less_than_it_uses() {
    // The charts variant uses stats, chart and section.
    assert!(rejected(&|page| {
        page["variants"][1]["requires"]["blocks"] = json!(["stats", "chart"]);
    }));
    assert!(rejected(&|page| {
        page["variants"][1]
            .as_object_mut()
            .unwrap()
            .remove("requires");
    }));
    // Stating more than it uses is the author's choice.
    assert!(!rejected(&|page| {
        page["variants"][1]["requires"]["native"] = json!(["documentCapture"]);
    }));
    // A native capability beyond version 1 that a variant's action uses
    // must be stated.
    let capture = |page: &mut Value| {
        let schema = &mut page["variants"][1]["schema"];
        schema["inputs"] = json!([{"id": "front", "kind": "document", "label": "人像面",
            "side": "front", "file": {"accept": ["image/jpeg"], "maxBytes": 1024, "upload": {
                "prepareOperation": "example.poll.dash", "completeOperation": "example.poll.mine",
                "purpose": "kyc"}}}]);
        schema["actions"] = json!([{"id": "scan", "label": "拍证件", "target": {
            "kind": "native", "capability": "documentCapture",
            "params": {"side": "front"}, "into": "front"}}]);
        page["variants"][1]["requires"]["inputs"] = json!(["document"]);
    };
    let derived = {
        let mut manifest = manifest_value();
        capture(dashboard_raw(&mut manifest));
        let page: UiContribution =
            serde_json::from_value(dashboard_raw(&mut manifest).clone()).unwrap();
        page.variants[1].missing_requires(&page)
    };
    assert_eq!(derived.native, set(&["documentCapture"]));
    assert!(derived.blocks.is_empty() && derived.inputs.is_empty());
    let mut stated = manifest_value();
    capture(dashboard_raw(&mut stated));
    dashboard_raw(&mut stated)["variants"][1]["requires"]["native"] = json!(["documentCapture"]);
    let stated =
        serde_json::from_value::<UiContribution>(dashboard_raw(&mut stated).clone()).unwrap();
    assert!(stated.variants[1].missing_requires(&stated).is_empty());
    // Version 1 names need not be written: the basic variant states nothing.
    assert!(!rejected(&|_| {}));
    // A bundle variant must state its bundle.
    assert!(rejected(&|page| {
        page["variants"][0]
            .as_object_mut()
            .unwrap()
            .remove("requires");
    }));
}

#[test]
fn publication_refuses_bad_variants() {
    // The last variant is declarative: a bundle never stands alone.
    assert!(rejected(&|page| {
        let variants = page["variants"].as_array_mut().unwrap();
        variants.truncate(1);
    }));
    // A bundle is a variant's, and `variant` and `upgrade` are Core's.
    assert!(rejected(
        &|page| page["bundle"] = json!({"id": "dashboard", "route": "/"})
    ));
    assert!(rejected(&|page| page["variant"] = json!("basic")));
    assert!(rejected(&|page| page["upgrade"] = json!({})));
    // Unique, valid ids.
    assert!(rejected(&|page| page["variants"][2]["id"] = json!("charts")));
    assert!(rejected(&|page| page["variants"][2]["id"] = json!("bad id")));
    // A bundle route stays inside the bundle.
    assert!(rejected(
        &|page| page["variants"][0]["bundle"]["route"] = json!("/../x")
    ));
    assert!(rejected(
        &|page| page["variants"][0]["bundle"]["route"] = json!("x")
    ));
    // requires.bundle names the variant's own bundle.
    assert!(rejected(&|page| {
        page["variants"][0]["requires"]["bundle"]["id"] = json!("other");
    }));
    assert!(rejected(&|page| {
        page["variants"][2]["requires"] = json!({"bundle": {"id": "dashboard", "bridge": 1}});
    }));
    // Each variant is checked as a contribution: its operations are declared.
    assert!(rejected(&|page| {
        page["variants"][2]["source"]["operation"] = json!("example.poll.unknown");
    }));
    // At most 8 variants.
    assert!(rejected(&|page| {
        let basic = page["variants"][2].clone();
        let variants = page["variants"].as_array_mut().unwrap();
        for index in 0..7 {
            let mut copy = basic.clone();
            copy["id"] = json!(format!("basic{index}"));
            variants.insert(1, copy);
        }
    }));
    // minClient builds fit an i64.
    assert!(rejected(
        &|page| page["minClient"] = json!({"android": u64::MAX})
    ));
    assert!(!rejected(
        &|page| page["minClient"] = json!({"android": 41020})
    ));
}

// ---- tolerant reading ----

fn card(blocks: Value, extra: Value) -> Value {
    let mut raw = json!({
        "kind": "message-content", "id": "example.poll.card", "schemaVersion": 1,
        "requiredPermissions": [],
        "schema": {"title": "投票", "blocks": blocks,
                   "translations": {"en-US": {"title": "Poll"}}}
    });
    for (key, value) in extra.as_object().unwrap() {
        raw["schema"][key] = value.clone();
    }
    raw
}
fn ready(prepared: Prepared) -> UiContribution {
    match prepared {
        Prepared::Ready(contribution) => contribution,
        other => panic!("expected a readable contribution, got {other:?}"),
    }
}
fn block_kinds(contribution: &UiContribution) -> Vec<&'static str> {
    contribution
        .schema
        .blocks
        .iter()
        .map(UiBlock::kind_name)
        .collect()
}

#[test]
fn an_unknown_field_makes_a_known_block_unknown() {
    // `visibleWhen` is a condition this client does not know: ignoring it
    // could show what it hides, so the whole block goes.
    let raw = card(
        json!([
            {"kind": "text", "text": "公开"},
            {"kind": "note", "text": "仅发起人可见", "visibleWhen": [{"path": "/x", "exists": true}]},
            {"kind": "futureBlock", "anything": 1}
        ]),
        json!({}),
    );
    let contribution = ready(lenient::prepare_contribution(&raw, &engine_vocabulary()));
    assert_eq!(block_kinds(&contribution), ["text"]);
    contribution.schema.validate().unwrap();
}

#[test]
fn a_required_block_the_client_cannot_read_needs_an_upgrade() {
    let raw = card(
        json!([{"kind": "text", "text": "x"}, {"kind": "futureBlock", "required": true}]),
        json!({}),
    );
    let Prepared::Upgrade(placeholder) = lenient::prepare_contribution(&raw, &engine_vocabulary())
    else {
        panic!("expected an upgrade placeholder");
    };
    assert_eq!(placeholder.id, "example.poll.card");
    assert_eq!(placeholder.kind, UiContributionKind::MessageContent);
    assert_eq!(placeholder.schema.title, "投票");
    assert_eq!(placeholder.schema.translations["en-US"].title, "Poll");
    assert!(placeholder.schema.blocks.is_empty());
    assert_eq!(placeholder.upgrade, Some(UiUpgrade { min_build: None }));
    // A required block known to the protocol but not to this client's kit.
    let stats = card(
        json!([{"kind": "stats", "required": true, "items": [
            {"label": "a", "valuePath": "/a"}, {"label": "b", "valuePath": "/b"}]}]),
        json!({}),
    );
    let old = base_vocabulary();
    assert!(matches!(
        lenient::prepare_contribution(&stats, &old),
        Prepared::Upgrade(_)
    ));
    // Not required: it just goes.
    let optional = card(
        json!([{"kind": "stats", "items": [
            {"label": "a", "valuePath": "/a"}, {"label": "b", "valuePath": "/b"}]},
            {"kind": "text", "text": "x"}]),
        json!({}),
    );
    assert_eq!(
        block_kinds(&ready(lenient::prepare_contribution(&optional, &old))),
        ["text"]
    );
}

#[test]
fn a_dropped_block_takes_its_actions_and_a_section_its_unknown_children() {
    let raw = card(
        json!([
            {"kind": "hero", "amountPath": "/amount", "actions": ["pay"]},
            {"kind": "section", "blocks": [{"kind": "futureBlock"}, {"kind": "text", "text": "x"}]},
            {"kind": "section", "blocks": [{"kind": "futureBlock"}]}
        ]),
        json!({"actions": [
            {"id": "pay", "label": "付款", "target": {"kind": "operation", "operation": "example.poll.pay"}},
            {"id": "close", "label": "结束", "target": {"kind": "operation", "operation": "example.poll.close"}}
        ]}),
    );
    // A kit that knows sections but not heroes.
    let vocabulary = without_blocks(engine_vocabulary(), &["hero"]);
    let contribution = ready(lenient::prepare_contribution(&raw, &vocabulary));
    assert_eq!(
        block_kinds(&contribution),
        ["section"],
        "the empty section went"
    );
    assert_eq!(contribution.schema.blocks[0].children().len(), 1);
    let ids: Vec<_> = contribution
        .schema
        .actions
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(ids, ["close"], "the hero's action never becomes a button");
}

#[test]
fn unknown_formats_and_renderers_degrade_to_plain_content() {
    let raw = card(
        json!([{"kind": "fields", "fields": [
            {"label": "进度", "path": "/p", "format": {"kind": "percent", "ratio": true}}]}]),
        json!({
            "fields": [{"label": "体积", "path": "/b", "format": {"kind": "futureFormat"}}],
            "presentation": {"renderer": "redPacket.card"}
        }),
    );
    let contribution = ready(lenient::prepare_contribution(&raw, &base_vocabulary()));
    assert!(contribution.schema.fields[0].format.is_none());
    let UiBlock::Fields(fields) = &contribution.schema.blocks[0] else {
        panic!("fields block kept");
    };
    assert!(
        fields.fields[0].format.is_none(),
        "percent is not version 1"
    );
    assert!(contribution.schema.presentation.is_none());
    // A kit with the renderer keeps it.
    let mut kit = engine_vocabulary();
    kit.renderers.insert("redPacket.card".into());
    let kept = ready(lenient::prepare_contribution(&raw, &kit));
    assert!(kept.schema.presentation.is_some());
    assert!(
        kept.schema.fields[0].format.is_none(),
        "unknown to every client"
    );
}

#[test]
fn an_unknown_input_makes_the_contribution_unreadable() {
    let raw = card(
        json!([]),
        json!({"inputs": [{"id": "amount", "kind": "money", "label": "金额",
                           "money": {"currency": "CNY"}}]}),
    );
    assert!(matches!(
        lenient::prepare_contribution(&raw, &base_vocabulary()),
        Prepared::Upgrade(_)
    ));
    assert!(matches!(
        lenient::prepare_contribution(&raw, &engine_vocabulary()),
        Prepared::Ready(_)
    ));
    // A known input with a field this client does not know.
    let future = card(
        json!([]),
        json!({"inputs": [{"id": "n", "kind": "text", "label": "n", "mask": "###"}]}),
    );
    assert!(matches!(
        lenient::prepare_contribution(&future, &engine_vocabulary()),
        Prepared::Upgrade(_)
    ));
}

#[test]
fn an_action_the_client_cannot_run_goes_with_its_references() {
    let raw = card(
        json!([{"kind": "choices", "options": [{"label": "A"}, {"label": "B"}], "action": "vote"}]),
        json!({
            "actions": [
                {"id": "vote", "label": "投票", "target": {"kind": "futureTarget", "x": 1}},
                {"id": "scan", "label": "拍照", "target": {"kind": "native", "capability": "camera"}},
                {"id": "close", "label": "结束",
                 "target": {"kind": "operation", "operation": "example.poll.close"}}
            ],
            "translations": {"en-US": {"title": "Poll",
                "actions": {"vote": "Vote", "scan": "Photo", "close": "Close"}}}
        }),
    );
    let mut vocabulary = engine_vocabulary();
    vocabulary.native.clear();
    let contribution = ready(lenient::prepare_contribution(&raw, &vocabulary));
    let ids: Vec<_> = contribution
        .schema
        .actions
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(ids, ["close"]);
    let translated: Vec<_> = contribution.schema.translations["en-US"]
        .actions
        .keys()
        .collect();
    assert_eq!(translated, ["close"]);
    let UiBlock::Choices(choices) = &contribution.schema.blocks[0] else {
        panic!("choices kept");
    };
    assert_eq!(choices.action, None, "its submit is gone");
    contribution.schema.validate().unwrap();
}

#[test]
fn identity_decides_between_a_placeholder_and_nothing() {
    let vocabulary = engine_vocabulary();
    // A contribution kind this client does not know: nothing to show.
    let mut raw = card(json!([]), json!({}));
    raw["kind"] = json!("scan-handler");
    assert_eq!(
        lenient::prepare_contribution(&raw, &vocabulary),
        Prepared::Dropped
    );
    assert_eq!(
        lenient::prepare_contribution(&json!("x"), &vocabulary),
        Prepared::Dropped
    );
    let mut no_id = card(json!([]), json!({}));
    no_id.as_object_mut().unwrap().remove("id");
    assert_eq!(
        lenient::prepare_contribution(&no_id, &vocabulary),
        Prepared::Dropped
    );
    // A top-level field this client does not know: a placeholder.
    let mut future = card(json!([]), json!({}));
    future["hotness"] = json!(3);
    assert!(matches!(
        lenient::prepare_contribution(&future, &vocabulary),
        Prepared::Upgrade(_)
    ));
    // Core's own placeholder reads as one, with its minBuild.
    let placeholder = serde_json::to_value(dashboard(None).upgrade_placeholder(Some(7))).unwrap();
    let Prepared::Upgrade(read) = lenient::prepare_contribution(&placeholder, &vocabulary) else {
        panic!("expected an upgrade placeholder");
    };
    assert_eq!(read.upgrade.unwrap().min_build, Some(7));
}

// ---- catalog ----

fn catalog_with(contribution: &UiContribution) -> UiCatalog {
    serde_json::from_value(json!({
        "tenantId": "t1", "version": 1, "validUntilMs": 10_000,
        "plugins": [{
            "pluginId": "example.poll", "version": "1.0.0",
            "manifestDigest": format!("sha256:{}", "0".repeat(64)), "grantRevision": 1,
            "extensions": {"platforms": ["android"], "contributions": [contribution]},
            "operations": ["example.poll.dash", "example.poll.mine"]
        }]
    }))
    .unwrap()
}
fn context(kind: UiContributionKind) -> UiRenderContext<'static> {
    UiRenderContext {
        tenant: "t1",
        platform: ClientPlatform::Android,
        kind,
        locale: "en-US",
        viewer: "u1",
        now_ms: 1,
    }
}

#[test]
fn an_upgrade_placeholder_resolves_as_upgrade_required() {
    let page = dashboard(Some(json!({"android": 41020})));
    let placeholder = page
        .select(&client(&engine_vocabulary(), 1, false))
        .into_contribution();
    let catalog = catalog_with(&placeholder);
    let page_context = context(UiContributionKind::Page);
    let Err(reason) = catalog.page("example.poll", "example.poll.dashboard", &page_context) else {
        panic!("a placeholder is no page");
    };
    assert_eq!(reason, UiUnavailableReason::UpgradeRequired);
    let view = ResolvedPluginView::upgrade_required_entry(&placeholder, "en-US");
    assert!(!view.available);
    assert_eq!(view.title, "Dashboard");
    assert_eq!(view.min_build, Some(41020));
    assert_eq!(
        serde_json::to_value(&view).unwrap()["unavailableReason"],
        "upgrade_required"
    );

    // A card of a placeholder shows its summary with the reason and floor.
    let mut card = contribution(card(json!([]), json!({})));
    card.min_client.insert(ClientPlatform::Android, 9);
    let placeholder = card
        .select(&client(&engine_vocabulary(), 1, false))
        .into_contribution();
    let catalog = catalog_with(&placeholder);
    let message: PluginMessage = serde_json::from_value(json!({
        "pluginId": "example.poll", "contributionId": "example.poll.card",
        "schemaVersion": 1, "summary": "[投票] 午饭", "payload": {}
    }))
    .unwrap();
    let view = catalog.resolve_with_context(&message, context(UiContributionKind::MessageContent));
    assert_eq!(
        view.unavailable_reason,
        Some(UiUnavailableReason::UpgradeRequired)
    );
    assert_eq!(view.title, "[投票] 午饭");
    assert_eq!(view.min_build, Some(9));
}

#[test]
fn a_card_bundle_variant_keeps_the_card_declarative() {
    // A message card never runs a bundle inline: its bundle variant keeps the
    // declarative document and the bundle is only what its detail opens.
    let mut raw = card(json!([]), json!({}));
    raw["variants"] = json!([
        {"id": "board", "requires": {"bundle": {"id": "board", "bridge": 1}},
         "bundle": {"id": "board", "route": "/detail"}},
        {"id": "basic"}
    ]);
    let card = contribution(raw);
    let chosen = card
        .select(&client(&engine_vocabulary(), 1, true))
        .into_contribution();
    assert_eq!(chosen.variant.as_deref(), Some("board"));
    assert_eq!(chosen.schema.title, "投票");
    assert_eq!(chosen.schema.translations["en-US"].title, "Poll");
    assert_eq!(chosen.bundle.as_ref().unwrap().route, "/detail");
    let basic = card
        .select(&client(&engine_vocabulary(), 1, false))
        .into_contribution();
    assert_eq!(basic.variant.as_deref(), Some("basic"));
    assert_eq!(basic.schema, chosen.schema);
    assert!(basic.bundle.is_none());
}
