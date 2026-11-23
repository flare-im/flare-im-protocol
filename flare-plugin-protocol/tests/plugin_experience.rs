//! Plugin experience additions (flare-im-design docs/plugin-experience.md):
//! `{label, tone}` field values, action `group` and `icon`, the `result`
//! block and `unmapped` on resolved values. Documents without them read and
//! resolve as before.
use flare_plugin_protocol::{manifest::UiContribution, ui::*};
use serde_json::{Value, json};

fn contribution(schema: Value) -> UiContribution {
    serde_json::from_value(json!({
        "kind": "message-content", "id": "x.thing", "schemaVersion": 1,
        "requiredPermissions": [], "schema": schema
    }))
    .unwrap()
}
fn resolve(schema: Value, data: Value) -> ResolvedPluginView {
    let contribution = contribution(schema);
    contribution.schema.validate().unwrap();
    contribution.resolve(&data, "en-US", "u1")
}
fn invalid(document: Value) -> bool {
    serde_json::from_value::<UiDocument>(document).map_or(true, |d| d.validate().is_err())
}
fn op(id: &str) -> Value {
    json!({"kind": "operation", "operation": format!("x.op.{id}")})
}

#[test]
fn field_values_take_words_or_words_with_a_tone() {
    let schema = json!({
        "title": "单据",
        "fields": [
            {"label": "状态", "path": "/state", "values": {
                "done": {"label": "已完成", "tone": "success"},
                "pending": "处理中"}},
            {"label": "备注", "path": "/note"}
        ]
    });
    let view = resolve(schema.clone(), json!({"state": "done", "note": "午饭"}));
    assert_eq!(view.fields[0].value, "已完成");
    assert_eq!(view.fields[0].tone, Some(UiTone::Success));
    assert!(!view.fields[0].unmapped);
    assert_eq!(view.fields[1].tone, None, "free text has no tone");
    assert!(!view.fields[1].unmapped, "free text is never unmapped");

    let plain = resolve(schema.clone(), json!({"state": "pending"}));
    assert_eq!(plain.fields[0].value, "处理中");
    assert_eq!(plain.fields[0].tone, None, "the plain form stays text");

    let unknown = resolve(schema, json!({"state": "prepared"}));
    assert_eq!(unknown.fields[0].value, "prepared", "raw for old kits");
    assert!(unknown.fields[0].unmapped);
    assert_eq!(unknown.fields[0].tone, Some(UiTone::Neutral));

    // The wire keeps both forms as written; a tone-less object stays an object.
    let field: UiField = serde_json::from_value(json!({
        "label": "状态", "path": "/s", "values": {"a": "甲", "b": {"label": "乙"}}
    }))
    .unwrap();
    assert_eq!(
        serde_json::to_value(&field).unwrap()["values"],
        json!({"a": "甲", "b": {"label": "乙"}})
    );
    assert_eq!(field.values["b"].tone(), Some(UiTone::Neutral));
}

#[test]
fn toned_values_are_validated() {
    let doc = |values: Value| json!({"title": "单据", "fields": [{"label": "状态", "path": "/s", "values": values}]});
    assert!(!invalid(doc(
        json!({"a": {"label": "甲", "tone": "warning"}})
    )));
    assert!(
        invalid(doc(json!({"a": {"label": "", "tone": "success"}}))),
        "blank label"
    );
    assert!(
        invalid(doc(json!({"a": {"label": "甲", "tone": "brand"}}))),
        "unknown tone"
    );
    assert!(
        invalid(doc(json!({"a": {"label": "甲", "color": "red"}}))),
        "unknown key"
    );
    assert!(invalid(doc(json!({"a": 3}))), "neither words nor an object");
}

#[test]
fn plain_labels_keep_unlabelled_free_text_as_sent() {
    // A map of words may label a few values of free text (a default wallet's name).
    let view = resolve(
        json!({"title": "钱包", "fields": [{"label": "钱包", "path": "/key",
            "values": {"primary": "余额钱包"}}]}),
        json!({"key": "travel"}),
    );
    assert_eq!(view.fields[0].value, "travel");
    assert!(!view.fields[0].unmapped);
    assert_eq!(view.fields[0].tone, None);
}

