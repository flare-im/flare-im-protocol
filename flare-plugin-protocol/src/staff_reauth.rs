//! Short-lived, command-bound IAM assurance. Transport carries the opaque token
//! separately from business input and durable command payloads.
use crate::{ProtocolError, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const STAFF_REAUTH_HEADER: &str = "x-flare-staff-reauth";
pub const STAFF_REAUTH_SCHEMA_KEY: &str = "x-flare-staff-reauthentication";
pub const MAX_STAFF_REAUTH_TOKEN_BYTES: usize = 8192;
pub const STAFF_REAUTH_ISSUER: &str = "flare-platform-staff-reauth";
pub const MAX_STAFF_REAUTH_TTL_MS: i64 = 60_000;
pub const MAX_STAFF_REAUTH_AGE_MS: i64 = 300_000;
const TOKEN_TYPE: &str = "flare.staff-reauth.v1";
const CLOCK_SKEW_MS: i64 = 5_000;

fn denied() -> ProtocolError {
    ProtocolError::Denied("staff reauthentication refused")
}
fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "*"
        && value
            .bytes()
            .all(|v| v.is_ascii_alphanumeric() || b"_.:-".contains(&v))
}
/// Transport bounds only. This does not authenticate or decode a proof.
pub fn valid_staff_reauth_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_STAFF_REAUTH_TOKEN_BYTES
        && value.split('.').count() == 3
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|v| v.is_ascii_alphanumeric() || b"_-".contains(&v))
        })
}
/// Exactly the existing Core command identity derivation, shared with IAM.
pub fn staff_command_id(staff_id: &str, idempotency_key: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"flare.plugin.admin.command\0");
    digest.update(staff_id.as_bytes());
    digest.update(b"\0");
    digest.update(idempotency_key.as_bytes());
    format!("intent-{:x}", digest.finalize())
}
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(values) => {
            let mut keys: Vec<_> = values.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), canonical(&values[key])))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical).collect()),
        other => other.clone(),
    }
}
/// Binds the actual post-Core input, including its trusted commandId and stated reason.
pub fn command_digest(
    tenant: &str,
    staff: &str,
    plugin: &str,
    operation: &str,
    input: &Value,
    reason: &str,
) -> Result<String> {
    if ![tenant, staff, plugin, operation].into_iter().all(id)
        || !input.is_object()
        || !crate::invocation::valid_staff_reason(reason)
        || serde_json::to_vec(input).map_err(|_| denied())?.len() > 65_536
    {
        return Err(denied());
    }
    let value = canonical(
        &serde_json::json!({"domain":"flare.staff-command.v1", "tenantId":tenant,
        "staffId":staff,"pluginId":plugin,"operation":operation,"input":input,"reason":reason}),
    );
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&value).map_err(|_| denied())?)
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StaffReauthClaims {
    pub iss: String,
    pub aud: String,
    pub tenant_id: String,
    pub staff_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,
    pub plugin_id: String,
    pub operation: String,
    pub request_id: String,
    pub command_id: String,
    pub payload_digest: String,
    pub nonce: String,
    pub credential_id: String,
    pub credential_revision: String,
    pub verified_at_ms: i64,
    pub issued_at_ms: i64,
    pub expires_at_ms: i64,
}
impl StaffReauthClaims {
    pub fn validate(&self, now_ms: i64) -> Result<()> {
        if ![
            self.tenant_id.as_str(),
            &self.staff_id,
            &self.plugin_id,
            &self.operation,
            &self.request_id,
            &self.command_id,
            &self.credential_id,
        ]
        .into_iter()
        .all(id)
            || self.org_id.as_deref().is_some_and(|v| !id(v))
            || self.iss != STAFF_REAUTH_ISSUER
            || self.aud != format!("{}:staff-reauth", self.plugin_id)
            || !self.operation.starts_with(&format!("{}.", self.plugin_id))
            || !crate::manifest::is_sha256_digest(&self.payload_digest)
            || self
                .credential_revision
                .parse::<u64>()
                .ok()
                .is_none_or(|v| v == 0 || v.to_string() != self.credential_revision)
            || URL_SAFE_NO_PAD
                .decode(&self.nonce)
                .ok()
                .is_none_or(|v| !(16..=64).contains(&v.len()))
            || now_ms < 0
            || self.verified_at_ms <= 0
            || self.issued_at_ms <= 0
            || self.issued_at_ms > now_ms.saturating_add(CLOCK_SKEW_MS)
            || self.verified_at_ms > self.issued_at_ms.saturating_add(CLOCK_SKEW_MS)
            || self.issued_at_ms.saturating_sub(self.verified_at_ms) >= MAX_STAFF_REAUTH_AGE_MS
            || now_ms.saturating_sub(self.verified_at_ms) >= MAX_STAFF_REAUTH_AGE_MS
            || self.expires_at_ms <= now_ms
            || self.expires_at_ms <= self.issued_at_ms
            || self.expires_at_ms.saturating_sub(self.issued_at_ms) > MAX_STAFF_REAUTH_TTL_MS
            || self.expires_at_ms > self.verified_at_ms.saturating_add(MAX_STAFF_REAUTH_AGE_MS)
        {
            return Err(denied());
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: String,
    typ: String,
    kid: String,
}
/// No Debug implementation exposes private signing material.
pub struct StaffReauthSigner {
    kid: String,
    key: SigningKey,
}
impl StaffReauthSigner {
    pub fn new(kid: String, seed: [u8; 32]) -> Result<Self> {
        if !id(&kid) {
            return Err(denied());
        }
        Ok(Self {
            kid,
            key: SigningKey::from_bytes(&seed),
        })
    }
    pub fn sign(&self, claims: &StaffReauthClaims, now_ms: i64) -> Result<String> {
        claims.validate(now_ms)?;
        let header = Header {
            alg: "EdDSA".into(),
            typ: TOKEN_TYPE.into(),
            kid: self.kid.clone(),
        };
        let unsigned = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).map_err(|_| denied())?),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).map_err(|_| denied())?)
        );
        let token = format!(
            "{unsigned}.{}",
            URL_SAFE_NO_PAD.encode(self.key.sign(unsigned.as_bytes()).to_bytes())
        );
        if !valid_staff_reauth_token(&token) {
            return Err(denied());
        }
        Ok(token)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaffReauthVerificationKey {
    pub tenant_id: String,
    pub kid: String,
    pub public_key_base64url: String,
}
#[derive(Clone, Default)]
pub struct StaffReauthVerifier {
    keys: BTreeMap<(String, String), VerifyingKey>,
}
/// Can be constructed only by the verifier; contains no bearer token.
pub struct VerifiedStaffReauth {
    claims: StaffReauthClaims,
}
impl VerifiedStaffReauth {
    pub fn claims(&self) -> &StaffReauthClaims {
        &self.claims
    }
}
impl StaffReauthVerifier {
    pub fn new(keys: Vec<StaffReauthVerificationKey>) -> Result<Self> {
        if keys.len() > 64 {
            return Err(denied());
        }
        let mut result = Self::default();
        for key in keys {
            if !id(&key.tenant_id) || !id(&key.kid) {
                return Err(denied());
            }
            let bytes: [u8; 32] = URL_SAFE_NO_PAD
                .decode(key.public_key_base64url)
                .ok()
                .and_then(|v| v.try_into().ok())
                .ok_or_else(denied)?;
            let value = VerifyingKey::from_bytes(&bytes).map_err(|_| denied())?;
            if result
                .keys
                .insert((key.tenant_id, key.kid), value)
                .is_some()
            {
                return Err(denied());
            }
        }
        Ok(result)
    }
    pub fn verify(&self, token: &str, tenant: &str, now_ms: i64) -> Result<VerifiedStaffReauth> {
        if !valid_staff_reauth_token(token) {
            return Err(denied());
        }
        let parts: Vec<_> = token.split('.').collect();
        let header: Header =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).map_err(|_| denied())?)
                .map_err(|_| denied())?;
        if header.alg != "EdDSA" || header.typ != TOKEN_TYPE {
            return Err(denied());
        }
        let key = self
            .keys
            .get(&(tenant.to_owned(), header.kid))
            .ok_or_else(denied)?;
        let signature =
            Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).map_err(|_| denied())?)
                .map_err(|_| denied())?;
        key.verify_strict(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .map_err(|_| denied())?;
        let claims: StaffReauthClaims =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).map_err(|_| denied())?)
                .map_err(|_| denied())?;
        claims.validate(now_ms)?;
        if claims.tenant_id != tenant {
            return Err(denied());
        }
        Ok(VerifiedStaffReauth { claims })
    }
}
