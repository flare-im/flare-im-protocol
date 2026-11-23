//! The host bridge of web bundles (design 8.6): the only way a bundle reaches
//! its plugin, the host and the viewer.
//!
//! Every message is a [`BridgeEnvelope`] of JSON, carried by `postMessage`
//! (Web, Tauri; the host checks `event.source`) or the platform message
//! channel (Android `addWebMessageListener`, iOS
//! `WKScriptMessageHandlerWithReply`). Hosts only carry messages; the
//! engine checks them in this order and answers with a fixed
//! [`BridgeErrorCode`], never a Core or plugin error text:
//!
//! 1. the envelope ([`BridgeEnvelope::parse`]: at most
//!    [`MAX_BRIDGE_ENVELOPE_BYTES`], a request with an id and a method);
//! 2. the bridge version (`1..=` the host's);
//! 3. `host.hello` first ([`BridgeErrorCode::NotReady`] before it);
//! 4. the method and its parameters ([`BridgeRequest::decode`], operation
//!    input at most [`crate::ui::MAX_UI_ACTION_INPUT_BYTES`]);
//! 5. the bridge permission and the operation or capability
//!    ([`BridgeGrant::authorize`]: `bundle.json` intersected with what the
//!    tenant approved and the catalog authorizes);
//! 6. the rate ([`BRIDGE_REQUESTS_PER_SECOND`],
//!    [`BRIDGE_CONCURRENT_OPERATIONS`]);
//! 7. the call.
//!
//! `bridge` is an integer; methods, events and fields are only ever added.
use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::bundle::{BundleManifest, WebBundleDecl, is_https_url};
use crate::fields::Fields;
use crate::ui::{
    ClientPlugin, MAX_UI_ACTION_INPUT_BYTES, MAX_UI_TEXT_BYTES, NativeCapability,
    valid_bundle_route,
};

/// The bridge version this protocol defines.
pub const BRIDGE_VERSION: u32 = 1;
/// Serialized size of one envelope.
pub const MAX_BRIDGE_ENVELOPE_BYTES: usize = 64 * 1024;
/// Bytes of a request id, `[A-Za-z0-9_.:-]`.
pub const MAX_BRIDGE_REQUEST_ID_BYTES: usize = 64;
/// Requests of one open bundle per second.
pub const BRIDGE_REQUESTS_PER_SECOND: u32 = 20;
/// Plugin operations and host capabilities of one open bundle in flight.
pub const BRIDGE_CONCURRENT_OPERATIONS: u32 = 4;
/// Users of one `people.resolve`.
pub const MAX_BRIDGE_PEOPLE: usize = 50;
/// What `storage` keeps per plugin and user.
pub const MAX_BRIDGE_STORAGE_BYTES: usize = 1024 * 1024;
/// Serialized size of one `storage` value.
pub const MAX_BRIDGE_STORAGE_VALUE_BYTES: usize = 64 * 1024;
/// Bytes of a `storage` key, `[A-Za-z0-9_.:/-]`.
pub const MAX_BRIDGE_STORAGE_KEY_BYTES: usize = 128;
/// Serialized size of the state `ui.saveState` keeps.
pub const MAX_BRIDGE_SAVED_STATE_BYTES: usize = 64 * 1024;
/// Bytes of a `clipboard.write` text.
pub const MAX_BRIDGE_CLIPBOARD_BYTES: usize = 16 * 1024;
/// Bytes of a `diag.log` message.
pub const MAX_BRIDGE_DIAG_BYTES: usize = 1024;
/// Bytes of a `ui.toast` or `ui.confirm` message.
pub const MAX_BRIDGE_MESSAGE_BYTES: usize = 512;
/// Height of `ui.resize`, CSS pixels.
pub const MAX_BRIDGE_RESIZE_HEIGHT: u32 = 4096;
/// Bytes of an id the bridge passes on (a command id, a file id, a user id).
pub const MAX_BRIDGE_ID_BYTES: usize = 128;

/// Permissions a `bundle.json` asks for and a tenant approves.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum BridgePermission {
    #[serde(rename = "plugin.query")]
    PluginQuery,
    #[serde(rename = "plugin.command")]
    PluginCommand,
    #[serde(rename = "ui.navigate")]
    UiNavigate,
    #[serde(rename = "ui.openExternal")]
    UiOpenExternal,
    #[serde(rename = "people.resolve")]
    PeopleResolve,
    #[serde(rename = "media.read")]
    MediaRead,
    #[serde(rename = "storage")]
    Storage,
    #[serde(rename = "clipboard")]
    Clipboard,
}

/// The wire names of every [`BridgePermission`], in declaration order.
pub const BRIDGE_PERMISSIONS: &[&str] = &[
    "plugin.query",
    "plugin.command",
    "ui.navigate",
    "ui.openExternal",
    "people.resolve",
    "media.read",
    "storage",
    "clipboard",
];

impl BridgePermission {
    pub const ALL: &'static [Self] = &[
        Self::PluginQuery,
        Self::PluginCommand,
        Self::UiNavigate,
        Self::UiOpenExternal,
        Self::PeopleResolve,
        Self::MediaRead,
        Self::Storage,
        Self::Clipboard,
    ];
    pub fn name(self) -> &'static str {
        BRIDGE_PERMISSIONS[Self::ALL
            .iter()
            .position(|permission| *permission == self)
            .unwrap_or_default()]
    }
    pub fn parse(name: &str) -> Option<Self> {
        BRIDGE_PERMISSIONS
            .iter()
            .position(|known| *known == name)
            .map(|index| Self::ALL[index])
    }
}

