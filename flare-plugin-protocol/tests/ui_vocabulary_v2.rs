//! Vocabulary version 2: the blocks `section`, `stats`, `chart`, `image`,
//! `qr`, `hero`, `steps`, `banner` and `timeline`; the inputs `money`,
//! `image` and `document`; the formats `percent`, `duration`, `count` and
//! `bytes`; native capability parameters and `into`; protected media on a
//! user page through an owner-scoped query.
use flare_plugin_protocol::{PluginManifest, manifest::UiContribution, ui::*};
use serde_json::{Value, json};

fn op(id: &str) -> Value {
    json!({"kind": "operation", "operation": format!("example.poll.{id}")})
}
fn document(raw: Value) -> Option<UiDocument> {
    serde_json::from_value::<UiDocument>(raw)
        .ok()
        .filter(|d| d.validate().is_ok())
}
fn valid(raw: Value) -> bool {
    document(raw).is_some()
}
fn with_blocks(blocks: Value) -> Value {
    json!({"title": "t", "blocks": blocks})
}
/// Resolve a page document against `data` for `u1`.
fn resolve(raw: Value, data: Value) -> ResolvedPluginView {
    let contribution: UiContribution = serde_json::from_value(json!({
        "kind": "page", "id": "example.poll.page", "schemaVersion": 1,
        "requiredPermissions": [], "schema": raw
    }))
    .unwrap();
    contribution.schema.validate().unwrap();
    contribution.resolve_page(&data, "en-US", "u1").unwrap()
}
fn only(view: &ResolvedPluginView) -> &ResolvedUiBlock {
    assert_eq!(view.blocks.len(), 1, "{view:#?}");
    &view.blocks[0]
}
fn text(n: usize) -> Value {
    json!({"kind": "text", "text": format!("t{n}")})
}

// ---- section ----

#[test]
fn a_section_groups_one_level_and_counts_towards_the_document() {
    let section =
        |blocks: Vec<Value>| json!({"kind": "section", "label": "分组", "blocks": blocks});
    assert!(valid(with_blocks(json!([section(vec![text(0)])]))));
    assert!(!valid(with_blocks(json!([section(vec![])]))));
    assert!(!valid(with_blocks(json!([section(
        (0..9).map(text).collect()
    )]))));
    assert!(!valid(with_blocks(json!([section(vec![section(vec![
        text(0)
    ])])]))));
    assert!(!valid(with_blocks(json!([section(vec![json!({
        "kind": "rows", "source": "example.poll.list", "path": "/r", "keyPath": "/k",
        "columns": [{"label": "c", "path": "/c"}]})])]))));
    // Three full sections are 27 blocks: over 24.
    let full = || section((0..8).map(text).collect());
    assert!(valid(with_blocks(json!([full(), full()]))));
    assert!(!valid(with_blocks(json!([full(), full(), full()]))));
    assert!(!valid(with_blocks(json!([
        full(),
        full(),
        text(0),
        text(1),
        text(2),
        text(3),
        text(4),
        text(5),
        text(6)
    ]))));
}

#[test]
fn a_section_resolves_its_blocks_and_owns_their_actions() {
    let raw = json!({"title": "t",
        "actions": [{"id": "vote", "label": "投票", "target": op("vote")}],
        "blocks": [{"kind": "section", "label": "投票", "collapsed": true, "blocks": [
            {"kind": "choices", "options": [{"label": "A"}, {"label": "B"}], "action": "vote"},
            {"kind": "note", "path": "/missing"}]},
            {"kind": "section", "blocks": [{"kind": "note", "path": "/missing"}]}]});
    let parsed = document(raw.clone()).unwrap();
    assert!(
        parsed.choices_for("vote").is_some(),
        "found inside the section"
    );
    let view = resolve(raw, json!({}));
    assert!(view.actions.is_empty(), "the choices block owns its action");
    let ResolvedUiBlock::Section(section) = only(&view) else {
        panic!("section");
    };
    assert_eq!(section.label.as_deref(), Some("投票"));
    assert!(section.collapsed);
    assert_eq!(section.blocks.len(), 1, "the note has no data");
    let ResolvedUiBlock::Choices(choices) = &section.blocks[0] else {
        panic!("choices");
    };
    assert!(choices.action.is_some());
    // A read-only copy clears the submit inside the section too.
    let copy = view.into_read_only();
    let ResolvedUiBlock::Section(section) = &copy.blocks[0] else {
        panic!("section");
    };
    let ResolvedUiBlock::Choices(choices) = &section.blocks[0] else {
        panic!("choices");
    };
    assert!(choices.action.is_none());
}

// ---- stats ----

