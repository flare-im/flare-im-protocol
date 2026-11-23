//! Records pages (flare-im-design docs/plugin-experience.md 4.7, R2.3): page
//! `tabs` naming sibling pages, and the rows block `icon` that makes each
//! row a record. Pages and blocks without them read and resolve as before.
use flare_plugin_protocol::{
    PluginManifest,
    manifest::UiContribution,
    ui::{lenient::Prepared, *},
};
use serde_json::{Value, json};

fn op(id: &str) -> Value {
    json!({"id": id, "mode": "query", "idempotent": true, "inputSchema": {"type": "object"},
           "outputSchema": {"type": "object"}, "permissionIds": ["example.poll.read"],
           "timeoutMs": 5000})
}
fn tabs() -> Value {
    json!([{"page": "example.poll.received", "label": "收到的"},
           {"page": "example.poll.sent", "label": "发出的"}])
}
fn records_page(name: &str) -> Value {
    json!({
        "kind": "page", "id": format!("example.poll.{name}"), "schemaVersion": 1,
        "requiredPermissions": [], "tabs": tabs(),
        "source": {"operation": format!("example.poll.{name}"), "cursorPath": "/nextCursor"},
        "schema": {
            "title": "红包记录",
            "blocks": [{"kind": "rows", "source": format!("example.poll.{name}"), "path": "/items",
                        "keyPath": "/id", "icon": "red-packet", "emptyText": "还没有红包记录",
                        "columns": [{"label": "金额", "path": "/amount"},
                                    {"label": "标题", "path": "/title"},
                                    {"label": "时间", "path": "/atMs", "format": "time"}]}],
            "translations": {"en-US": {"title": "Red packets",
                                       "texts": {"收到的": "Received", "发出的": "Sent"}}}
        }
    })
}
fn manifest() -> Value {
    let mut manifest: Value =
        serde_json::from_slice(include_bytes!("fixtures/plugin-ui-v2-manifest.json")).unwrap();
    let operations = manifest["capabilities"][0]["operations"]
        .as_array_mut()
        .unwrap();
    operations.push(op("received"));
    operations.push(op("sent"));
    let contributions = manifest["clientExtensions"]["contributions"]
        .as_array_mut()
        .unwrap();
    contributions.push(records_page("received"));
    contributions.push(records_page("sent"));
    manifest
}
fn parse(manifest: &Value) -> Result<PluginManifest, flare_plugin_protocol::ProtocolError> {
    PluginManifest::from_json(&serde_json::to_vec(manifest).unwrap())
}
fn page<'a>(manifest: &'a mut Value, id: &str) -> &'a mut Value {
    let list = manifest["clientExtensions"]["contributions"]
        .as_array_mut()
        .unwrap();
    list.iter_mut().find(|c| c["id"] == id).unwrap()
}
fn rejected(edit: &dyn Fn(&mut Value)) -> bool {
    let mut value = manifest();
    edit(&mut value);
    parse(&value).is_err()
}
fn data() -> Value {
    json!({"items": [{"id": "p1", "title": "苏晚晴", "amount": "2.96", "atMs": 1000}]})
}

#[test]
fn sibling_pages_resolve_their_tabs_with_the_shown_one_selected() {
    let manifest = parse(&manifest()).unwrap();
    let received = manifest.contribution("example.poll.received").unwrap();
    let view = received.resolve_page(&data(), "en-US", "u1").unwrap();
    assert_eq!(
        serde_json::to_value(&view.tabs).unwrap(),
        json!([{"page": "example.poll.received", "label": "Received", "selected": true},
               {"page": "example.poll.sent", "label": "Sent", "selected": false}])
    );
    let sent = manifest.contribution("example.poll.sent").unwrap();
    let view = sent.resolve_page(&data(), "zh-CN", "u1").unwrap();
    assert_eq!(view.tabs[0].label, "收到的", "no translation: as declared");
    assert!(!view.tabs[0].selected && view.tabs[1].selected);
}

#[test]
fn a_rows_icon_resolves_as_declared() {
    let manifest = parse(&manifest()).unwrap();
    let view = manifest
        .contribution("example.poll.received")
        .unwrap()
        .resolve_page(&data(), "zh-CN", "u1")
        .unwrap();
    let ResolvedUiBlock::Rows(rows) = &view.blocks[0] else {
        panic!("rows")
    };
    assert_eq!(rows.icon.as_deref(), Some("red-packet"));
    assert_eq!(
        rows.rows[0].cells[0].text, "2.96",
        "the value column leads, as on a ledger line"
    );
}