/// The bridge methods. A request names one by its wire name; an unknown
/// name is [`BridgeErrorCode::MethodNotFound`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BridgeMethod {
    HostHello,
    PluginQuery,
    PluginCommand,
    UiNavigate,
    UiClose,
    UiSetTitle,
    UiToast,
    UiConfirm,
    UiResize,
    UiOpenExternal,
    UiSaveState,
    HostCapability,
    PeopleResolve,
    MediaRead,
    StorageGet,
    StorageSet,
    StorageRemove,
    StorageKeys,
    ClipboardWrite,
    EventsSubscribe,
    DiagLog,
}

/// What a method needs beyond the handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeMethodNeed {
    /// Nothing: every bundle may call it.
    Nothing,
    /// The permission.
    Permission(BridgePermission),
    /// The capability named in its parameters, listed in `bundle.json`
    /// `capabilities` and allowed for the plugin.
    Capability,
}

impl BridgeMethod {
    pub const ALL: &'static [Self] = &[
        Self::HostHello,
        Self::PluginQuery,
        Self::PluginCommand,
        Self::UiNavigate,
        Self::UiClose,
        Self::UiSetTitle,
        Self::UiToast,
        Self::UiConfirm,
        Self::UiResize,
        Self::UiOpenExternal,
        Self::UiSaveState,
        Self::HostCapability,
        Self::PeopleResolve,
        Self::MediaRead,
        Self::StorageGet,
        Self::StorageSet,
        Self::StorageRemove,
        Self::StorageKeys,
        Self::ClipboardWrite,
        Self::EventsSubscribe,
        Self::DiagLog,
    ];

    /// The wire name (`plugin.query`, ...).
    pub fn name(self) -> &'static str {
        match self {
            Self::HostHello => "host.hello",
            Self::PluginQuery => "plugin.query",
            Self::PluginCommand => "plugin.command",
            Self::UiNavigate => "ui.navigate",
            Self::UiClose => "ui.close",
            Self::UiSetTitle => "ui.setTitle",
            Self::UiToast => "ui.toast",
            Self::UiConfirm => "ui.confirm",
            Self::UiResize => "ui.resize",
            Self::UiOpenExternal => "ui.openExternal",
            Self::UiSaveState => "ui.saveState",
            Self::HostCapability => "host.capability",
            Self::PeopleResolve => "people.resolve",
            Self::MediaRead => "media.read",
            Self::StorageGet => "storage.get",
            Self::StorageSet => "storage.set",
            Self::StorageRemove => "storage.remove",
            Self::StorageKeys => "storage.keys",
            Self::ClipboardWrite => "clipboard.write",
            Self::EventsSubscribe => "events.subscribe",
            Self::DiagLog => "diag.log",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|method| method.name() == name)
    }

    /// The bridge version that introduced the method.
    pub fn since(self) -> u32 {
        1
    }

    pub fn need(self) -> BridgeMethodNeed {
        use BridgeMethodNeed::{Capability, Nothing, Permission};
        match self {
            Self::PluginQuery => Permission(BridgePermission::PluginQuery),
            Self::PluginCommand => Permission(BridgePermission::PluginCommand),
            Self::UiNavigate => Permission(BridgePermission::UiNavigate),
            Self::UiOpenExternal => Permission(BridgePermission::UiOpenExternal),
            Self::PeopleResolve => Permission(BridgePermission::PeopleResolve),
            Self::MediaRead => Permission(BridgePermission::MediaRead),
            Self::StorageGet | Self::StorageSet | Self::StorageRemove | Self::StorageKeys => {
                Permission(BridgePermission::Storage)
            }
            Self::ClipboardWrite => Permission(BridgePermission::Clipboard),
            Self::HostCapability => Capability,
            Self::HostHello
            | Self::UiClose
            | Self::UiSetTitle
            | Self::UiToast
            | Self::UiConfirm
            | Self::UiResize
            | Self::UiSaveState
            | Self::EventsSubscribe
            | Self::DiagLog => Nothing,
        }
    }

    /// Whether it counts against [`BRIDGE_CONCURRENT_OPERATIONS`]: plugin
    /// operations and host capabilities.
    pub fn is_operation(self) -> bool {
        matches!(
            self,
            Self::PluginQuery | Self::PluginCommand | Self::HostCapability
        )
    }
}

/// `type` of an envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BridgeMessageType {
    /// Bundle to host, with `id`, `method` and `params`.
    Request,
    /// Host to bundle, with the request's `id` and `result` or `error`.
    Response,
    /// Host to bundle, with `event` and `payload`.
    Event,
}

/// One bridge message. Which fields a message has depends on its `type`
/// ([`Self::validate`]); `method` and `event` stay strings so an unknown one
/// gets an answer rather than a parse failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeEnvelope {
    pub bridge: u32,
    /// Request and response: `[A-Za-z0-9_.:-]{1,64}`, chosen by the bundle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: BridgeMessageType,
    /// Request: the method's wire name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Request: the method's parameters, an object (absent reads as `{}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Successful response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Failed response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<BridgeError>,
    /// Event: the topic's wire name ([`BridgeEvent`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    /// Event: the topic's payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

fn valid_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_BRIDGE_REQUEST_ID_BYTES
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}

impl BridgeEnvelope {
    fn empty(kind: BridgeMessageType, id: Option<String>) -> Self {
        Self {
            bridge: BRIDGE_VERSION,
            id,
            kind,
            method: None,
            params: None,
            result: None,
            error: None,
            event: None,
            payload: None,
        }
    }

    pub fn request(id: impl Into<String>, method: BridgeMethod, params: Value) -> Self {
        Self {
            method: Some(method.name().to_owned()),
            params: Some(params),
            ..Self::empty(BridgeMessageType::Request, Some(id.into()))
        }
    }