#[test]
fn stats_hold_two_to_six_figures_with_formats_and_trends() {
    let item = |n: usize| json!({"label": format!("s{n}"), "valuePath": format!("/v{n}")});
    let stats = |n: usize| {
        with_blocks(json!([{"kind": "stats", "items": (0..n).map(item).collect::<Vec<_>>()}]))
    };
    assert!(!valid(stats(1)));
    assert!(valid(stats(2)));
    assert!(valid(stats(6)));
    assert!(!valid(stats(7)));
    let view = resolve(
        with_blocks(json!([{"kind": "stats", "items": [
            {"label": "余额", "valuePath": "/balance", "format": {"kind": "money", "scale": 2},
             "trendPath": "/delta", "tone": "success"},
            {"label": "笔数", "valuePath": "/count", "trendPath": "/countDelta"},
            {"label": "缺", "valuePath": "/missing"}]}])),
        json!({"balance": "12345", "delta": "+3%", "count": 7, "countDelta": -2}),
    );
    let ResolvedUiBlock::Stats(stats) = only(&view) else {
        panic!("stats");
    };
    assert_eq!(stats.items.len(), 2, "an item without a value is left out");
    assert_eq!(stats.items[0].value, "123.45");
    assert!(matches!(
        stats.items[0].format,
        Some(ResolvedUiFormat::Money(_))
    ));
    let trend = stats.items[0].trend.as_ref().unwrap();
    assert_eq!(
        (trend.text.as_str(), trend.direction),
        ("+3%", UiTrendDirection::Up)
    );
    assert_eq!(stats.items[0].tone, Some(UiTone::Success));
    assert_eq!(
        stats.items[1].trend.as_ref().unwrap().direction,
        UiTrendDirection::Down
    );
}

// ---- chart ----

#[test]
fn a_chart_reads_one_or_more_series_of_bounded_points() {
    let one = json!({"kind": "chart", "chartKind": "bar", "seriesPath": "/days",
                     "labelPath": "/day", "valuePath": "/n", "format": {"kind": "count"}});
    let days: Vec<Value> = (0..70)
        .map(|i| json!({"day": format!("d{i}"), "n": i}))
        .collect();
    let view = resolve(with_blocks(json!([one])), json!({"days": days}));
    let ResolvedUiBlock::Chart(chart) = only(&view) else {
        panic!("chart");
    };
    assert_eq!(chart.series.len(), 1);
    assert_eq!(chart.series[0].points.len(), MAX_UI_CHART_POINTS);
    assert_eq!(chart.series[0].points[3].value, 3.0);
    assert_eq!(chart.series[0].points[3].text, "3");
    // Several series; a donut draws the first only.
    let many = |kind: &str| {
        with_blocks(
            json!([{"kind": "chart", "chartKind": kind, "seriesPath": "/series",
            "pointsPath": "/points", "seriesLabelPath": "/name",
            "labelPath": "/l", "valuePath": "/v"}]),
        )
    };
    let series: Vec<Value> = (0..5)
        .map(|i| json!({"name": format!("s{i}"), "points": [{"l": "a", "v": "1.5"}, {"l": "b"}]}))
        .collect();
    let data = json!({"series": series});
    let ResolvedUiBlock::Chart(lines) = only(&resolve(many("line"), data.clone())).clone() else {
        panic!("chart");
    };
    assert_eq!(lines.series.len(), MAX_UI_CHART_SERIES);
    assert_eq!(lines.series[1].label.as_deref(), Some("s1"));
    assert_eq!(
        lines.series[1].points.len(),
        1,
        "a point without a value is skipped"
    );
    assert_eq!(lines.series[1].points[0].value, 1.5);
    let ResolvedUiBlock::Chart(donut) = only(&resolve(many("donut"), data)).clone() else {
        panic!("chart");
    };
    assert_eq!(donut.series.len(), 1);
    // Empty: the empty text, or nothing.
    let empty = |extra: Value| {
        let mut block = json!({"kind": "chart", "chartKind": "line", "seriesPath": "/s",
                               "labelPath": "/l", "valuePath": "/v"});
        for (key, value) in extra.as_object().unwrap() {
            block[key] = value.clone();
        }
        resolve(with_blocks(json!([block])), json!({"s": []}))
    };
    assert!(empty(json!({})).blocks.is_empty());
    let ResolvedUiBlock::Chart(shown) = only(&empty(json!({"emptyText": "暂无数据"}))).clone()
    else {
        panic!("chart");
    };
    assert_eq!(shown.empty_text.as_deref(), Some("暂无数据"));
    // A series name needs series objects.
    assert!(!valid(with_blocks(
        json!([{"kind": "chart", "chartKind": "line",
        "seriesPath": "/s", "seriesLabelPath": "/name", "labelPath": "/l", "valuePath": "/v"}])
    )));
}

// ---- image ----