#[test]
fn actions_take_a_group_and_an_icon() {
    let schema = json!({
        "title": "钱包",
        "actions": [
            {"id": "history", "label": "交易记录", "group": "common", "icon": "clock",
             "target": {"kind": "page", "page": "x.history"}},
            {"id": "security", "label": "支付密码", "group": "records", "icon": "lock",
             "target": {"kind": "page", "page": "x.security"}},
            {"id": "plain", "label": "说明", "target": op("plain")}
        ]
    });
    let view = resolve(schema, json!({}));
    assert_eq!(view.actions[0].group, Some(UiActionGroup::Common));
    assert_eq!(view.actions[0].icon.as_deref(), Some("clock"));
    assert_eq!(view.actions[1].group, Some(UiActionGroup::Records));
    assert_eq!(view.actions[2].group, None);
    assert_eq!(view.actions[2].icon, None);

    let action = |extra: Value| {
        let mut action = json!({"id": "a", "label": "甲", "target": op("a")});
        action
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        json!({"title": "钱包", "actions": [action]})
    };
    assert!(!invalid(action(json!({"icon": "arrow-down"}))));
    assert!(
        invalid(action(json!({"icon": "Arrow"}))),
        "not a kit icon name"
    );
    assert!(invalid(action(json!({"icon": "a--b"}))), "empty name part");
    assert!(invalid(action(json!({"group": "other"}))), "unknown group");
}

fn result_doc(result: Value) -> Value {
    json!({"title": "结果", "fields": [{"label": "单号", "path": "/id"}], "blocks": [result]})
}
fn transfer_result() -> Value {
    json!({"kind": "result", "statePath": "/state",
           "values": {"completed": {"label": "转账成功", "tone": "success"},
                      "pending": {"label": "处理中", "tone": "info"}},
           "amountPath": "/amount",
           "format": {"kind": "money", "scale": 2},
           "descriptionPath": "/message"})
}

#[test]
fn a_result_block_resolves_its_state_amount_and_description() {
    let view = resolve(
        result_doc(transfer_result()),
        json!({"state": "completed", "amount": "9900", "message": "已转入对方钱包", "id": "t1"}),
    );
    let ResolvedUiBlock::Result(result) = &view.blocks[0] else {
        panic!("{view:#?}")
    };
    assert_eq!(result.state, "completed");
    assert_eq!(result.title, "转账成功");
    assert_eq!(result.tone, UiTone::Success);
    assert!(!result.unmapped);
    assert_eq!(result.amount.as_deref(), Some("99.00"));
    assert!(matches!(result.format, Some(ResolvedUiFormat::Money(_))));
    assert_eq!(result.description.as_deref(), Some("已转入对方钱包"));

    let unknown = resolve(result_doc(transfer_result()), json!({"state": "prepared"}));
    let ResolvedUiBlock::Result(result) = &unknown.blocks[0] else {
        panic!("{unknown:#?}")
    };
    assert!(result.unmapped);
    assert_eq!(
        (result.title.as_str(), result.tone),
        ("prepared", UiTone::Neutral)
    );
    assert_eq!(
        (result.amount.as_deref(), result.description.as_deref()),
        (None, None)
    );

    let absent = resolve(result_doc(transfer_result()), json!({}));
    assert!(absent.blocks.is_empty(), "no state, no result");
}

#[test]
fn result_blocks_are_validated() {
    assert!(!invalid(result_doc(transfer_result())));
    let edited = |edit: &dyn Fn(&mut Value)| {
        let mut result = transfer_result();
        edit(&mut result);
        invalid(result_doc(result))
    };
    assert!(
        edited(&|r| r["values"] = json!({})),
        "a result names its states"
    );
    assert!(
        edited(&|r| r["statePath"] = json!("state")),
        "a JSON pointer"
    );
    assert!(
        edited(&|r| {
            r.as_object_mut().unwrap().remove("amountPath");
        }),
        "a format formats an amount"
    );
    assert!(
        edited(&|r| r["values"]["x"] = json!("words")),
        "states carry {{label, tone}}"
    );
    assert!(
        invalid(json!({"title": "结果", "blocks": [transfer_result(), transfer_result()]})),
        "one result per document"
    );
    assert!(
        engine_vocabulary().blocks.contains("result"),
        "the engine reads the block"
    );
}

#[test]
fn status_and_badges_mark_unlabelled_values() {
    let view = resolve(
        json!({"title": "开户", "blocks": [
            {"kind": "status", "path": "/a", "values": {"ok": {"label": "已通过", "tone": "success"}}},
            {"kind": "status", "path": "/b"}
        ]}),
        json!({"a": "frozen", "b": "free"}),
    );
    let [
        ResolvedUiBlock::Status(labelled),
        ResolvedUiBlock::Status(free),
    ] = &view.blocks[..]
    else {
        panic!("{view:#?}")
    };
    assert!(labelled.unmapped && labelled.text == "frozen");
    assert!(!free.unmapped, "a status without values is free text");
}
