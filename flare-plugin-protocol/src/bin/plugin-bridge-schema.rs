//! Writes the JSON Schemas of web bundles into a directory (default: print
//! both): `plugin_web_bundle.schema.json` (`bundle.json`) and
//! `plugin_bridge.schema.json` (the bridge envelope with every parameter,
//! result and event payload in `$defs`, and `x-flare-bridge-methods`
//! mapping each method to its parameter and result definitions and the
//! permission it needs). The plugin web SDK generates its types from them.
use flare_plugin_protocol::bridge::{self, BridgeEvent, BridgeMethod, BridgeMethodNeed};
use flare_plugin_protocol::bundle::BundleManifest;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

fn method_types(method: BridgeMethod) -> (&'static str, Option<&'static str>) {
    match method {
        BridgeMethod::HostHello => ("HelloParams", Some("HelloResult")),
        BridgeMethod::PluginQuery => ("PluginQueryParams", Some("PluginOperationResult")),
        BridgeMethod::PluginCommand => ("PluginCommandParams", Some("PluginOperationResult")),
        BridgeMethod::UiNavigate => ("UiNavigateParams", None),
        BridgeMethod::UiClose => ("UiCloseParams", None),
        BridgeMethod::UiSetTitle => ("UiSetTitleParams", None),
        BridgeMethod::UiToast => ("UiToastParams", None),
        BridgeMethod::UiConfirm => ("UiConfirmParams", Some("UiConfirmResult")),
        BridgeMethod::UiResize => ("UiResizeParams", None),
        BridgeMethod::UiOpenExternal => ("UiOpenExternalParams", None),
        BridgeMethod::UiSaveState => ("UiSaveStateParams", None),
        BridgeMethod::HostCapability => ("HostCapabilityParams", None),
        BridgeMethod::PeopleResolve => ("PeopleResolveParams", Some("PeopleResolveResult")),
        BridgeMethod::MediaRead => ("MediaReadParams", Some("MediaReadResult")),
        BridgeMethod::StorageGet => ("StorageKeyParams", Some("StorageGetResult")),
        BridgeMethod::StorageSet => ("StorageSetParams", None),
        BridgeMethod::StorageRemove => ("StorageKeyParams", None),
        BridgeMethod::StorageKeys => ("StorageKeysParams", Some("StorageKeysResult")),
        BridgeMethod::ClipboardWrite => ("ClipboardWriteParams", None),
        BridgeMethod::EventsSubscribe => ("EventsSubscribeParams", None),
        BridgeMethod::DiagLog => ("DiagLogParams", None),
    }
}

fn event_payload(event: BridgeEvent) -> Option<&'static str> {
    match event {
        BridgeEvent::ThemeChanged => Some("BridgeTheme"),
        BridgeEvent::LocaleChanged => Some("LocaleChangedPayload"),
        BridgeEvent::VisibilityChanged => Some("VisibilityChangedPayload"),
        BridgeEvent::CatalogChanged => None,
        BridgeEvent::LifecycleRevoked => Some("LifecycleRevokedPayload"),
        BridgeEvent::EntityState => Some("EntityStatePayload"),
    }
}

fn bridge_schema() -> Value {
    let mut generator = SchemaSettings::draft2020_12().into_generator();
    macro_rules! define {
        ($($ty:ty),* $(,)?) => { $( let _ = generator.subschema_for::<$ty>(); )* };
    }
    define!(
        bridge::BridgeError,
        bridge::BridgeErrorCode,
        bridge::BridgeEvent,
        bridge::BridgePermission,
        bridge::BridgeGrant,
        bridge::HelloParams,
        bridge::HelloResult,
        bridge::PluginQueryParams,
        bridge::PluginCommandParams,
        bridge::PluginOperationResult,
        bridge::UiNavigateParams,
        bridge::UiCloseParams,
        bridge::UiSetTitleParams,
        bridge::UiToastParams,
        bridge::UiConfirmParams,
        bridge::UiConfirmResult,
        bridge::UiResizeParams,
        bridge::UiOpenExternalParams,
        bridge::UiSaveStateParams,
        bridge::HostCapabilityParams,
        bridge::PeopleResolveParams,
        bridge::PeopleResolveResult,
        bridge::MediaReadParams,
        bridge::MediaReadResult,
        bridge::StorageKeyParams,
        bridge::StorageGetResult,
        bridge::StorageSetParams,
        bridge::StorageKeysParams,
        bridge::StorageKeysResult,
        bridge::ClipboardWriteParams,
        bridge::EventsSubscribeParams,
        bridge::DiagLogParams,
        bridge::BridgeTheme,
        bridge::LocaleChangedPayload,
        bridge::VisibilityChangedPayload,
        bridge::LifecycleRevokedPayload,
        bridge::EntityStatePayload,
    );
    let mut schema = generator
        .into_root_schema_for::<bridge::BridgeEnvelope>()
        .to_value();
    let methods: Map<String, Value> = BridgeMethod::ALL
        .iter()
        .map(|method| {
            let (params, result) = method_types(*method);
            let permission = match method.need() {
                BridgeMethodNeed::Nothing => Value::Null,
                BridgeMethodNeed::Permission(permission) => json!(permission.name()),
                BridgeMethodNeed::Capability => json!("capability"),
            };
            (
                method.name().to_owned(),
                json!({
                    "params": params,
                    "result": result,
                    "permission": permission,
                    "since": method.since(),
                    "operation": method.is_operation(),
                }),
            )
        })
        .collect();
    let events: Map<String, Value> = BridgeEvent::ALL
        .iter()
        .map(|event| (event.name().to_owned(), json!(event_payload(*event))))
        .collect();
    if let Value::Object(object) = &mut schema {
        object.insert(
            "x-flare-bridge-version".into(),
            json!(bridge::BRIDGE_VERSION),
        );
        object.insert("x-flare-bridge-methods".into(), Value::Object(methods));
        object.insert("x-flare-bridge-events".into(), Value::Object(events));
        object.insert(
            "x-flare-bridge-error-keys".into(),
            Value::Object(
                bridge::BridgeErrorCode::ALL
                    .iter()
                    .map(|code| (code.code().to_owned(), json!(code.message_key())))
                    .collect(),
            ),
        );
    }
    schema
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bundle = schemars::schema_for!(BundleManifest);
    let outputs = [
        (
            "plugin_web_bundle.schema.json",
            format!("{}\n", serde_json::to_string_pretty(&bundle)?),
        ),
        (
            "plugin_bridge.schema.json",
            format!("{}\n", serde_json::to_string_pretty(&bridge_schema())?),
        ),
    ];
    if let Some(dir) = std::env::args_os().nth(1) {
        let dir = std::path::PathBuf::from(dir);
        for (name, content) in outputs {
            std::fs::write(dir.join(name), content)?;
        }
    } else {
        for (_, content) in outputs {
            print!("{content}");
        }
    }
    Ok(())
}