#[test]
fn an_image_is_a_signed_asset_or_a_private_file() {
    let image = |source: Value| json!({"kind": "image", "source": source, "aspect": "4:3", "alt": "证件照"});
    assert!(valid(with_blocks(json!([image(
        json!({"asset": "assets/img/banner.png"})
    )]))));
    assert!(!valid(with_blocks(json!([image(
        json!({"asset": "../x.png"})
    )]))));
    assert!(!valid(with_blocks(json!([image(json!({}))]))));
    assert!(!valid(with_blocks(json!([image(
        json!({"asset": "a.png", "filePath": "/f"})
    )]))));
    assert!(!valid(with_blocks(json!([image(
        json!({"url": "https://x"})
    )]))));
    let five: Vec<Value> = (0..5).map(|_| image(json!({"asset": "a.png"}))).collect();
    assert!(!valid(with_blocks(json!(five))));
    let raw = with_blocks(json!([image(json!({"filePath": "/front"}))]));
    let view = resolve(raw.clone(), json!({"front": {"fileId": "f_1"}}));
    let ResolvedUiBlock::Image(image) = only(&view) else {
        panic!("image");
    };
    assert_eq!(image.file_id.as_deref(), Some("f_1"));
    assert_eq!(image.aspect, UiImageAspect::Standard);
    assert!(resolve(raw.clone(), json!({})).blocks.is_empty());
    assert!(
        resolve(raw, json!({"front": {"url": "https://x"}}))
            .blocks
            .is_empty()
    );
}

// ---- qr ----

#[test]
fn a_qr_code_is_bounded_and_may_refresh() {
    let raw = with_blocks(
        json!([{"kind": "qr", "valuePath": "/code", "label": "收款码",
        "refreshOperation": "example.poll.code", "expiresAtPath": "/expiresAtMs"}]),
    );
    let view = resolve(raw.clone(), json!({"code": "pay:abc", "expiresAtMs": 99}));
    let ResolvedUiBlock::Qr(qr) = only(&view) else {
        panic!("qr");
    };
    assert_eq!(qr.value, "pay:abc");
    assert_eq!(qr.refresh_operation.as_deref(), Some("example.poll.code"));
    assert_eq!(
        (qr.value_path.as_str(), qr.expires_at_ms),
        ("/code", Some(99))
    );
    assert!(
        resolve(
            raw.clone(),
            json!({"code": "x".repeat(MAX_UI_QR_VALUE_BYTES + 1)})
        )
        .blocks
        .is_empty()
    );
    let copy = view.into_read_only();
    let ResolvedUiBlock::Qr(qr) = &copy.blocks[0] else {
        panic!("qr");
    };
    assert!(qr.refresh_operation.is_none());
    // A trimmed document loses a refresh it may not call.
    let mut parsed = document(raw).unwrap();
    parsed.retain_targets(&mut |target| {
        !matches!(target, UiActionTarget::Operation { operation } if operation == "example.poll.code")
    });
    let UiBlock::Qr(qr) = &parsed.blocks[0] else {
        panic!("qr");
    };
    assert!(qr.refresh_operation.is_none());
}

// ---- hero ----

#[test]
fn a_hero_shows_one_value_and_owns_up_to_three_actions() {
    let actions = json!([
        {"id": "top", "label": "充值", "target": op("top")},
        {"id": "out", "label": "提现", "target": op("out"),
         "hiddenWhen": [{"path": "/frozen", "anyOf": ["true"]}]},
        {"id": "help", "label": "帮助", "target": op("help")},
        {"id": "more", "label": "更多", "target": op("more")}
    ]);
    let hero = |extra: Value| {
        let mut block = json!({"kind": "hero", "label": "余额", "amountPath": "/balance",
                               "subtitlePath": "/hint", "tone": "brand",
                               "format": {"kind": "money", "scale": 2}});
        for (key, value) in extra.as_object().unwrap() {
            block[key] = value.clone();
        }
        block
    };
    let doc = |blocks: Value| json!({"title": "t", "actions": actions, "blocks": blocks});
    assert!(valid(doc(json!([hero(
        json!({"actions": ["top", "out", "help"]})
    )]))));
    assert!(!valid(doc(json!([hero(
        json!({"actions": ["top", "out", "help", "more"]})
    )]))));
    assert!(!valid(doc(json!([hero(json!({"actions": ["nope"]}))]))));
    assert!(!valid(doc(json!([hero(json!({"valuePath": "/x"}))]))));
    assert!(!valid(doc(json!([hero(json!({})), hero(json!({}))]))));
    let view = resolve(
        doc(json!([hero(json!({"actions": ["top", "out"]}))])),
        json!({"balance": 1050, "hint": "可用", "frozen": true}),
    );
    let ResolvedUiBlock::Hero(hero) = only(&view) else {
        panic!("hero");
    };
    assert_eq!(hero.value, "10.50");
    assert!(hero.amount);
    assert_eq!(hero.tone, UiHeroTone::Brand);
    assert_eq!(hero.subtitle.as_deref(), Some("可用"));
    let ids: Vec<_> = hero.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["top"], "hidden actions stay hidden");
    let buttons: Vec<_> = view.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(buttons, ["help", "more"], "owned actions are not buttons");
}

