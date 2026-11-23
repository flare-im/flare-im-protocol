//! Plugin experience hand-offs (flare-im-design docs/plugin-experience.md R6,
//! R8, 4.3): a document's refusal words (`errors`), an input's `help`, a
//! continuation's `run` and a payment order's `requestedBy`. All optional:
//! documents and orders without them read, resolve and serialize as before.
use flare_plugin_protocol::{
    manifest::UiContribution,
    ui::{
        PaymentConfirmationOrder, UiDocument, UiPageTransition, UiVocabulary, engine_vocabulary,
        lenient::{Prepared, prepare_contribution},
    },
};
use serde_json::{Value, json};

fn contribution(schema: Value) -> Value {
    json!({"kind": "page", "id": "x.page.home", "schemaVersion": 1,
        "requiredPermissions": [], "schema": schema})
}
fn document(schema: Value) -> UiDocument {
    let document: UiDocument = serde_json::from_value(schema).unwrap();
    document.validate().unwrap();
    document
}
fn invalid(schema: Value) -> bool {
    serde_json::from_value::<UiDocument>(schema).map_or(true, |d| d.validate().is_err())
}
fn op(id: &str) -> Value {
    json!({"kind": "operation", "operation": format!("x.op.{id}")})
}

#[test]
fn a_document_words_its_refusals_in_the_viewers_language() {
    let doc = document(json!({
        "title": "开户",
        "actions": [{"id": "open", "label": "开户", "target": op("open")}],
        "errors": {"WALLET_LIMIT": "钱包数量已达上限", "RISK_DENIED": "暂时无法开户"},
        "translations": {"en-US": {"title": "Open", "fields": [],
            "texts": {"钱包数量已达上限": "You have reached the wallet limit."}}}
    }));
    assert_eq!(
        doc.refusal_text("WALLET_LIMIT", "en-US").as_deref(),
        Some("You have reached the wallet limit.")
    );
    // Same language, another region; then the declared words.
    assert_eq!(
        doc.refusal_text("WALLET_LIMIT", "en-GB").as_deref(),
        Some("You have reached the wallet limit.")
    );
    assert_eq!(
        doc.refusal_text("RISK_DENIED", "en-US").as_deref(),
        Some("暂时无法开户")
    );
    assert_eq!(
        doc.refusal_text("WALLET_LIMIT", "").as_deref(),
        Some("钱包数量已达上限")
    );
    assert_eq!(
        doc.refusal_text("NOT_FOUND", "zh-CN"),
        None,
        "an unworded code"
    );
}

#[test]
fn refusal_words_are_bounded() {
    let doc = |errors: Value| json!({"title": "t", "errors": errors});
    assert!(!invalid(doc(json!({"A": "x", "A_1_B": "y"}))));
    for errors in [
        json!({"lower": "x"}),
        json!({"1CODE": "x"}),
        json!({"CODE-1": "x"}),
        json!({"": "x"}),
        json!({ "A".repeat(65): "x" }),
        json!({"CODE": ""}),
        json!({"CODE": "   "}),
        json!({"CODE": "第一行\n第二行"}),
        json!({"CODE": "字".repeat(86)}),
    ] {
        assert!(invalid(doc(errors.clone())), "{errors}");
    }
    let many: serde_json::Map<String, Value> =
        (0..33).map(|i| (format!("CODE_{i}"), json!("x"))).collect();
    assert!(invalid(doc(Value::Object(many))), "at most 32 codes");
    assert!(invalid(json!({"title": "t", "errors": ["CODE"]})), "a map");
}