    pub fn success(id: impl Into<String>, result: Value) -> Self {
        Self {
            result: Some(result),
            ..Self::empty(BridgeMessageType::Response, Some(id.into()))
        }
    }

    /// A failed response; `id` is `None` only when the request had no
    /// readable id.
    pub fn failure(id: Option<String>, error: BridgeError) -> Self {
        Self {
            error: Some(error),
            ..Self::empty(BridgeMessageType::Response, id)
        }
    }

    pub fn event(event: BridgeEvent, payload: Value) -> Self {
        Self {
            event: Some(event.name().to_owned()),
            payload: Some(payload),
            ..Self::empty(BridgeMessageType::Event, None)
        }
    }

    /// Reads a message a bundle sent: at most [`MAX_BRIDGE_ENVELOPE_BYTES`],
    /// JSON, strict fields, a request ([`Self::validate`]), a bridge in
    /// `1..=host_bridge`. On failure the id is returned when it could be
    /// read, so the host can still answer.
    pub fn parse(
        bytes: &[u8],
        host_bridge: u32,
    ) -> Result<Self, (Option<String>, BridgeErrorCode)> {
        if bytes.len() > MAX_BRIDGE_ENVELOPE_BYTES {
            return Err((None, BridgeErrorCode::PayloadTooLarge));
        }
        let value: Value =
            serde_json::from_slice(bytes).map_err(|_| (None, BridgeErrorCode::InvalidEnvelope))?;
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| valid_request_id(id))
            .map(str::to_owned);
        // Only a request is accepted, so every other shape is the same
        // `invalid_envelope`, whatever field made it so.
        let envelope =
            Self::read_request(value).ok_or((id.clone(), BridgeErrorCode::InvalidEnvelope))?;
        envelope.validate().map_err(|code| (id.clone(), code))?;
        if envelope.bridge == 0 || envelope.bridge > host_bridge {
            return Err((id, BridgeErrorCode::UnsupportedBridge));
        }
        Ok(envelope)
    }

    /// A request envelope read as its derived `Deserialize` would, `None` for
    /// anything else: unknown fields, a field of the wrong type, another
    /// `type`, or a field a request never has.
    fn read_request(value: Value) -> Option<Self> {
        let mut f = Fields::new(
            value,
            &[
                "bridge", "id", "type", "method", "params", "result", "error", "event", "payload",
            ],
        )?;
        let bridge = f.u32("bridge")?;
        let id = f.opt_string("id")?;
        let method = f.opt_string("method")?;
        (f.string("type")? == "request"
            && ["result", "error", "event", "payload"]
                .iter()
                .all(|key| f.optional(key).is_none()))
        .then(|| Self {
            bridge,
            id,
            method,
            params: f.optional("params"),
            ..Self::empty(BridgeMessageType::Request, None)
        })
    }

    /// The fields of its type and no other: a request has a valid `id` and
    /// a `method`, `params` an object when present; a response the `id` and
    /// exactly one of `result` and `error`; an event an `event`.
    pub fn validate(&self) -> Result<(), BridgeErrorCode> {
        let valid = match self.kind {
            BridgeMessageType::Request => {
                self.id.as_deref().is_some_and(valid_request_id)
                    && self.method.is_some()
                    && self.params.as_ref().is_none_or(Value::is_object)
                    && self.result.is_none()
                    && self.error.is_none()
                    && self.event.is_none()
                    && self.payload.is_none()
            }
            BridgeMessageType::Response => {
                self.id.as_deref().is_none_or(valid_request_id)
                    && self.result.is_some() != self.error.is_some()
                    && self.method.is_none()
                    && self.params.is_none()
                    && self.event.is_none()
                    && self.payload.is_none()
            }
            BridgeMessageType::Event => {
                self.id.is_none()
                    && self.event.is_some()
                    && self.method.is_none()
                    && self.params.is_none()
                    && self.result.is_none()
                    && self.error.is_none()
            }
        };
        if valid {
            Ok(())
        } else {
            Err(BridgeErrorCode::InvalidEnvelope)
        }
    }

    /// The typed request of a parsed request envelope.
    pub fn request_body(&self) -> Result<BridgeRequest, BridgeErrorCode> {
        let method = self
            .method
            .as_deref()
            .and_then(BridgeMethod::parse)
            .filter(|method| method.since() <= self.bridge)
            .ok_or(BridgeErrorCode::MethodNotFound)?;
        BridgeRequest::decode(
            method,
            self.params
                .clone()
                .unwrap_or_else(|| Value::Object(Map::new())),
        )
    }
}

/// The error of a failed response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeError {
    pub code: BridgeErrorCode,
    /// The kit string a bundle may show ([`BridgeErrorCode::message_key`]).
    pub message_key: String,
    /// `operation_failed` only: the error code the plugin returned, when it
    /// is one of its own (`[A-Za-z0-9_.-]{1,64}`). Never a Core error text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_code: Option<String>,
}

impl BridgeError {
    pub fn new(code: BridgeErrorCode) -> Self {
        Self {
            code,
            message_key: code.message_key().to_owned(),
            plugin_code: None,
        }
    }

    /// `operation_failed` with the plugin's own code, dropped when it is not
    /// a plain name.
    pub fn operation_failed(plugin_code: Option<&str>) -> Self {
        Self {
            plugin_code: plugin_code
                .filter(|code| {
                    !code.is_empty()
                        && code.len() <= 64
                        && code
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                })
                .map(str::to_owned),
            ..Self::new(BridgeErrorCode::OperationFailed)
        }
    }
}