// ---- steps ----

#[test]
fn steps_read_their_states() {
    let step = |n: usize| json!({"label": format!("第{n}步"), "statePath": format!("/s{n}")});
    let steps = |n: usize| {
        with_blocks(json!([{"kind": "steps", "items": (0..n).map(step).collect::<Vec<_>>()}]))
    };
    assert!(!valid(steps(1)));
    assert!(valid(steps(2)));
    assert!(!valid(steps(7)));
    let view = resolve(
        steps(4),
        json!({"s0": "done", "s1": "current", "s2": "bogus"}),
    );
    let ResolvedUiBlock::Steps(steps_view) = only(&view) else {
        panic!("steps");
    };
    let states: Vec<_> = steps_view.items.iter().map(|s| s.state).collect();
    assert_eq!(
        states,
        [
            UiStepState::Done,
            UiStepState::Current,
            UiStepState::Pending,
            UiStepState::Pending
        ]
    );
    assert!(resolve(steps(2), json!({})).blocks.is_empty());
}

// ---- banner ----

#[test]
fn banners_are_bounded_and_own_their_action() {
    let doc = |blocks: Value| {
        json!({"title": "t", "actions": [{"id": "fix", "label": "重新提交", "target": op("fix")}],
               "blocks": blocks})
    };
    let banner = json!({"kind": "banner", "tone": "danger", "path": "/reason", "actionId": "fix"});
    let plain = json!({"kind": "banner", "tone": "info", "text": "提示"});
    assert!(valid(doc(json!([banner, plain]))));
    assert!(!valid(doc(json!([banner, plain, plain]))));
    assert!(!valid(doc(
        json!([{"kind": "banner", "tone": "info", "text": "a", "path": "/a"}])
    )));
    assert!(!valid(doc(
        json!([{"kind": "banner", "tone": "neutral", "text": "a"}])
    )));
    assert!(!valid(doc(
        json!([{"kind": "banner", "tone": "info", "text": "a", "actionId": "x"}])
    )));
    let view = resolve(doc(json!([banner])), json!({"reason": "照片模糊"}));
    let ResolvedUiBlock::Banner(resolved) = only(&view) else {
        panic!("banner");
    };
    assert_eq!(resolved.text, "照片模糊");
    assert_eq!(resolved.tone, UiBannerTone::Danger);
    assert_eq!(resolved.action.as_ref().unwrap().id, "fix");
    assert!(view.actions.is_empty());
}

// ---- timeline ----

#[test]
fn a_timeline_lists_dated_items_with_badges() {
    let raw = with_blocks(json!([{"kind": "timeline", "itemsPath": "/lines",
        "timePath": "/at", "titlePath": "/title", "detailPath": "/amount",
        "statusPath": "/kind", "values": {"in": {"label": "收入", "tone": "success"}}}]));
    let mut lines: Vec<Value> = (0..60)
        .map(|i| json!({"at": 1000 + i, "title": format!("第{i}笔"), "kind": "in"}))
        .collect();
    lines[1] = json!({"title": "无时间"});
    lines[2]["kind"] = json!("out");
    let view = resolve(raw, json!({"lines": lines}));
    let ResolvedUiBlock::Timeline(timeline) = only(&view) else {
        panic!("timeline");
    };
    assert_eq!(
        timeline.items.len(),
        MAX_UI_TIMELINE_ITEMS - 1,
        "50 read, one skipped"
    );
    assert_eq!(timeline.items[0].status.as_ref().unwrap().text, "收入");
    assert_eq!(timeline.items[1].status.as_ref().unwrap().text, "out");
    assert_eq!(
        timeline.items[1].status.as_ref().unwrap().tone,
        UiTone::Neutral
    );
    assert!(!valid(with_blocks(
        json!([{"kind": "timeline", "itemsPath": "/l",
        "timePath": "/t", "titlePath": "/x", "values": {"a": {"label": "A"}}}])
    )));
}

// ---- inputs ----

fn money(extra: Value) -> Value {
    let mut input = json!({"id": "amount", "kind": "money", "label": "金额",
                           "money": {"currency": "CNY"}});
    for (key, value) in extra.as_object().unwrap() {
        input[key] = value.clone();
    }
    json!({"title": "t", "inputs": [input],
           "actions": [{"id": "send", "label": "发", "form": ["amount"], "target": op("send")}]})
}
fn submit(raw: Value, data: Value, value: Value) -> Result<(), Vec<UiFormIssue>> {
    let parsed = document(raw).unwrap();
    parsed
        .validate_form_over(&parsed.actions[0], &data, &json!({"amount": value}))
        .map(|_| ())
        .map_err(|errors| errors.into_iter().map(|e| e.issue).collect())
}