#[test]
fn an_input_says_its_rule_under_the_field_in_the_viewers_language() {
    let schema = json!({
        "title": "开户",
        "inputs": [{"id": "instanceKey", "kind": "text", "label": "钱包标识",
            "placeholder": "personal-wallet", "help": "英文、数字或 _ : - .，最多 128 个字符"}],
        "actions": [{"id": "open", "label": "开户", "target": op("open"), "form": ["instanceKey"]}],
        "translations": {"en-US": {"title": "Open", "fields": [], "texts": {
            "英文、数字或 _ : - .，最多 128 个字符": "Letters, digits or _ : - ., up to 128 characters"}}}
    });
    let page: UiContribution = serde_json::from_value(contribution(schema.clone())).unwrap();
    page.schema.validate().unwrap();
    let view = page.resolve_page(&json!({}), "en-US", "u1").unwrap();
    assert_eq!(
        view.inputs[0].help.as_deref(),
        Some("Letters, digits or _ : - ., up to 128 characters")
    );
    assert_eq!(
        view.inputs[0].placeholder.as_deref(),
        Some("personal-wallet")
    );
    let zh = page.resolve_page(&json!({}), "zh-CN", "u1").unwrap();
    assert_eq!(
        zh.inputs[0].help.as_deref(),
        Some("英文、数字或 _ : - .，最多 128 个字符")
    );
    // Bounded like a placeholder.
    let with_help = |help: Value| {
        let mut schema = schema.clone();
        schema["inputs"][0]["help"] = help;
        schema
    };
    assert!(invalid(with_help(json!(""))));
    assert!(invalid(with_help(json!("两行\n帮助"))));
    assert!(invalid(with_help(json!("字".repeat(86)))));
}

#[test]
fn a_continuation_names_the_action_its_page_runs_on_landing() {
    let transition: UiPageTransition = serde_json::from_value(json!({
        "pluginId": "flare.wallet", "page": "flare.wallet.page.checkout",
        "input": {"intentId": "/intentId"}, "run": "authorize"
    }))
    .unwrap();
    transition.validate().unwrap();
    assert_eq!(
        transition.resolve(&json!({"intentId": "i1"})).unwrap(),
        Some(json!({"kind": "host",
            "target": {"kind": "page", "page": "flare.wallet.page.checkout", "pluginId": "flare.wallet"},
            "payload": {"intentId": "i1"}, "run": "authorize"}))
    );
    // Without it the navigation is as before.
    let plain: UiPageTransition =
        serde_json::from_value(json!({"page": "x.page.next", "input": {}})).unwrap();
    assert_eq!(
        plain.resolve(&json!({})).unwrap(),
        Some(
            json!({"kind": "host", "target": {"kind": "page", "page": "x.page.next"}, "payload": {}})
        )
    );
    assert!(serde_json::to_value(&plain).unwrap().get("run").is_none());
    for bad in [
        json!({"page": "x.page.next", "run": ""}),
        json!({"page": "x.page.next", "run": "a b"}),
        json!({"page": "x.page.next", "run": "a".repeat(65)}),
        // The engine runs `actionId` itself; the host runs `run`: never both.
        json!({"page": "x.page.next", "run": "pay", "actionId": "commit"}),
    ] {
        let transition: UiPageTransition = serde_json::from_value(bad.clone()).unwrap();
        assert!(transition.validate().is_err(), "{bad}");
    }
}

#[test]
fn a_payment_order_may_name_the_plugin_it_is_for() {
    let order = |requested_by: Option<Value>| {
        let mut order = json!({"orderId": "ord-1", "amount": "8.88", "currency": "CNY",
            "payee": "红包", "title": "支付红包", "expiresAtMs": 1_900_000_000_000_i64,
            "operation": "flare.wallet.payments.confirm"});
        if let Some(by) = requested_by {
            order["requestedBy"] = by;
        }
        json!({"confirmation": order})
    };
    let paid = PaymentConfirmationOrder::from_result(
        "flare.wallet",
        &order(Some(json!("flare.redpacket"))),
    )
    .unwrap();
    assert_eq!(paid.requested_by.as_deref(), Some("flare.redpacket"));
    assert_eq!(
        serde_json::to_value(&paid).unwrap()["requestedBy"],
        "flare.redpacket"
    );
    let old = PaymentConfirmationOrder::from_result("flare.wallet", &order(None)).unwrap();
    assert_eq!(old.requested_by, None);
    assert!(
        serde_json::to_value(&old)
            .unwrap()
            .get("requestedBy")
            .is_none()
    );
    for bad in [
        json!(""),
        json!("Flare.RedPacket"),
        json!("flare..x"),
        json!("x".repeat(129)),
        json!(7),
    ] {
        assert!(
            PaymentConfirmationOrder::from_result("flare.wallet", &order(Some(bad.clone())))
                .is_none(),
            "{bad}"
        );
    }
}