impl From<BridgeErrorCode> for BridgeError {
    fn from(code: BridgeErrorCode) -> Self {
        Self::new(code)
    }
}

/// The fixed error codes of the bridge.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BridgeErrorCode {
    /// Not JSON, unknown fields, or the wrong fields for its type.
    InvalidEnvelope,
    /// The envelope's bridge is above the host's.
    UnsupportedBridge,
    /// A request before `host.hello`.
    NotReady,
    /// No such method in the envelope's bridge version.
    MethodNotFound,
    /// Parameters of the wrong shape or out of bounds.
    InvalidParams,
    /// The envelope or the operation input is too large.
    PayloadTooLarge,
    /// The bridge permission, operation or capability is not granted (not
    /// in `bundle.json`, not approved by the tenant, not authorized by the
    /// catalog, or a transient operation).
    PermissionDenied,
    /// Over [`BRIDGE_REQUESTS_PER_SECOND`] or
    /// [`BRIDGE_CONCURRENT_OPERATIONS`].
    RateLimited,
    /// The viewer declined (a confirmation, a capability, an external link).
    Cancelled,
    /// `clipboard.write` without a recent user gesture.
    GestureRequired,
    /// `storage` is full for this plugin and user.
    QuotaExceeded,
    /// The file, user or page does not exist or is not visible to the caller.
    NotFound,
    /// The host cannot do it here (`ui.resize` outside a sheet, a capability
    /// this host lacks, offline).
    Unavailable,
    /// The call did not answer in time; a command's result is unknown.
    Timeout,
    /// The plugin answered with an error.
    OperationFailed,
    /// The bundle was withdrawn or the plugin disabled; the host closes it.
    Revoked,
    /// Anything else.
    Internal,
}

impl BridgeErrorCode {
    pub const ALL: &'static [Self] = &[
        Self::InvalidEnvelope,
        Self::UnsupportedBridge,
        Self::NotReady,
        Self::MethodNotFound,
        Self::InvalidParams,
        Self::PayloadTooLarge,
        Self::PermissionDenied,
        Self::RateLimited,
        Self::Cancelled,
        Self::GestureRequired,
        Self::QuotaExceeded,
        Self::NotFound,
        Self::Unavailable,
        Self::Timeout,
        Self::OperationFailed,
        Self::Revoked,
        Self::Internal,
    ];

    /// The wire code (`permission_denied`, ...).
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidEnvelope => "invalid_envelope",
            Self::UnsupportedBridge => "unsupported_bridge",
            Self::NotReady => "not_ready",
            Self::MethodNotFound => "method_not_found",
            Self::InvalidParams => "invalid_params",
            Self::PayloadTooLarge => "payload_too_large",
            Self::PermissionDenied => "permission_denied",
            Self::RateLimited => "rate_limited",
            Self::Cancelled => "cancelled",
            Self::GestureRequired => "gesture_required",
            Self::QuotaExceeded => "quota_exceeded",
            Self::NotFound => "not_found",
            Self::Unavailable => "unavailable",
            Self::Timeout => "timeout",
            Self::OperationFailed => "operation_failed",
            Self::Revoked => "revoked",
            Self::Internal => "internal",
        }
    }

    pub fn parse(code: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|known| known.code() == code)
    }

    /// The kit string key: `plugin.bridge.<lowerCamelCode>`.
    pub fn message_key(self) -> &'static str {
        match self {
            Self::InvalidEnvelope => "plugin.bridge.invalidEnvelope",
            Self::UnsupportedBridge => "plugin.bridge.unsupportedBridge",
            Self::NotReady => "plugin.bridge.notReady",
            Self::MethodNotFound => "plugin.bridge.methodNotFound",
            Self::InvalidParams => "plugin.bridge.invalidParams",
            Self::PayloadTooLarge => "plugin.bridge.payloadTooLarge",
            Self::PermissionDenied => "plugin.bridge.permissionDenied",
            Self::RateLimited => "plugin.bridge.rateLimited",
            Self::Cancelled => "plugin.bridge.cancelled",
            Self::GestureRequired => "plugin.bridge.gestureRequired",
            Self::QuotaExceeded => "plugin.bridge.quotaExceeded",
            Self::NotFound => "plugin.bridge.notFound",
            Self::Unavailable => "plugin.bridge.unavailable",
            Self::Timeout => "plugin.bridge.timeout",
            Self::OperationFailed => "plugin.bridge.operationFailed",
            Self::Revoked => "plugin.bridge.revoked",
            Self::Internal => "plugin.bridge.internal",
        }
    }

    /// Whether the same request may succeed later unchanged.
    pub fn retryable(self) -> bool {
        matches!(self, Self::RateLimited | Self::Unavailable | Self::Timeout)
    }
}

/// Event topics.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum BridgeEvent {
    /// [`BridgeTheme`].
    #[serde(rename = "theme.changed")]
    ThemeChanged,
    /// [`LocaleChangedPayload`].
    #[serde(rename = "locale.changed")]
    LocaleChanged,
    /// [`VisibilityChangedPayload`].
    #[serde(rename = "visibility.changed")]
    VisibilityChanged,
    /// `{}`: the plugin's catalog entry changed; re-read what depends on it.
    #[serde(rename = "catalog.changed")]
    CatalogChanged,
    /// [`LifecycleRevokedPayload`]; always delivered, subscribed or not. The
    /// host closes the bundle right after.
    #[serde(rename = "lifecycle.revoked")]
    LifecycleRevoked,
    /// [`EntityStatePayload`]: shared state of one of this plugin's entities.
    #[serde(rename = "entity.state")]
    EntityState,
}