#[test]
fn money_is_a_bounded_decimal_in_one_currency() {
    assert!(valid(money(json!({}))));
    assert!(!valid(money(json!({"money": {}}))));
    assert!(!valid(money(
        json!({"money": {"currency": "CNY", "currencyPath": "/c"}})
    )));
    assert!(!valid(money(
        json!({"money": {"currency": "CNY", "min": "5", "max": "1"}})
    )));
    assert!(!valid(money(
        json!({"money": {"currency": "CNY", "max": "1.234"}})
    )));
    assert!(!valid(money(
        json!({"money": {"currency": "CNY", "scale": 19}})
    )));
    assert!(
        !valid(money(json!({"min": 1}))),
        "number bounds are not money bounds"
    );
    let mut file = money(json!({}));
    file["inputs"][0]["kind"] = json!("text");
    assert!(!valid(file), "money rules on another kind");

    let raw = money(json!({"money": {"currencyPath": "/currency", "min": "0.01",
                                     "maxPath": "/config/max"}}));
    let data = json!({"currency": "CNY", "config": {"max": "200"}});
    let ok = |amount: &str| json!({"amount": amount, "currency": "CNY"});
    assert_eq!(submit(raw.clone(), data.clone(), ok("12.5")), Ok(()));
    assert_eq!(submit(raw.clone(), data.clone(), ok("200")), Ok(()));
    assert_eq!(
        submit(raw.clone(), data.clone(), ok("200.01")),
        Err(vec![UiFormIssue::OutOfRange])
    );
    assert_eq!(
        submit(raw.clone(), data.clone(), ok("0")),
        Err(vec![UiFormIssue::OutOfRange])
    );
    for bad in ["1.234", "-1", "01", "1e3", "", ".5", "1."] {
        assert_eq!(
            submit(raw.clone(), data.clone(), ok(bad)),
            Err(vec![UiFormIssue::InvalidType]),
            "{bad}"
        );
    }
    assert_eq!(
        submit(
            raw.clone(),
            data.clone(),
            json!({"amount": "1", "currency": "USD"})
        ),
        Err(vec![UiFormIssue::NotAChoice])
    );
    assert_eq!(
        submit(raw.clone(), data, json!({"amount": 1, "currency": "CNY"})),
        Err(vec![UiFormIssue::InvalidType])
    );
    // The resolved input carries what it read, and no paths.
    let view = resolve(raw, json!({"currency": "CNY", "config": {"max": "200"}}));
    let rules = view.inputs[0].money.as_ref().unwrap();
    assert_eq!(rules.currency.as_deref(), Some("CNY"));
    assert_eq!(rules.max.as_deref(), Some("200"));
    assert!(rules.currency_path.is_none() && rules.max_path.is_none());
    // Decimal order never goes through floats.
    assert!(cmp_ui_decimal("10.5", "9.99").is_gt());
    assert!(cmp_ui_decimal("1.50", "1.5").is_eq());
    assert!(
        cmp_ui_decimal(
            "123456789012345678901234567890",
            "123456789012345678901234567891"
        )
        .is_lt()
    );
}

#[test]
fn image_and_document_inputs_are_private_image_uploads() {
    let upload = |accept: Value| {
        json!({"accept": accept, "maxBytes": 1_048_576, "upload": {
            "prepareOperation": "example.poll.prepare", "completeOperation": "example.poll.complete",
            "purpose": "kyc"}})
    };
    let input = |kind: &str, extra: Value| {
        let mut input = json!({"id": "front", "kind": kind, "label": "人像面",
                               "file": upload(json!(["image/jpeg"]))});
        for (key, value) in extra.as_object().unwrap() {
            input[key] = value.clone();
        }
        json!({"title": "t", "inputs": [input],
               "actions": [{"id": "go", "label": "提交", "form": ["front"], "target": op("go")}]})
    };
    assert!(valid(input("image", json!({}))));
    assert!(valid(input("document", json!({"side": "front"}))));
    assert!(
        !valid(input("document", json!({}))),
        "a document says its side"
    );
    assert!(!valid(input("image", json!({"side": "front"}))));
    assert!(!valid(input(
        "image",
        json!({"file": upload(json!(["application/pdf"]))})
    )));
    assert!(!valid(input("image", json!({"file": null}))));
    let raw = input("document", json!({"side": "holding"}));
    assert_eq!(submit_input(&raw, json!({"fileId": "f_1"})), Ok(()));
    assert_eq!(
        submit_input(&raw, json!({"fileId": "f_1", "url": "https://x"})),
        Err(vec![UiFormIssue::InvalidType])
    );
}
fn submit_input(raw: &Value, value: Value) -> Result<(), Vec<UiFormIssue>> {
    let parsed = document(raw.clone()).unwrap();
    parsed
        .validate_form(&parsed.actions[0], &json!({"front": value}))
        .map(|_| ())
        .map_err(|errors| errors.into_iter().map(|e| e.issue).collect())
}

