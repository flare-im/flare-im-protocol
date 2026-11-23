//! Plugin experience: an action shown disabled with its reason
//! (`disabledWhen` + `disabledReason`, docs/plugin-experience.md R2.5) and a
//! payment order that says what the payer lacks (`shortfall` + `topUp`, 4.3).
//! Documents and orders without them read and resolve as before.
use flare_plugin_protocol::{
    manifest::UiContribution,
    ui::{lenient::Prepared, *},
};
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
fn pay() -> Value {
    json!({"id": "pay", "label": "付款", "style": "primary", "target": op("pay"),
        "hiddenWhen": [{"path": "/closed", "anyOf": ["true"]}],
        "disabledWhen": [{"path": "/balanceLow", "anyOf": ["true"]}],
        "disabledReason": "余额不足"})
}
fn wallet(extra: Value) -> Value {
    json!({"title": "钱包",
        "actions": [pay(), {"id": "history", "label": "交易记录", "target": op("history")}, extra],
        "blocks": [{"kind": "banner", "tone": "warning", "text": "请先充值", "actionId": "topUp"}],
        "translations": {"en-US": {"title": "Wallet", "texts": {"余额不足": "Not enough balance"}}}})
}
fn top_up() -> Value {
    json!({"id": "topUp", "label": "充值", "target": op("topUp"),
        "disabledWhen": [{"path": "/rechargeClosed", "exists": true}],
        "disabledReason": "暂不可充值"})
}

#[test]
fn an_action_carries_its_reason_only_while_a_condition_holds() {
    let low = resolve(
        wallet(top_up()),
        json!({"balanceLow": true, "rechargeClosed": "x"}),
    );
    assert_eq!(
        low.actions[0].disabled_reason.as_deref(),
        Some("Not enough balance"),
        "translated through texts"
    );
    assert!(
        low.actions[0].disabled_when.is_empty(),
        "conditions stay in the core"
    );
    assert_eq!(
        low.actions[1].disabled_reason, None,
        "no condition, enabled"
    );
    let wire = serde_json::to_value(&low.actions[0]).unwrap();
    assert_eq!(wire["disabledReason"], "Not enough balance");
    assert!(wire.get("disabledWhen").is_none());
    let ResolvedUiBlock::Banner(banner) = &low.blocks[0] else {
        panic!("{low:#?}")
    };
    assert_eq!(
        banner.action.as_ref().unwrap().disabled_reason.as_deref(),
        Some("暂不可充值"),
        "an owned action resolves the same way"
    );

    let enough = resolve(wallet(top_up()), json!({"balanceLow": false}));
    assert_eq!(enough.actions[0].disabled_reason, None);
    assert!(
        serde_json::to_value(&enough.actions[0])
            .unwrap()
            .get("disabledReason")
            .is_none(),
        "an enabled action says nothing"
    );

    // Hidden goes first: a hidden action is not drawn at all, disabled or not.
    let closed = resolve(
        wallet(top_up()),
        json!({"closed": true, "balanceLow": true}),
    );
    assert_eq!(closed.actions.len(), 1);
    assert_eq!(closed.actions[0].id, "history");
}

#[test]
fn disabled_actions_are_validated() {
    let doc = |action: Value| json!({"title": "钱包", "actions": [action]});
    assert!(!invalid(doc(pay())));
    let edited = |edit: &dyn Fn(&mut Value)| {
        let mut action = pay();
        edit(&mut action);
        invalid(doc(action))
    };
    assert!(
        edited(&|a| {
            a.as_object_mut().unwrap().remove("disabledReason");
        }),
        "a condition needs its reason"
    );
    assert!(
        edited(&|a| {
            a.as_object_mut().unwrap().remove("disabledWhen");
        }),
        "a reason needs its condition"
    );
    assert!(
        edited(&|a| a["disabledReason"] = json!("  ")),
        "blank reason"
    );
    assert!(
        edited(&|a| a["disabledReason"] = json!("a\u{7}b")),
        "control character"
    );
    assert!(
        edited(&|a| a["disabledWhen"] = json!([{"path": "/x", "exists": true, "viewer": true}])),
        "one test per condition"
    );
    let five = Value::Array(vec![json!({"path": "/a", "exists": true}); 5]);
    assert!(
        edited(&|a| a["disabledWhen"] = five.clone()),
        "at most four"
    );
    let rows = |remove: Value| {
        json!({"title": "列表", "actions": [remove],
            "blocks": [{"kind": "rows", "source": "x.op.list", "path": "/items", "keyPath": "/id",
                "columns": [{"label": "名称", "path": "/name"}], "actions": ["remove"]}]})
    };
    let remove = json!({"id": "remove", "label": "删除", "target": op("remove")});
    assert!(!invalid(rows(remove.clone())));
    let mut disabled = remove;
    disabled["disabledWhen"] = json!([{"path": "/locked", "exists": true}]);
    disabled["disabledReason"] = json!("已锁定");
    assert!(
        invalid(rows(disabled)),
        "a resolved row carries no disabled state"
    );
}