impl BridgeEvent {
    pub const ALL: &'static [Self] = &[
        Self::ThemeChanged,
        Self::LocaleChanged,
        Self::VisibilityChanged,
        Self::CatalogChanged,
        Self::LifecycleRevoked,
        Self::EntityState,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::ThemeChanged => "theme.changed",
            Self::LocaleChanged => "locale.changed",
            Self::VisibilityChanged => "visibility.changed",
            Self::CatalogChanged => "catalog.changed",
            Self::LifecycleRevoked => "lifecycle.revoked",
            Self::EntityState => "entity.state",
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|event| event.name() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BridgeThemeMode {
    Light,
    Dark,
}

impl BridgeThemeMode {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

/// The host theme: its mode and the kit tokens the SDK maps to CSS
/// variables (name to CSS value).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeTheme {
    pub mode: BridgeThemeMode,
    #[serde(default)]
    pub tokens: BTreeMap<String, String>,
}

impl BridgeTheme {
    /// Read as its derived `Deserialize` would.
    pub fn read(value: Value) -> Option<Self> {
        let mut f = Fields::new(value, &["mode", "tokens"])?;
        Some(Self {
            mode: f.name("mode", BridgeThemeMode::parse)?,
            tokens: {
                // Inserted one by one: collecting would sort, a second copy
                // of the map's ordering code.
                let mut tokens = BTreeMap::new();
                for (name, value) in f.object_or_empty("tokens")? {
                    let Value::String(css) = value else {
                        return None;
                    };
                    tokens.insert(name, css);
                }
                tokens
            },
        })
    }
}

/// Where the bundle is shown (design 8.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BridgePresentation {
    /// A page.
    Page,
    /// A card's detail, in a sheet.
    Detail,
    /// A composer form: `ui.close` carries the operation result.
    ComposerForm,
}

impl BridgePresentation {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "page" => Some(Self::Page),
            "detail" => Some(Self::Detail),
            "composerForm" => Some(Self::ComposerForm),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeConversation {
    pub id: String,
    /// `single`, `group`, ... as the host names conversation types.
    #[serde(rename = "type")]
    pub kind: String,
}

impl BridgeConversation {
    /// Read as its derived `Deserialize` would.
    pub fn read(value: Value) -> Option<Self> {
        let mut f = Fields::new(value, &["id", "type"])?;
        Some(Self {
            id: f.string("id")?,
            kind: f.string("type")?,
        })
    }
}

/// `host.hello` parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HelloParams {
    /// The SDK and its version (`plugin-web-sdk/1.0.0`), for diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk: Option<String>,
}

/// `host.hello` result: the context of this opening.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HelloResult {
    /// The bridge the host speaks.
    pub bridge: u32,
    pub plugin_id: String,
    pub version: String,
    pub contribution_id: String,
    /// The route opened ([`crate::ui::UiBundleRef::route`]).
    pub route: String,
    /// Page arguments, or the card payload of a detail.
    #[serde(default)]
    pub arguments: Map<String, Value>,
    pub locale: String,
    pub theme: BridgeTheme,
    pub presentation: BridgePresentation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<BridgeConversation>,
    /// The granted permissions ([`BridgeGrant::permissions`]).
    pub permissions: BTreeSet<BridgePermission>,
    /// The granted capabilities ([`BridgeGrant::capabilities`]).
    #[serde(default)]
    pub capabilities: BTreeSet<NativeCapability>,
    /// What the bundle saved with `ui.saveState` before the host destroyed
    /// it in the background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_state: Option<Value>,
}

/// `plugin.query` parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginQueryParams {
    /// Wire id of a query operation of the plugin.
    pub operation: String,
    /// An object, at most [`MAX_UI_ACTION_INPUT_BYTES`] serialized.
    #[serde(default = "empty_object")]
    pub input: Value,
}

/// `plugin.command` parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCommandParams {
    pub operation: String,
    #[serde(default = "empty_object")]
    pub input: Value,
    /// The id of an earlier attempt whose result is unknown, to recover it
    /// from the host's command log; new commands leave it out and the host
    /// generates one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
}

/// `plugin.query` and `plugin.command` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginOperationResult {
    /// The operation's `data`. A `data.message` of a command is posted by
    /// the host as for a declarative action.
    pub data: Value,
    /// Commands: the id the host logged the command under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

/// `ui.navigate` parameters: exactly one of `page` and `bundleRoute`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiNavigateParams {
    /// A page contribution id of this plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// Page arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Map<String, Value>>,
    /// A route of this bundle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_route: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiCloseParams {
    /// A composer form: the operation result whose `data.message` the host
    /// posts. Ignored elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiSetTitleParams {
    pub title: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum UiToastKind {
    #[default]
    Info,
    Success,
    Error,
}