// ---- native parameters and into ----

fn native(target: Value, inputs: Value) -> bool {
    valid(json!({"title": "t", "inputs": inputs,
                 "actions": [{"id": "n", "label": "n", "target": target}]}))
}
fn upload_input(id: &str, kind: &str, side: Option<&str>) -> Value {
    let mut input = json!({"id": id, "kind": kind, "label": id, "file": {
        "accept": ["image/jpeg"], "maxBytes": 1024, "upload": {
            "prepareOperation": "example.poll.prepare",
            "completeOperation": "example.poll.complete", "purpose": "kyc"}}});
    if let Some(side) = side {
        input["side"] = json!(side);
    }
    input
}

#[test]
fn native_targets_take_the_parameters_of_their_capability() {
    let front = json!([upload_input("front", "document", Some("front"))]);
    let capture = |params: Value, into: Value| json!({"kind": "native", "capability": "documentCapture", "params": params, "into": into});
    assert!(native(
        capture(json!({"side": "front"}), json!("front")),
        front.clone()
    ));
    assert!(
        !native(
            capture(json!({"side": "back"}), json!("front")),
            front.clone()
        ),
        "side"
    );
    assert!(!native(
        capture(json!({"side": "front"}), json!("missing")),
        front.clone()
    ));
    assert!(
        !native(
            json!({"kind": "native", "capability": "documentCapture", "params": {"side": "front"}}),
            front.clone()
        ),
        "a capture needs into"
    );
    assert!(!native(capture(json!({}), json!("front")), front.clone()));
    assert!(!native(
        capture(
            json!({"side": "front", "precision": "fine"}),
            json!("front")
        ),
        front.clone()
    ));
    // into names an input of a kind the capability writes.
    let image =
        json!([upload_input("photo", "image", None), {"id": "note", "kind": "text", "label": "n"}]);
    let pick = |into: &str| json!({"kind": "native", "capability": "selectImage", "into": into});
    assert!(native(pick("photo"), image.clone()));
    assert!(!native(pick("note"), image.clone()));
    assert!(native(
        json!({"kind": "native", "capability": "camera"}),
        json!([])
    ));
    assert!(!native(
        json!({"kind": "native", "capability": "camera", "params": {"side": "front"}}),
        json!([])
    ));
    assert!(!native(
        json!({"kind": "native", "capability": "clipboardWrite", "into": "photo"}),
        image
    ));
    // Location and RTC take their own parameters.
    let location =
        |params: Value| json!({"kind": "native", "capability": "locationOnce", "params": params});
    assert!(native(location(json!({"precision": "coarse"})), json!([])));
    assert!(!native(location(json!({"precision": "street"})), json!([])));
    assert!(!native(
        json!({"kind": "native", "capability": "locationOnce"}),
        json!([])
    ));
    let rtc = |media: Value| json!({"kind": "native", "capability": "rtcSession", "params": {"media": media}});
    assert!(native(rtc(json!(["audio", "video"])), json!([])));
    assert!(!native(rtc(json!([])), json!([])));
    assert!(!native(rtc(json!(["audio", "audio"])), json!([])));
    // A payment confirmation starts from an operation result, never a button.
    assert!(!native(
        json!({"kind": "native", "capability": "paymentConfirmation"}),
        json!([])
    ));
    // The wire keeps a plain native target as it was.
    let plain = UiActionTarget::native(NativeCapability::Camera);
    assert_eq!(
        serde_json::to_value(&plain).unwrap(),
        json!({"kind": "native", "capability": "camera"})
    );
}

#[test]
fn a_payment_confirmation_order_comes_from_a_result() {
    let data = json!({"confirmation": {
        "orderId": "o_1", "amount": "8.88", "currency": "CNY", "payee": "张三",
        "title": "红包", "expiresAtMs": 2000, "operation": "example.poll.pay"}});
    let order = PaymentConfirmationOrder::from_result("example.poll", &data).unwrap();
    assert!(order.is_open(1999) && !order.is_open(2000));
    assert!(PaymentConfirmationOrder::from_result("other.plugin", &data).is_none());
    let mut bad = data.clone();
    bad["confirmation"]["amount"] = json!("8.8.8");
    assert!(PaymentConfirmationOrder::from_result("example.poll", &bad).is_none());
    assert!(NativeCapability::PaymentConfirmation.is_sensitive());
    assert!(!NativeCapability::SelectImage.needs_consent());
    assert!(NativeCapability::DocumentCapture.needs_consent());
}