#[test]
fn tabs_and_icons_are_validated() {
    assert!(!rejected(&|_| {}));
    let set_tabs = |value: Value| {
        move |m: &mut Value| {
            page(m, "example.poll.received")["tabs"] = value.clone();
            page(m, "example.poll.sent")["tabs"] = value.clone();
        }
    };
    assert!(
        rejected(&set_tabs(
            json!([{"page": "example.poll.received", "label": "收到的"}])
        )),
        "one tab"
    );
    let five: Vec<Value> = (0..5)
        .map(|i| json!({"page": format!("example.poll.p{i}"), "label": "x"}))
        .collect();
    assert!(rejected(&set_tabs(json!(five))), "five tabs");
    assert!(
        rejected(&set_tabs(
            json!([{"page": "example.poll.received", "label": "a"},
                                  {"page": "example.poll.received", "label": "b"}])
        )),
        "a page twice"
    );
    assert!(
        rejected(&set_tabs(
            json!([{"page": "example.poll.received", "label": "a"},
                                  {"page": "example.poll.missing", "label": "b"}])
        )),
        "not a page of this plugin"
    );
    assert!(
        rejected(&set_tabs(
            json!([{"page": "example.poll.received", "label": ""},
                                  {"page": "example.poll.sent", "label": "b"}])
        )),
        "blank label"
    );
    assert!(
        rejected(&|m| {
            page(m, "example.poll.sent")["tabs"][1]["label"] = json!("别的");
        }),
        "siblings declare the same tabs"
    );
    assert!(
        rejected(&|m| {
            page(m, "example.poll.sent")
                .as_object_mut()
                .unwrap()
                .remove("tabs");
        }),
        "a tab's page declares the tabs too"
    );
    assert!(
        rejected(&|m| {
            let other = json!([{"page": "example.poll.sent", "label": "a"},
                               {"page": "example.poll.card", "label": "b"}]);
            page(m, "example.poll.received")["tabs"] = other;
        }),
        "the page is among its tabs, and every tab is a page"
    );
    assert!(
        rejected(&|m| {
            page(m, "example.poll.card")["tabs"] = tabs();
        }),
        "pages only"
    );
    assert!(
        rejected(&|m| {
            page(m, "example.poll.received")["schema"]["blocks"][0]["icon"] = json!("Red Packet");
        }),
        "an icon is a kit icon name"
    );
}

#[test]
fn pages_and_rows_without_them_are_unchanged() {
    let mut value = manifest();
    for name in ["example.poll.received", "example.poll.sent"] {
        let declared = page(&mut value, name);
        declared.as_object_mut().unwrap().remove("tabs");
        declared["schema"]["blocks"][0]
            .as_object_mut()
            .unwrap()
            .remove("icon");
    }
    let manifest = parse(&value).unwrap();
    let contribution = manifest.contribution("example.poll.received").unwrap();
    let raw = serde_json::to_value(contribution).unwrap();
    assert!(raw.get("tabs").is_none(), "nothing new on the wire");
    let view =
        serde_json::to_value(contribution.resolve_page(&data(), "zh-CN", "u1").unwrap()).unwrap();
    assert!(view.get("tabs").is_none());
    assert!(view["blocks"][0].get("icon").is_none());
}

#[test]
fn a_client_reads_tabs_and_icons_and_pages_a_view_that_has_them() {
    let raw: UiContribution = parse(&manifest())
        .unwrap()
        .contribution("example.poll.sent")
        .unwrap()
        .clone();
    let Prepared::Ready(read) =
        lenient::prepare_contribution(&serde_json::to_value(&raw).unwrap(), &engine_vocabulary())
    else {
        panic!("readable")
    };
    assert_eq!(read.tabs, raw.tabs);
    let view = read.resolve_page(&data(), "zh-CN", "u1").unwrap();
    // A host passes the shown view back for its next page: tabs stay.
    let shown = ShownPluginView::from_value(serde_json::to_value(&view).unwrap()).expect("reads");
    assert_eq!(shown.into_value()["tabs"][1]["selected"], json!(true));
    // An upgrade placeholder carries no tabs.
    assert!(raw.upgrade_placeholder(Some(2)).tabs.is_empty());
}