#[test]
fn the_tolerant_reader_keeps_the_hand_offs() {
    let raw = contribution(json!({
        "title": "开户",
        "inputs": [{"id": "key", "kind": "text", "label": "标识", "help": "最多 128 个字符"}],
        "actions": [{"id": "open", "label": "开户", "target": op("open"), "form": ["key"],
            "command": {"slot": "x.workflow"},
            "continuation": {"page": "x.page.next", "run": "pay"}}],
        "errors": {"WALLET_LIMIT": "钱包数量已达上限"}
    }));
    let vocabulary: UiVocabulary = engine_vocabulary();
    let Prepared::Ready(page) = prepare_contribution(&raw, &vocabulary) else {
        panic!("readable");
    };
    assert_eq!(page.schema.errors["WALLET_LIMIT"], "钱包数量已达上限");
    assert_eq!(
        page.schema.inputs[0].help.as_deref(),
        Some("最多 128 个字符")
    );
    assert_eq!(
        page.schema.actions[0]
            .continuation
            .as_ref()
            .unwrap()
            .run
            .as_deref(),
        Some("pay")
    );
}

#[test]
fn documents_without_the_hand_offs_are_unchanged() {
    let schema = json!({
        "title": "开户",
        "inputs": [{"id": "key", "kind": "text", "label": "标识", "placeholder": "如 a-1"}],
        "actions": [{"id": "open", "label": "开户", "target": op("open"), "form": ["key"]}],
        "translations": {}
    });
    let doc = document(schema.clone());
    assert!(doc.errors.is_empty());
    assert_eq!(doc.inputs[0].help, None);
    let written = serde_json::to_value(&doc).unwrap();
    assert!(written.get("errors").is_none());
    assert!(written["inputs"][0].get("help").is_none());
    assert_eq!(serde_json::from_value::<UiDocument>(written).unwrap(), doc);
}

#[test]
fn a_money_value_names_its_currency() {
    let schema = json!({"title": "钱包", "fields": [{"label": "余额", "path": "/minor",
        "format": {"kind": "money", "scalePath": "/scale", "currencyPath": "/currency"}}]});
    let page: UiContribution = serde_json::from_value(contribution(schema)).unwrap();
    page.schema.validate().unwrap();
    let view = |data: Value| {
        serde_json::to_value(page.resolve_page(&data, "zh-CN", "u1").unwrap()).unwrap()
    };
    let shown = view(json!({"minor": "996100", "scale": 2, "currency": "LABCASH"}));
    assert_eq!(shown["fields"][0]["value"], "9961.00");
    assert_eq!(
        shown["fields"][0]["format"],
        json!({"kind": "money", "minor": "996100", "scale": 2, "currency": "LABCASH"})
    );
    // Something that is no code is left out; the amount still reads.
    let odd = view(json!({"minor": "5", "scale": 0, "currency": "两 元"}));
    assert_eq!(odd["fields"][0]["value"], "5");
    assert!(odd["fields"][0]["format"].get("currency").is_none());
    // Without the path the format reads as before.
    let plain: UiContribution =
        serde_json::from_value(contribution(json!({"title": "钱包", "fields": [
        {"label": "余额", "path": "/minor", "format": {"kind": "money", "scale": 2}}]})))
        .unwrap();
    let before = serde_json::to_value(
        plain
            .resolve_page(&json!({"minor": "100"}), "zh-CN", "u1")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        before["fields"][0]["format"],
        json!({"kind": "money", "minor": "100", "scale": 2})
    );
    assert!(invalid(
        json!({"title": "t", "fields": [{"label": "a", "path": "/a",
        "format": {"kind": "money", "scale": 2, "currencyPath": "no pointer"}}]})
    ));
}