// ---- formats ----

#[test]
fn new_formats_read_numbers_and_keep_a_plain_text() {
    let field = |format: Value| {
        with_blocks(json!([{"kind": "fields", "fields": [
            {"label": "v", "path": "/v", "format": format}]}]))
    };
    let read = |format: Value, value: Value| {
        let view = resolve(field(format), json!({"v": value}));
        let ResolvedUiBlock::Fields(fields) = only(&view) else {
            panic!("fields");
        };
        (
            fields.fields[0].value.clone(),
            fields.fields[0].format.clone(),
        )
    };
    assert_eq!(read(json!({"kind": "percent"}), json!(42)).0, "42%");
    assert_eq!(
        read(
            json!({"kind": "percent", "ratio": true, "decimals": 1}),
            json!(0.125)
        )
        .0,
        "12.5%"
    );
    assert!(!valid(field(json!({"kind": "percent", "decimals": 5}))));
    assert_eq!(
        read(json!({"kind": "duration"}), json!(7_500_000)).0,
        "2h 5m"
    );
    assert_eq!(
        read(json!({"kind": "duration", "unit": "seconds"}), json!(45)).0,
        "45s"
    );
    assert_eq!(read(json!({"kind": "duration"}), json!(86_400_000)).0, "1d");
    let (text, format) = read(json!({"kind": "count"}), json!(12345));
    assert_eq!(text, "12345");
    assert_eq!(
        format,
        Some(ResolvedUiFormat::Count(ResolvedUiCount { count: 12345 }))
    );
    assert_eq!(read(json!({"kind": "bytes"}), json!(1_572_864)).0, "1.5 MB");
    assert_eq!(read(json!({"kind": "bytes"}), json!(512)).0, "512 B");
    assert_eq!(
        read(json!({"kind": "bytes"}), json!(20 * 1024 * 1024)).0,
        "20 MB"
    );
    // A value the format does not fit stays plain.
    let (text, format) = read(json!({"kind": "count"}), json!("many"));
    assert_eq!((text.as_str(), format), ("many", None));
}

// ---- manifest rules ----

fn manifest_value() -> Value {
    let mut manifest: Value =
        serde_json::from_slice(include_bytes!("fixtures/plugin-ui-v2-manifest.json")).unwrap();
    let operations = manifest["capabilities"][0]["operations"]
        .as_array_mut()
        .unwrap();
    for (id, mode, owner) in [
        ("mine", "query", false),
        ("material", "query", true),
        ("code", "query", false),
        ("open", "command", false),
    ] {
        operations.push(json!({
            "id": id, "idempotent": true, "inputSchema": {"type": "object"},
            "outputSchema": {"type": "object"}, "permissionIds": ["example.poll.read"],
            "timeoutMs": 5000, "mode": mode, "ownerScoped": owner
        }));
    }
    manifest["assets"] = json!([{"path": "assets/img/banner.png",
        "digest": format!("sha256:{}", "a".repeat(64)), "size": 10}]);
    manifest
}
fn page_with(blocks: Value, actions: Value, inputs: Value) -> Value {
    json!({"kind": "page", "id": "example.poll.me", "schemaVersion": 1,
           "requiredPermissions": [], "source": {"operation": "example.poll.mine"},
           "schema": {"title": "我的", "blocks": blocks, "actions": actions, "inputs": inputs}})
}
fn publishes(page: Value, edit: &dyn Fn(&mut Value)) -> bool {
    let mut manifest = manifest_value();
    edit(&mut manifest);
    manifest["clientExtensions"]["contributions"]
        .as_array_mut()
        .unwrap()
        .push(page);
    PluginManifest::from_json(&serde_json::to_vec(&manifest).unwrap()).is_ok()
}
fn media(action: &str) -> Value {
    json!([{"kind": "protected-media", "itemsPath": "/materials", "idPath": "/id",
            "action": action, "inputName": "materialId", "urlPath": "/url"}])
}
fn read_action(operation: &str) -> Value {
    json!([{"id": "read", "label": "查看", "form": ["materialId"],
            "target": {"kind": "operation", "operation": operation}}])
}
fn material_input() -> Value {
    json!([{"id": "materialId", "kind": "text", "label": "材料"}])
}

