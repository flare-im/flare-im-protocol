use flare_plugin_protocol::ui::{NativeCapability, UiDocument};
use serde_json::json;

fn document() -> serde_json::Value {
    json!({"title":"Security", "inputs":[{"id":"pin","kind":"password","label":"PIN","maxLength":6}],
        "actions":[{"id":"verify","label":"Verify", "form":["pin"],
            "target":{"kind":"native","capability":"reauthenticate"}}]})
}

#[test]
fn native_reauthentication_is_a_closed_ephemeral_form_target() {
    let value: UiDocument = serde_json::from_value(document()).unwrap();
    value.validate().unwrap();
    assert_eq!(
        serde_json::to_value(NativeCapability::Reauthenticate).unwrap(),
        json!("reauthenticate")
    );
    for capability in ["camera", "clipboardWrite", "unknown"] {
        let mut raw = document();
        raw["actions"][0]["target"]["capability"] = json!(capability);
        assert!(
            !serde_json::from_value::<UiDocument>(raw)
                .map(|v| v.validate().is_ok())
                .unwrap_or(false)
        );
    }
    for property in ["command", "continuation"] {
        let mut raw = document();
        raw["actions"][0][property] = if property == "command" {
            json!({"slot":"example.plugin.secret"})
        } else {
            json!({"page":"example.plugin.next"})
        };
        assert!(
            serde_json::from_value::<UiDocument>(raw)
                .unwrap()
                .validate()
                .is_err()
        );
    }
}
