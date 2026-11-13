use flare_plugin_protocol::ui::{PaymentTicket, UiCatalog, UiPaymentProvider};
use serde_json::json;

#[test]
fn a_catalog_names_its_payment_provider_only_when_it_has_one() {
    let without: UiCatalog = serde_json::from_value(
        json!({"tenantId": "t", "version": 1, "validUntilMs": 10, "plugins": []}),
    )
    .unwrap();
    assert_eq!(without.payment_provider, None);
    assert!(
        serde_json::to_value(&without)
            .unwrap()
            .get("paymentProvider")
            .is_none()
    );
    let with: UiCatalog = serde_json::from_value(json!({
        "tenantId": "t", "version": 1, "validUntilMs": 10, "plugins": [],
        "paymentProvider": {"pluginId": "example.pay", "operation": "example.pay.ticket"}
    }))
    .unwrap();
    assert_eq!(
        with.payment_provider,
        Some(UiPaymentProvider {
            plugin_id: "example.pay".into(),
            operation: "example.pay.ticket".into(),
        })
    );
    assert!(
        serde_json::from_value::<UiCatalog>(json!({
            "tenantId": "t", "version": 1, "validUntilMs": 10, "plugins": [],
            "paymentProvider": {"pluginId": "example.pay", "operation": "x", "password": "1"}
        }))
        .is_err()
    );
}

#[test]
fn a_ticket_is_short_opaque_url_safe_text() {
    assert!(PaymentTicket::is_valid("AbC_9-z"));
    assert!(PaymentTicket::is_valid(&"a".repeat(256)));
    for bad in ["", "a b", "a/b", "a=", "票"] {
        assert!(!PaymentTicket::is_valid(bad), "{bad}");
    }
    assert!(!PaymentTicket::is_valid(&"a".repeat(257)));
}