#[test]
fn a_user_page_reads_protected_media_through_an_owner_scoped_query() {
    let none = &|_: &mut Value| {};
    assert!(publishes(
        page_with(
            media("read"),
            read_action("example.poll.material"),
            material_input()
        ),
        none
    ));
    // A query that is not owner-scoped, or a command, is refused.
    assert!(!publishes(
        page_with(
            media("read"),
            read_action("example.poll.code"),
            material_input()
        ),
        none
    ));
    assert!(!publishes(
        page_with(
            media("read"),
            read_action("example.poll.open"),
            material_input()
        ),
        none
    ));
    // Owner scope belongs to queries.
    assert!(!publishes(
        page_with(json!([]), json!([]), json!([])),
        &|m| {
            let operations = m["capabilities"][0]["operations"].as_array_mut().unwrap();
            let open = operations.iter_mut().find(|o| o["id"] == "open").unwrap();
            open["ownerScoped"] = json!(true);
        }
    ));
    // Never on a card or a form.
    let mut card = page_with(
        media("read"),
        read_action("example.poll.material"),
        material_input(),
    );
    card["kind"] = json!("message-content");
    card.as_object_mut().unwrap().remove("source");
    assert!(!publishes(card, none));
}

#[test]
fn image_assets_and_qr_refreshes_are_declared() {
    let none = &|_: &mut Value| {};
    let image = |asset: &str| json!([{"kind": "image", "source": {"asset": asset}, "aspect": "16:9", "alt": "配图"}]);
    assert!(publishes(
        page_with(image("assets/img/banner.png"), json!([]), json!([])),
        none
    ));
    assert!(!publishes(
        page_with(image("assets/img/other.png"), json!([]), json!([])),
        none
    ));
    let qr = |operation: &str| json!([{"kind": "qr", "valuePath": "/code", "refreshOperation": operation}]);
    assert!(publishes(
        page_with(qr("example.poll.code"), json!([]), json!([])),
        none
    ));
    assert!(!publishes(
        page_with(qr("example.poll.open"), json!([]), json!([])),
        none
    ));
    assert!(!publishes(
        page_with(qr("example.poll.unknown"), json!([]), json!([])),
        none
    ));
    let manifest = {
        let mut manifest = manifest_value();
        manifest["clientExtensions"]["contributions"]
            .as_array_mut()
            .unwrap()
            .push(page_with(qr("example.poll.code"), json!([]), json!([])));
        PluginManifest::from_json(&serde_json::to_vec(&manifest).unwrap()).unwrap()
    };
    assert!(
        manifest
            .client_operation_refs()
            .contains("example.poll.code")
    );
}

// ---- entry times, day headings, maskable heroes ----

#[test]
fn an_entry_carries_its_time() {
    let view = resolve(
        with_blocks(
            json!([{"kind": "entries", "itemsPath": "/claims", "userPath": "/id",
            "textPath": "/amount", "timePath": "/at", "numbered": false}]),
        ),
        json!({"claims": [{"id": "u1", "amount": "1.00", "at": 1_700_000_000_000_i64},
                          {"id": "u2", "amount": "2.00", "at": "1700000060000"},
                          {"id": "u3", "amount": "3.00"}]}),
    );
    let ResolvedUiBlock::Entries(entries) = only(&view) else {
        panic!("{view:#?}")
    };
    let times: Vec<_> = entries.items.iter().map(|e| e.time_ms).collect();
    assert_eq!(
        times,
        [Some(1_700_000_000_000), Some(1_700_000_060_000), None]
    );
    assert!(!valid(with_blocks(
        json!([{"kind": "entries", "itemsPath": "/claims", "timePath": "at"}])
    )));
}

#[test]
fn rows_grouped_by_day_need_a_time_column() {
    let rows = |columns: Value| {
        json!({"title": "t", "blocks": [{"kind": "rows", "source": "example.poll.mine",
            "path": "/rows", "keyPath": "/id", "columns": columns, "groupByDay": true}]})
    };
    assert!(valid(rows(
        json!([{"label": "金额", "path": "/amount"}, {"label": "时间", "path": "/at", "format": "time"}])
    )));
    assert!(!valid(rows(json!([{"label": "金额", "path": "/amount"}]))));
}

#[test]
fn a_maskable_hero_says_so() {
    let hero = |extra: Value| {
        let mut block = json!({"kind": "hero", "label": "可用余额", "amountPath": "/balance"});
        for (key, value) in extra.as_object().unwrap() {
            block[key] = value.clone();
        }
        resolve(with_blocks(json!([block])), json!({"balance": "12.00"}))
    };
    let ResolvedUiBlock::Hero(masked) = only(&hero(json!({"maskable": true}))).clone() else {
        panic!()
    };
    assert!(masked.maskable);
    assert_eq!(masked.value, "12.00");
    let plain = hero(json!({}));
    let ResolvedUiBlock::Hero(shown) = only(&plain) else {
        panic!()
    };
    assert!(!shown.maskable);
    assert!(
        serde_json::to_value(shown)
            .unwrap()
            .get("maskable")
            .is_none()
    );
}