#[test]
fn documents_without_it_resolve_as_before() {
    let view = resolve(
        json!({"title": "钱包", "actions": [{"id": "history", "label": "交易记录", "target": op("history"),
            "hiddenWhen": [{"path": "/closed", "anyOf": ["true"]}]}]}),
        json!({}),
    );
    assert_eq!(
        serde_json::to_value(&view.actions).unwrap(),
        json!([{"id": "history", "label": "交易记录", "target": op("history"), "input": {}}])
    );
}

#[test]
fn a_tolerant_read_keeps_a_disabled_action() {
    let raw = json!({"kind": "message-content", "id": "x.thing", "schemaVersion": 1,
        "requiredPermissions": [], "schema": wallet(top_up())});
    let Prepared::Ready(read) = lenient::prepare_contribution(&raw, &engine_vocabulary()) else {
        panic!("readable");
    };
    read.schema.validate().unwrap();
    let view = read.resolve(&json!({"balanceLow": true}), "zh-CN", "u1");
    assert_eq!(view.actions[0].disabled_reason.as_deref(), Some("余额不足"));
}

fn order() -> Value {
    json!({"orderId": "o_1", "amount": "26.64", "currency": "CNY", "payee": "红包",
        "title": "发送红包", "expiresAtMs": 2000, "operation": "x.wallet.payments.confirm"})
}
fn read_order(order: Value) -> Option<PaymentConfirmationOrder> {
    PaymentConfirmationOrder::from_result("x.wallet", &json!({"confirmation": order}))
}

#[test]
fn an_order_may_say_what_the_payer_lacks_and_where_to_top_up() {
    let mut short = order();
    short["shortfall"] = json!("6.64");
    short["topUp"] =
        json!({"kind": "page", "page": "x.wallet.page.recharge", "pluginId": "x.wallet"});
    let read = read_order(short.clone()).unwrap();
    assert_eq!(read.shortfall.as_deref(), Some("6.64"));
    assert_eq!(
        read.top_up,
        Some(UiActionTarget::Page {
            page: "x.wallet.page.recharge".into(),
            plugin_id: Some("x.wallet".into())
        })
    );
    assert_eq!(
        serde_json::to_value(&read).unwrap(),
        short,
        "carried as sent"
    );

    // An old order reads and writes back exactly as it came.
    let old = read_order(order()).unwrap();
    assert!(old.shortfall.is_none() && old.top_up.is_none());
    assert_eq!(serde_json::to_value(&old).unwrap(), order());

    let with = |key: &str, value: Value| {
        let mut o = order();
        o[key] = value;
        read_order(o)
    };
    for bad in ["0", "0.00", "-1", "1.2.3", "06.64", "", "abc"] {
        assert!(with("shortfall", json!(bad)).is_none(), "shortfall {bad:?}");
    }
    assert!(with("shortfall", json!(6.64)).is_none(), "a decimal string");
    assert!(with("shortfall", json!("0.01")).is_some());
    assert!(
        with(
            "topUp",
            json!({"kind": "page", "page": "x.wallet.page.recharge"})
        )
        .is_some(),
        "the order's own plugin"
    );
    for bad in [
        json!({"kind": "operation", "operation": "x.wallet.recharge"}),
        json!({"kind": "native", "capability": "selectImage"}),
        json!({"kind": "page", "page": ""}),
        json!({"kind": "page", "page": "p", "pluginId": ""}),
        json!({"kind": "page", "page": "p", "input": {}}),
    ] {
        assert!(with("topUp", bad.clone()).is_none(), "topUp {bad}");
    }
}
