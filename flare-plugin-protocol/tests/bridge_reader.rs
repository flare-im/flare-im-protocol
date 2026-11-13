//! The bridge reads its messages with one field reader instead of serde. It
//! must accept and refuse exactly what the derived `Deserialize` impls do.
use flare_plugin_protocol::bridge::*;
use flare_plugin_protocol::json::from_value;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

fn samples() -> Vec<Option<Value>> {
    let mut out = vec![None];
    for value in [
        Value::Null,
        json!(""),
        json!("s"),
        json!("info"),
        json!("error"),
        json!("dark"),
        json!("page"),
        json!("composerForm"),
        json!("selectImage"),
        json!("paymentConfirmation"),
        json!("https://example.com/a"),
        json!("theme.changed"),
        json!("op.list"),
        json!(0),
        json!(5),
        json!(4096),
        json!(4097),
        json!(4294967295u64),
        json!(4294967296u64),
        json!(-1),
        json!(5.0),
        json!(true),
        json!(false),
        json!({}),
        json!({"a": 1}),
        json!({"mode": "light"}),
        json!([]),
        json!(["a"]),
        json!(["theme.changed", "entity.state"]),
        json!(["theme.changed", "nope"]),
        json!([1]),
    ] {
        out.push(Some(value));
    }
    out
}

fn typed<T: DeserializeOwned>(
    params: Value,
    wrap: fn(T) -> BridgeRequest,
) -> Result<BridgeRequest, BridgeErrorCode> {
    from_value::<T>(params)
        .map(wrap)
        .map_err(|_| BridgeErrorCode::InvalidParams)
}

/// The decoding the bridge used before: serde, then the bounds.
fn decode_with_serde(
    method: BridgeMethod,
    params: Value,
) -> Result<BridgeRequest, BridgeErrorCode> {
    use BridgeMethod as M;
    use BridgeRequest as R;
    let request = match method {
        M::HostHello => typed(params, R::Hello),
        M::PluginQuery => typed(params, R::PluginQuery),
        M::PluginCommand => typed(params, R::PluginCommand),
        M::UiNavigate => typed(params, R::UiNavigate),
        M::UiClose => typed(params, R::UiClose),
        M::UiSetTitle => typed(params, R::UiSetTitle),
        M::UiToast => typed(params, R::UiToast),
        M::UiConfirm => typed(params, R::UiConfirm),
        M::UiResize => typed(params, R::UiResize),
        M::UiOpenExternal => typed(params, R::UiOpenExternal),
        M::UiSaveState => typed(params, R::UiSaveState),
        M::HostCapability => typed(params, R::HostCapability),
        M::PeopleResolve => typed(params, R::PeopleResolve),
        M::MediaRead => typed(params, R::MediaRead),
        M::StorageGet => typed(params, R::StorageGet),
        M::StorageSet => typed(params, R::StorageSet),
        M::StorageRemove => typed(params, R::StorageRemove),
        M::StorageKeys => typed(params, R::StorageKeys),
        M::ClipboardWrite => typed(params, R::ClipboardWrite),
        M::EventsSubscribe => typed(params, R::EventsSubscribe),
        M::DiagLog => typed(params, R::DiagLog),
    }?;
    request.validate()?;
    Ok(request)
}

fn fields_of(method: BridgeMethod) -> &'static [&'static str] {
    use BridgeMethod as M;
    match method {
        M::HostHello => &["sdk"],
        M::PluginQuery => &["operation", "input"],
        M::PluginCommand => &["operation", "input", "commandId"],
        M::UiNavigate => &["page", "arguments", "bundleRoute"],
        M::UiClose => &["result"],
        M::UiSetTitle => &["title"],
        M::UiToast => &["message", "kind"],
        M::UiConfirm => &["title", "message", "confirmLabel", "destructive"],
        M::UiResize => &["height"],
        M::UiOpenExternal => &["url"],
        M::UiSaveState => &["state"],
        M::HostCapability => &["name", "params"],
        M::PeopleResolve => &["userIds"],
        M::MediaRead => &["fileId"],
        M::StorageGet | M::StorageRemove => &["key"],
        M::StorageSet => &["key", "value"],
        M::StorageKeys => &["prefix"],
        M::ClipboardWrite => &["text"],
        M::EventsSubscribe => &["topics"],
        M::DiagLog => &["level", "message"],
    }
}

/// Every combination of the samples over the method's fields (up to three
/// fields at once), plus an unknown field and non-object parameters.
fn each_params(fields: &[&str], mut check: impl FnMut(Value)) {
    let samples = samples();
    let n = fields.len();
    let mut index = vec![0usize; n];
    loop {
        let mut map = Map::new();
        for (field, i) in fields.iter().zip(&index) {
            if let Some(value) = &samples[*i] {
                map.insert((*field).to_owned(), value.clone());
            }
        }
        check(Value::Object(map.clone()));
        if index.iter().all(|i| *i < 4) {
            map.insert("extra".into(), json!(1));
            check(Value::Object(map));
        }
        // Next combination; with four fields, the fourth stays within a few.
        let mut k = 0;
        loop {
            if k == n {
                break;
            }
            let limit = if k >= 3 { 6 } else { samples.len() };
            index[k] += 1;
            if index[k] < limit {
                break;
            }
            index[k] = 0;
            k += 1;
        }
        if k == n {
            break;
        }
    }
    for other in [Value::Null, json!([]), json!("x"), json!(1)] {
        check(other);
    }
}

