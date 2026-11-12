use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::SigningKey;
use flare_plugin_protocol::staff_reauth::*;
use serde_json::json;

const NOW: i64 = 1_800_000_000_000;
fn claims() -> StaffReauthClaims {
    let command = staff_command_id("staff-a", "intent-a");
    let input =
        json!({"commandId": command, "assetId":"points", "expectedRevision":"7", "amount":"100"});
    StaffReauthClaims {
        iss: STAFF_REAUTH_ISSUER.into(),
        aud: "example.supply:staff-reauth".into(),
        tenant_id: "tenant-a".into(),
        staff_id: "staff-a".into(),
        org_id: None,
        plugin_id: "example.supply".into(),
        operation: "example.supply.issue".into(),
        request_id: "request-a".into(),
        command_id: command,
        payload_digest: command_digest(
            "tenant-a",
            "staff-a",
            "example.supply",
            "example.supply.issue",
            &input,
            "reviewed issuance",
        )
        .unwrap(),
        nonce: URL_SAFE_NO_PAD.encode([7; 32]),
        credential_id: "mfa-a".into(),
        credential_revision: "19".into(),
        verified_at_ms: NOW - 10_000,
        issued_at_ms: NOW,
        expires_at_ms: NOW + 60_000,
    }
}
fn key(tenant: &str, kid: &str, seed: [u8; 32]) -> StaffReauthVerificationKey {
    StaffReauthVerificationKey {
        tenant_id: tenant.into(),
        kid: kid.into(),
        public_key_base64url: URL_SAFE_NO_PAD
            .encode(SigningKey::from_bytes(&seed).verifying_key().to_bytes()),
    }
}
#[test]
fn a_proof_is_tenant_and_key_scoped_and_verifies_while_both_freshness_windows_hold() {
    let signer = StaffReauthSigner::new("key-a".into(), [1; 32]).unwrap();
    let verifier = StaffReauthVerifier::new(vec![
        key("tenant-a", "key-a", [1; 32]),
        key("tenant-a", "key-b", [2; 32]),
    ])
    .unwrap();
    let expected = claims();
    let token = signer.sign(&expected, NOW).unwrap();
    assert!(valid_staff_reauth_token(&token));
    assert_eq!(
        verifier
            .verify(&token, "tenant-a", NOW + 59_999)
            .unwrap()
            .claims(),
        &expected
    );
    assert!(verifier.verify(&token, "tenant-b", NOW).is_err());
    assert!(verifier.verify(&token, "tenant-a", NOW + 60_000).is_err());
    let new = StaffReauthSigner::new("key-b".into(), [2; 32])
        .unwrap()
        .sign(&expected, NOW)
        .unwrap();
    assert!(verifier.verify(&new, "tenant-a", NOW).is_ok());
    let retired = StaffReauthVerifier::new(vec![key("tenant-a", "key-b", [2; 32])]).unwrap();
    assert!(retired.verify(&token, "tenant-a", NOW).is_err());
}
#[test]
fn authority_and_time_fields_cannot_be_rewritten_or_extend_assurance() {
    let signer = StaffReauthSigner::new("key-a".into(), [1; 32]).unwrap();
    let verifier = StaffReauthVerifier::new(vec![key("tenant-a", "key-a", [1; 32])]).unwrap();
    let token = signer.sign(&claims(), NOW).unwrap();
    let parts: Vec<_> = token.split('.').collect();
    let mut body: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    body["staffId"] = json!("other-staff");
    let altered = format!(
        "{}.{}.{}",
        parts[0],
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&body).unwrap()),
        parts[2]
    );
    assert!(verifier.verify(&altered, "tenant-a", NOW).is_err());
    for change in [
        "audience", "purpose", "future", "long ttl", "old mfa", "revision", "nonce",
    ] {
        let mut c = claims();
        match change {
            "audience" => c.aud = "other:staff-reauth".into(),
            "purpose" => c.iss = "flare-social-pin-reauth".into(),
            "future" => {
                c.issued_at_ms = NOW + 5001;
                c.expires_at_ms = NOW + 10_000;
            }
            "long ttl" => c.expires_at_ms = NOW + 60_001,
            "old mfa" => c.verified_at_ms = NOW - 300_000,
            "revision" => c.credential_revision = "01".into(),
            _ => c.nonce = "short".into(),
        }
        assert!(signer.sign(&c, NOW).is_err(), "{change}");
    }
    let mut last_millisecond = claims();
    last_millisecond.verified_at_ms = NOW - 299_999;
    last_millisecond.expires_at_ms = NOW + 1;
    let short = signer.sign(&last_millisecond, NOW).unwrap();
    assert!(verifier.verify(&short, "tenant-a", NOW).is_ok());
    assert!(verifier.verify(&short, "tenant-a", NOW + 1).is_err());
    let mut extended = last_millisecond;
    extended.expires_at_ms = NOW + 60_000;
    assert!(
        signer.sign(&extended, NOW).is_err(),
        "a signed TTL must not extend MFA assurance"
    );
}
#[test]
fn material_input_and_reason_are_bound_but_object_key_order_is_not() {
    let digest = |input: &serde_json::Value, reason: &str| {
        command_digest(
            "tenant-a",
            "staff-a",
            "example.supply",
            "example.supply.issue",
            input,
            reason,
        )
        .unwrap()
    };
    let a = json!({"commandId":"intent-a","nested":{"a":true,"b":"100"}});
    let b: serde_json::Value =
        serde_json::from_str(r#"{"nested":{"b":"100","a":true},"commandId":"intent-a"}"#).unwrap();
    assert_eq!(digest(&a, "reviewed"), digest(&b, "reviewed"));
    let mut changed = b;
    changed["nested"]["b"] = json!("101");
    assert_ne!(digest(&a, "reviewed"), digest(&changed, "reviewed"));
    assert_ne!(digest(&a, "reviewed"), digest(&a, "different reason"));
    assert_ne!(
        staff_command_id("staff-a", "same"),
        staff_command_id("staff-b", "same")
    );
}
#[test]
fn transport_bounds_and_duplicate_key_configuration_fail_closed() {
    for value in [
        "", "a.b", "a.b.c.d", "a..c", "a.b.c,d", "a.b.c ", "a.b.c=", "a.b.é",
    ] {
        assert!(!valid_staff_reauth_token(value));
    }
    assert!(!valid_staff_reauth_token(&format!(
        "a.b.{}",
        "x".repeat(MAX_STAFF_REAUTH_TOKEN_BYTES)
    )));
    assert!(
        StaffReauthVerifier::new(vec![
            key("tenant-a", "same", [1; 32]),
            key("tenant-a", "same", [2; 32])
        ])
        .is_err()
    );
    assert!(
        StaffReauthVerifier::new(vec![])
            .unwrap()
            .verify("a.b.c", "tenant-a", NOW)
            .is_err()
    );
}