impl UiToastKind {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "info" => Some(Self::Info),
            "success" => Some(Self::Success),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiToastParams {
    pub message: String,
    #[serde(default)]
    pub kind: UiToastKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiConfirmParams {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm_label: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub destructive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiConfirmResult {
    pub confirmed: bool,
}

impl UiConfirmResult {
    /// Read as its derived `Deserialize` would.
    pub fn read(value: Value) -> Option<Self> {
        let mut f = Fields::new(value, &["confirmed"])?;
        match f.take("confirmed")? {
            Value::Bool(confirmed) => Some(Self { confirmed }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiResizeParams {
    /// CSS pixels, `1..=MAX_BRIDGE_RESIZE_HEIGHT`; sheets only.
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiOpenExternalParams {
    /// `https` only ([`is_https_url`]); the host asks the viewer first and
    /// opens the system browser (D-13).
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiSaveStateParams {
    /// At most [`MAX_BRIDGE_SAVED_STATE_BYTES`] serialized; returned in the
    /// next `host.hello` of the same opening.
    pub state: Value,
}

/// `host.capability` parameters: the capability and its parameters as a
/// declarative action gives them ([`crate::ui::NativeParams`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostCapabilityParams {
    pub name: NativeCapability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A QR code captured by the host. The wire carries a format and bounded plain text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeScanResult {
    pub format: BridgeScanFormat,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BridgeScanFormat {
    Qr,
}

/// The value returned by `host.capability`; the requested capability determines its shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum HostCapabilityResult {
    File(crate::ui::UiFileReference),
    Scan(BridgeScanResult),
    Location { location: crate::ui::UiLocationValue },
    Operation { data: Value },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PeopleResolveParams {
    /// At most [`MAX_BRIDGE_PEOPLE`].
    pub user_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgePerson {
    pub user_id: String,
    pub display_name: String,
    /// A `blob:` address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

/// `people.resolve` result: the users the caller may see, others left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PeopleResolveResult {
    pub people: Vec<BridgePerson>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaReadParams {
    /// A file the plugin uploaded privately.
    pub file_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaReadResult {
    /// The file as a `data:` address from the host, at most 4 MiB (the
    /// bundle file limit); the plugin web SDK hands the bundle a `blob:`
    /// address made from it.
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// `storage.get` and `storage.remove` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageKeyParams {
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageGetResult {
    /// `null` when the key is not set.
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageSetParams {
    pub key: String,
    /// At most [`MAX_BRIDGE_STORAGE_VALUE_BYTES`] serialized.
    pub value: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageKeysParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageKeysResult {
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClipboardWriteParams {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventsSubscribeParams {
    pub topics: BTreeSet<BridgeEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DiagLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl DiagLevel {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagLogParams {
    pub level: DiagLevel,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocaleChangedPayload {
    pub locale: String,
}

impl LocaleChangedPayload {
    /// Read as its derived `Deserialize` would.
    pub fn read(value: Value) -> Option<Self> {
        Some(Self {
            locale: Fields::new(value, &["locale"])?.string("locale")?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisibilityChangedPayload {
    pub visible: bool,
}

impl VisibilityChangedPayload {
    /// Read as its derived `Deserialize` would.
    pub fn read(value: Value) -> Option<Self> {
        match Fields::new(value, &["visible"])?.take("visible")? {
            Value::Bool(visible) => Some(Self { visible }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RevokeReason {
    /// The platform withdrew the bundle.
    Withdrawn,
    /// The platform suspended the plugin.
    Suspended,
    /// The tenant disabled or uninstalled the plugin.
    Disabled,
    /// The catalog no longer selects this bundle for the contribution (a
    /// version change, L3 turned off).
    Replaced,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleRevokedPayload {
    pub reason: RevokeReason,
}

/// `entity.state` payload: the shared state Core relays for an entity of
/// this plugin ([`crate::state::SharedState`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntityStatePayload {
    pub entity: String,
    pub version: u64,
    pub values: Map<String, Value>,
}

/// A decoded request.
#[derive(Debug, Clone, PartialEq)]
pub enum BridgeRequest {
    Hello(HelloParams),
    PluginQuery(PluginQueryParams),
    PluginCommand(PluginCommandParams),
    UiNavigate(UiNavigateParams),
    UiClose(UiCloseParams),
    UiSetTitle(UiSetTitleParams),
    UiToast(UiToastParams),
    UiConfirm(UiConfirmParams),
    UiResize(UiResizeParams),
    UiOpenExternal(UiOpenExternalParams),
    UiSaveState(UiSaveStateParams),
    HostCapability(HostCapabilityParams),
    PeopleResolve(PeopleResolveParams),
    MediaRead(MediaReadParams),
    StorageGet(StorageKeyParams),
    StorageSet(StorageSetParams),
    StorageRemove(StorageKeyParams),
    StorageKeys(StorageKeysParams),
    ClipboardWrite(ClipboardWriteParams),
    EventsSubscribe(EventsSubscribeParams),
    DiagLog(DiagLogParams),
}

fn serialized_len(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

fn text_ok(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.len() <= max && !text.chars().any(char::is_control)
}

fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_BRIDGE_ID_BYTES
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}

fn storage_key_ok(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_BRIDGE_STORAGE_KEY_BYTES
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:/-".contains(&b))
}

fn operation_input_ok(operation: &str, input: &Value) -> Result<(), BridgeErrorCode> {
    if operation.is_empty() || operation.len() > 192 || !input.is_object() {
        return Err(BridgeErrorCode::InvalidParams);
    }
    if serialized_len(input) > MAX_UI_ACTION_INPUT_BYTES {
        return Err(BridgeErrorCode::PayloadTooLarge);
    }
    Ok(())
}

impl BridgeRequest {
    /// Reads and bounds the parameters of `method`.
    pub fn decode(method: BridgeMethod, params: Value) -> Result<Self, BridgeErrorCode> {
        let request = Self::read(method, params).ok_or(BridgeErrorCode::InvalidParams)?;
        request.validate()?;
        Ok(request)
    }

    /// The parameters of `method` as their derived `Deserialize` reads them.
    fn read(method: BridgeMethod, params: Value) -> Option<Self> {
        use BridgeMethod as M;
        let known: &[&str] = match method {
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
        };
        let mut f = Fields::new(params, known)?;
        Some(match method {
            M::HostHello => Self::Hello(HelloParams {
                sdk: f.opt_string("sdk")?,
            }),
            M::PluginQuery => Self::PluginQuery(PluginQueryParams {
                operation: f.string("operation")?,
                input: f.value_or("input", empty_object),
            }),
            M::PluginCommand => Self::PluginCommand(PluginCommandParams {
                operation: f.string("operation")?,
                input: f.value_or("input", empty_object),
                command_id: f.opt_string("commandId")?,
            }),
            M::UiNavigate => Self::UiNavigate(UiNavigateParams {
                page: f.opt_string("page")?,
                arguments: f.opt_object("arguments")?,
                bundle_route: f.opt_string("bundleRoute")?,
            }),
            M::UiClose => Self::UiClose(UiCloseParams {
                result: f.optional("result"),
            }),
            M::UiSetTitle => Self::UiSetTitle(UiSetTitleParams {
                title: f.string("title")?,
            }),
            M::UiToast => Self::UiToast(UiToastParams {
                message: f.string("message")?,
                kind: f.name_or("kind", UiToastKind::parse, UiToastKind::Info)?,
            }),
            M::UiConfirm => Self::UiConfirm(UiConfirmParams {
                title: f.string("title")?,
                message: f.opt_string("message")?,
                confirm_label: f.opt_string("confirmLabel")?,
                destructive: f.bool_or_false("destructive")?,
            }),
            M::UiResize => Self::UiResize(UiResizeParams {
                height: f.u32("height")?,
            }),
            M::UiOpenExternal => Self::UiOpenExternal(UiOpenExternalParams {
                url: f.string("url")?,
            }),
            M::UiSaveState => Self::UiSaveState(UiSaveStateParams {
                state: f.value("state")?,
            }),
            M::HostCapability => Self::HostCapability(HostCapabilityParams {
                name: f.name("name", NativeCapability::parse)?,
                params: f.optional("params"),
            }),
            M::PeopleResolve => Self::PeopleResolve(PeopleResolveParams {
                user_ids: f.strings("userIds")?,
            }),
            M::MediaRead => Self::MediaRead(MediaReadParams {
                file_id: f.string("fileId")?,
            }),
            M::StorageGet => Self::StorageGet(StorageKeyParams {
                key: f.string("key")?,
            }),
            M::StorageRemove => Self::StorageRemove(StorageKeyParams {
                key: f.string("key")?,
            }),
            M::StorageSet => Self::StorageSet(StorageSetParams {
                key: f.string("key")?,
                value: f.value("value")?,
            }),
            M::StorageKeys => Self::StorageKeys(StorageKeysParams {
                prefix: f.opt_string("prefix")?,
            }),
            M::ClipboardWrite => Self::ClipboardWrite(ClipboardWriteParams {
                text: f.string("text")?,
            }),
            M::EventsSubscribe => Self::EventsSubscribe(EventsSubscribeParams {
                topics: {
                    let mut topics = BTreeSet::new();
                    for topic in f.strings("topics")? {
                        topics.insert(BridgeEvent::parse(&topic)?);
                    }
                    topics
                },
            }),
            M::DiagLog => Self::DiagLog(DiagLogParams {
                level: f.name("level", DiagLevel::parse)?,
                message: f.string("message")?,
            }),
        })
    }

    pub fn method(&self) -> BridgeMethod {
        match self {
            Self::Hello(_) => BridgeMethod::HostHello,
            Self::PluginQuery(_) => BridgeMethod::PluginQuery,
            Self::PluginCommand(_) => BridgeMethod::PluginCommand,
            Self::UiNavigate(_) => BridgeMethod::UiNavigate,
            Self::UiClose(_) => BridgeMethod::UiClose,
            Self::UiSetTitle(_) => BridgeMethod::UiSetTitle,
            Self::UiToast(_) => BridgeMethod::UiToast,
            Self::UiConfirm(_) => BridgeMethod::UiConfirm,
            Self::UiResize(_) => BridgeMethod::UiResize,
            Self::UiOpenExternal(_) => BridgeMethod::UiOpenExternal,
            Self::UiSaveState(_) => BridgeMethod::UiSaveState,
            Self::HostCapability(_) => BridgeMethod::HostCapability,
            Self::PeopleResolve(_) => BridgeMethod::PeopleResolve,
            Self::MediaRead(_) => BridgeMethod::MediaRead,
            Self::StorageGet(_) => BridgeMethod::StorageGet,
            Self::StorageSet(_) => BridgeMethod::StorageSet,
            Self::StorageRemove(_) => BridgeMethod::StorageRemove,
            Self::StorageKeys(_) => BridgeMethod::StorageKeys,
            Self::ClipboardWrite(_) => BridgeMethod::ClipboardWrite,
            Self::EventsSubscribe(_) => BridgeMethod::EventsSubscribe,
            Self::DiagLog(_) => BridgeMethod::DiagLog,
        }
    }

    /// The bounds of each method's parameters.
    pub fn validate(&self) -> Result<(), BridgeErrorCode> {
        let ok = match self {
            Self::Hello(params) => params.sdk.as_deref().is_none_or(|sdk| text_ok(sdk, 64)),
            Self::PluginQuery(params) => {
                return operation_input_ok(&params.operation, &params.input);
            }
            Self::PluginCommand(params) => {
                operation_input_ok(&params.operation, &params.input)?;
                params.command_id.as_deref().is_none_or(id_ok)
            }
            Self::UiNavigate(params) => match (&params.page, &params.bundle_route) {
                (Some(page), None) => {
                    !page.is_empty()
                        && page.len() <= 192
                        && params.arguments.as_ref().is_none_or(|arguments| {
                            serialized_len(&Value::Object(arguments.clone()))
                                <= MAX_UI_ACTION_INPUT_BYTES
                        })
                }
                (None, Some(route)) => params.arguments.is_none() && valid_bundle_route(route),
                _ => false,
            },
            Self::UiClose(params) => params
                .result
                .as_ref()
                .is_none_or(|result| serialized_len(result) <= MAX_UI_ACTION_INPUT_BYTES),
            Self::UiSetTitle(params) => text_ok(&params.title, MAX_UI_TEXT_BYTES),
            Self::UiToast(params) => text_ok(&params.message, MAX_BRIDGE_MESSAGE_BYTES),
            Self::UiConfirm(params) => {
                text_ok(&params.title, MAX_UI_TEXT_BYTES)
                    && params
                        .message
                        .as_deref()
                        .is_none_or(|message| text_ok(message, MAX_BRIDGE_MESSAGE_BYTES))
                    && params
                        .confirm_label
                        .as_deref()
                        .is_none_or(|label| text_ok(label, 32))
            }
            Self::UiResize(params) => (1..=MAX_BRIDGE_RESIZE_HEIGHT).contains(&params.height),
            Self::UiOpenExternal(params) => is_https_url(&params.url),
            Self::UiSaveState(params) => {
                serialized_len(&params.state) <= MAX_BRIDGE_SAVED_STATE_BYTES
            }
            Self::HostCapability(params) => params
                .params
                .as_ref()
                .is_none_or(|params| params.is_object() && serialized_len(params) <= 4096),
            Self::PeopleResolve(params) => {
                !params.user_ids.is_empty()
                    && params.user_ids.len() <= MAX_BRIDGE_PEOPLE
                    && params.user_ids.iter().all(|id| id_ok(id))
            }
            Self::MediaRead(params) => id_ok(&params.file_id),
            Self::StorageGet(params) | Self::StorageRemove(params) => storage_key_ok(&params.key),
            Self::StorageSet(params) => {
                storage_key_ok(&params.key)
                    && !params.value.is_null()
                    && serialized_len(&params.value) <= MAX_BRIDGE_STORAGE_VALUE_BYTES
            }
            Self::StorageKeys(params) => params
                .prefix
                .as_deref()
                .is_none_or(|prefix| prefix.is_empty() || storage_key_ok(prefix)),
            Self::ClipboardWrite(params) => {
                !params.text.is_empty() && params.text.len() <= MAX_BRIDGE_CLIPBOARD_BYTES
            }
            Self::EventsSubscribe(params) => !params.topics.is_empty(),
            Self::DiagLog(params) => {
                !params.message.is_empty() && params.message.len() <= MAX_BRIDGE_DIAG_BYTES
            }
        };
        if ok {
            Ok(())
        } else {
            Err(BridgeErrorCode::InvalidParams)
        }
    }
}

/// What an open bundle may do: `bundle.json` intersected with what the
/// tenant approved and the catalog authorizes. The engine builds it once per
/// opening ([`Self::new`]) and checks every request with it
/// ([`Self::authorize`]); hosts do not repeat the check.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeGrant {
    /// `bundle.json` permissions ∩ the catalog entry's approved permissions.
    pub permissions: BTreeSet<BridgePermission>,
    /// `bundle.json` operations ∩ the catalog's `operations`, without the
    /// catalog's `transientOperations`.
    pub operations: BTreeSet<String>,
    /// [`Self::operations`] ∩ the catalog's `queryOperations`: what
    /// `plugin.query` may call.
    pub query_operations: BTreeSet<String>,
    /// `bundle.json` capabilities ∩ the catalog's `nativeCapabilities` ∩
    /// what the host offers.
    pub capabilities: BTreeSet<NativeCapability>,
}

impl BridgeGrant {
    /// The grant of `bundle` (read from the verified archive) as `entry`
    /// (the catalog's [`WebBundleDecl`], with its approved permissions) of
    /// `plugin` (the catalog plugin) allows it on a host offering
    /// `host_native` (the wire names of the descriptor's `native`).
    pub fn new(
        bundle: &BundleManifest,
        entry: &WebBundleDecl,
        plugin: &ClientPlugin,
        host_native: &BTreeSet<String>,
    ) -> Self {
        // Inserted one by one: collecting a set sorts first, a copy of the
        // sorting code for each element type.
        let mut grant = Self::default();
        for operation in &bundle.operations {
            if plugin.operations.contains(operation)
                && !plugin.transient_operations.contains(operation)
            {
                if plugin.query_operations.contains(operation) {
                    grant.query_operations.insert(operation.clone());
                }
                grant.operations.insert(operation.clone());
            }
        }
        for permission in &bundle.permissions {
            if entry.permissions.contains(permission) {
                grant.permissions.insert(*permission);
            }
        }
        for capability in &bundle.capabilities {
            if plugin.extensions.native_capabilities.contains(capability)
                && host_native.contains(capability.name())
            {
                grant.capabilities.insert(*capability);
            }
        }
        grant
    }

    /// Step 5 of the check order: the method's permission, then the
    /// operation (`plugin.query` a granted query operation, `plugin.command`
    /// a granted operation, never a transient one) or the capability.
    pub fn authorize(&self, request: &BridgeRequest) -> Result<(), BridgeErrorCode> {
        let allowed = match request.method().need() {
            BridgeMethodNeed::Nothing => true,
            BridgeMethodNeed::Permission(permission) => self.permissions.contains(&permission),
            BridgeMethodNeed::Capability => match request {
                BridgeRequest::HostCapability(params) => self.capabilities.contains(&params.name),
                _ => false,
            },
        } && match request {
            BridgeRequest::PluginQuery(params) => self.query_operations.contains(&params.operation),
            BridgeRequest::PluginCommand(params) => self.operations.contains(&params.operation),
            _ => true,
        };
        if allowed {
            Ok(())
        } else {
            Err(BridgeErrorCode::PermissionDenied)
        }
    }
}