#[test]
fn request_parameters_read_exactly_as_serde_reads_them() {
    let mut checked = 0;
    for method in BridgeMethod::ALL {
        each_params(fields_of(*method), |params| {
            let ours = BridgeRequest::decode(*method, params.clone());
            let serde = decode_with_serde(*method, params.clone());
            assert_eq!(ours, serde, "{} {params}", method.name());
            checked += 1;
        });
    }
    assert!(checked > 100_000, "{checked}");
}

fn parse_with_serde(
    bytes: &[u8],
    host_bridge: u32,
) -> Result<BridgeEnvelope, (Option<String>, BridgeErrorCode)> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| (None, BridgeErrorCode::InvalidEnvelope))?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
        })
        .map(str::to_owned);
    let envelope: BridgeEnvelope =
        from_value(value).map_err(|_| (id.clone(), BridgeErrorCode::InvalidEnvelope))?;
    if envelope.kind != BridgeMessageType::Request {
        return Err((id, BridgeErrorCode::InvalidEnvelope));
    }
    envelope.validate().map_err(|code| (id.clone(), code))?;
    if envelope.bridge == 0 || envelope.bridge > host_bridge {
        return Err((id, BridgeErrorCode::UnsupportedBridge));
    }
    Ok(envelope)
}

#[test]
fn envelopes_read_exactly_as_serde_reads_them() {
    let base = json!({"bridge": 1, "id": "r1", "type": "request", "method": "ui.toast", "params": {"message": "hi"}});
    let mut extra_samples = samples();
    extra_samples.extend([
        Some(json!("request")),
        Some(json!("response")),
        Some(json!("event")),
        Some(json!("r:1.a_b-c")),
        Some(json!("bad id")),
        Some(json!(1)),
        Some(json!(2)),
        Some(json!({"code": "internal", "messageKey": "plugin.bridge.internal"})),
    ]);
    let keys = [
        "bridge", "id", "type", "method", "params", "result", "error", "event", "payload",
    ];
    let mut checked = 0;
    let mut check = |value: &Value| {
        let bytes = serde_json::to_vec(value).unwrap();
        for host in [1, 2] {
            assert_eq!(
                BridgeEnvelope::parse(&bytes, host),
                parse_with_serde(&bytes, host),
                "{value}"
            );
        }
        checked += 1;
    };
    // One field varied at a time, then every pair.
    for (a, key_a) in keys.iter().enumerate() {
        for sample_a in &extra_samples {
            let mut one = base.clone();
            match sample_a {
                Some(v) => one[*key_a] = v.clone(),
                None => {
                    one.as_object_mut().unwrap().remove(*key_a);
                }
            }
            check(&one);
            for key_b in &keys[a + 1..] {
                for sample_b in &extra_samples {
                    let mut two = one.clone();
                    match sample_b {
                        Some(v) => two[*key_b] = v.clone(),
                        None => {
                            two.as_object_mut().unwrap().remove(*key_b);
                        }
                    }
                    check(&two);
                }
            }
        }
    }
    let mut unknown = base.clone();
    unknown["other"] = json!(1);
    check(&unknown);
    for raw in [&b"not json"[..], b"[]", b"null", b"1"] {
        assert_eq!(BridgeEnvelope::parse(raw, 1), parse_with_serde(raw, 1));
    }
    assert!(checked > 10_000, "{checked}");
}

#[test]
fn host_side_values_read_exactly_as_serde_reads_them() {
    let mut values: Vec<Value> = samples().into_iter().flatten().collect();
    values.extend([
        json!({"mode": "dark", "tokens": {"bg": "#000"}}),
        json!({"mode": "dark", "tokens": {"bg": 1}}),
        json!({"mode": "dark", "tokens": null}),
        json!({"mode": "dim"}),
        json!({"mode": "light", "extra": 1}),
        json!({"id": "c1", "type": "group"}),
        json!({"id": "c1"}),
        json!({"id": "c1", "type": null}),
        json!({"confirmed": true}),
        json!({"confirmed": null}),
        json!({"confirmed": 1}),
        json!({"locale": "zh-CN"}),
        json!({"locale": 1}),
        json!({"visible": false}),
        json!({"visible": "no"}),
        json!({"visible": true, "locale": "x"}),
    ]);
    for value in values {
        assert_eq!(
            BridgeTheme::read(value.clone()),
            from_value::<BridgeTheme>(value.clone()).ok(),
            "{value}"
        );
        assert_eq!(
            BridgeConversation::read(value.clone()),
            from_value::<BridgeConversation>(value.clone()).ok(),
            "{value}"
        );
        assert_eq!(
            UiConfirmResult::read(value.clone()),
            from_value::<UiConfirmResult>(value.clone()).ok(),
            "{value}"
        );
        assert_eq!(
            LocaleChangedPayload::read(value.clone()),
            from_value::<LocaleChangedPayload>(value.clone()).ok(),
            "{value}"
        );
        assert_eq!(
            VisibilityChangedPayload::read(value.clone()),
            from_value::<VisibilityChangedPayload>(value.clone()).ok(),
            "{value}"
        );
        if let Some(name) = value.as_str() {
            assert_eq!(
                BridgePresentation::parse(name),
                from_value::<BridgePresentation>(value.clone()).ok()
            );
            assert_eq!(
                BridgeErrorCode::parse(name),
                from_value::<BridgeErrorCode>(value.clone()).ok()
            );
        }
    }
    for code in BridgeErrorCode::ALL {
        assert_eq!(BridgeErrorCode::parse(code.code()), Some(*code));
    }
    for name in ["page", "detail", "composerForm"] {
        assert_eq!(
            BridgePresentation::parse(name),
            from_value::<BridgePresentation>(json!(name)).ok()
        );
    }
}
