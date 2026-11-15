//! What a client can render: the UI vocabulary and the descriptor a client
//! sends with every catalog read.
//!
//! The engine knows the vocabulary of this crate ([`engine_vocabulary`]);
//! the host hands it the vocabulary of its design kit and its native
//! allowlist. The descriptor is their intersection
//! ([`UiVocabulary::for_host`]) plus the build identity, and Core selects
//! the contribution variants that fit it ([`super::UiContribution::select`]).
//! Every set holds names, never types, so a descriptor naming something
//! this crate does not know still reads.
use super::{NativeCapability, UiActionTarget, UiBlock, UiFieldFormat, UiInputKind};
use crate::{
    ProtocolError, Result,
    manifest::{ClientPlatform, UiContributionKind},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// The protocol vocabulary version: version 1 is the vocabulary before
/// descriptors existed ([`base_vocabulary`]); every batch of new names adds
/// one and is listed in the changelog. Logs and caches read it; selection
/// looks at the sets only.
pub const UI_VOCABULARY_VERSION: u32 = 2;
/// Serialized size of a descriptor.
pub const MAX_UI_DESCRIPTOR_BYTES: usize = 4096;
/// Names in one set of a descriptor or a vocabulary.
pub const MAX_UI_VOCABULARY_NAMES: usize = 128;
/// Bytes of one name.
pub const MAX_UI_VOCABULARY_NAME_BYTES: usize = 64;

/// A set of UI names a party knows: blocks, inputs, formats, action target
/// kinds, native capabilities, compiled-in renderers and contribution kinds.
/// The design kits publish theirs as `PLUGIN_UI_VOCABULARY`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UiVocabulary {
    pub version: u32,
    #[serde(default)]
    pub blocks: BTreeSet<String>,
    #[serde(default)]
    pub inputs: BTreeSet<String>,
    #[serde(default)]
    pub formats: BTreeSet<String>,
    #[serde(default)]
    pub actions: BTreeSet<String>,
    #[serde(default)]
    pub native: BTreeSet<String>,
    #[serde(default)]
    pub renderers: BTreeSet<String>,
    #[serde(default)]
    pub kinds: BTreeSet<String>,
}

fn names<'a>(items: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    items.into_iter().map(str::to_owned).collect()
}

/// Every name this crate reads, from the enums themselves: the tag lists of
/// [`UiBlock`], [`UiFieldFormat`] and [`UiActionTarget`], and every variant
/// of [`UiInputKind`], [`NativeCapability`] and [`UiContributionKind`].
/// Renderers are design-kit components the engine does not know: none.
pub fn engine_vocabulary() -> UiVocabulary {
    UiVocabulary {
        version: UI_VOCABULARY_VERSION,
        blocks: names(UiBlock::KINDS.iter().copied()),
        inputs: names(UiInputKind::ALL.iter().map(|k| k.name())),
        formats: names(UiFieldFormat::KINDS.iter().copied()),
        actions: names(UiActionTarget::KINDS.iter().copied()),
        native: names(NativeCapability::ALL.iter().map(|c| c.name())),
        renderers: BTreeSet::new(),
        kinds: names(UiContributionKind::ALL.iter().map(|k| k.name())),
    }
}

/// Vocabulary version 1: what every client reads, from before descriptors.
/// A contribution's last variant should stay within it; requirements inside
/// it need not be written ([`super::UiRequires::beyond`]).
pub fn base_vocabulary() -> UiVocabulary {
    UiVocabulary {
        version: 1,
        blocks: names([
            "text",
            "fields",
            "choices",
            "entries",
            "people",
            "status",
            "progress",
            "time",
            "divider",
            "note",
            "rows",
            "protected-media",
        ]),
        inputs: names([
            "text", "password", "textarea", "number", "toggle", "select", "datetime", "options",
            "members", "file",
        ]),
        formats: names(["money", "enum", "person", "reference", "time"]),
        actions: names(["operation", "page", "native"]),
        native: names([
            "selectImage",
            "selectFile",
            "camera",
            "microphone",
            "clipboardWrite",
        ]),
        renderers: BTreeSet::new(),
        kinds: names([
            "message-content",
            "message-action",
            "conversation-menu",
            "composer-action",
            "page",
            "account-entry",
        ]),
    }
}

fn valid_name_set(set: &BTreeSet<String>) -> bool {
    set.len() <= MAX_UI_VOCABULARY_NAMES
        && set.iter().all(|name| {
            !name.is_empty()
                && name.len() <= MAX_UI_VOCABULARY_NAME_BYTES
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}

impl UiVocabulary {
    pub fn validate(&self) -> Result<()> {
        if self.version == 0
            || ![
                &self.blocks,
                &self.inputs,
                &self.formats,
                &self.actions,
                &self.native,
                &self.renderers,
                &self.kinds,
            ]
            .into_iter()
            .all(valid_name_set)
        {
            return Err(ProtocolError::Denied("invalid UI vocabulary"));
        }
        Ok(())
    }

    /// The vocabulary a client renders with: this (the engine's) vocabulary
    /// intersected with the host's design-kit vocabulary, set by set. The
    /// renderers are the host's (the engine renders none); `native` is also
    /// intersected with the host's allowlist. Without a host vocabulary the
    /// engine's sets stand and `native` and `renderers` are empty: nothing
    /// the host has not offered is reported. The version stays this one.
    pub fn for_host(
        &self,
        host: Option<&UiVocabulary>,
        native_allowlist: &BTreeSet<NativeCapability>,
    ) -> UiVocabulary {
        let Some(host) = host else {
            return UiVocabulary {
                native: BTreeSet::new(),
                renderers: BTreeSet::new(),
                ..self.clone()
            };
        };
        let both = |a: &BTreeSet<String>, b: &BTreeSet<String>| -> BTreeSet<String> {
            a.intersection(b).cloned().collect()
        };
        let allowed: BTreeSet<String> = native_allowlist
            .iter()
            .map(|c| c.name().to_owned())
            .collect();
        UiVocabulary {
            version: self.version,
            blocks: both(&self.blocks, &host.blocks),
            inputs: both(&self.inputs, &host.inputs),
            formats: both(&self.formats, &host.formats),
            actions: both(&self.actions, &host.actions),
            native: both(&both(&self.native, &host.native), &allowed),
            renderers: host.renderers.clone(),
            kinds: both(&self.kinds, &host.kinds),
        }
    }

    /// Whether every name of `other` is in this vocabulary (versions aside).
    pub fn contains(&self, other: &UiVocabulary) -> bool {
        other.blocks.is_subset(&self.blocks)
            && other.inputs.is_subset(&self.inputs)
            && other.formats.is_subset(&self.formats)
            && other.actions.is_subset(&self.actions)
            && other.native.is_subset(&self.native)
            && other.renderers.is_subset(&self.renderers)
            && other.kinds.is_subset(&self.kinds)
    }
}

/// The web bundle runtime a host offers (L3): present only when the host has
/// one and its web engine meets the platform floor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BundleRuntime {
    /// Bridge protocol version the host speaks (`>= 1`).
    pub bridge: u32,
    /// `webview`, `wkwebview`, `browser`, ... (`[a-z0-9-]{1,32}`).
    pub engine: String,
    /// Major version of that engine: Android System WebView or Chrome, the
    /// iOS major version, or the browser's.
    pub engine_major: u32,
}

/// What a client can render, sent as the payload of `flare.plugin.ui.catalog`
/// (`{"descriptor": ...}`): the build identity and the sets of
/// [`UiVocabulary::for_host`]. No device identifier. Core selects variants
/// with it and caches the selection by [`Self::hash`].
///
/// Unknown fields are ignored: a newer client may say more than this Core
/// reads, and the hash covers what was read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientUiDescriptor {
    /// [`UiVocabulary::version`] of the client's engine.
    pub vocabulary: u32,
    pub platform: ClientPlatform,
    /// The build number of the application.
    pub app_build: u64,
    /// The distribution channel (`play`, `appstore`, `web`, ...):
    /// `[a-z0-9._-]{1,32}`.
    pub distribution: String,
    #[serde(default)]
    pub blocks: BTreeSet<String>,
    #[serde(default)]
    pub inputs: BTreeSet<String>,
    #[serde(default)]
    pub formats: BTreeSet<String>,
    #[serde(default)]
    pub actions: BTreeSet<String>,
    #[serde(default)]
    pub native: BTreeSet<String>,
    #[serde(default)]
    pub renderers: BTreeSet<String>,
    #[serde(default)]
    pub kinds: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<BundleRuntime>,
}

impl ClientUiDescriptor {
    /// The descriptor of a client rendering with `vocabulary`.
    pub fn new(
        vocabulary: &UiVocabulary,
        platform: ClientPlatform,
        app_build: u64,
        distribution: impl Into<String>,
        bundle: Option<BundleRuntime>,
    ) -> Self {
        Self {
            vocabulary: vocabulary.version,
            platform,
            app_build,
            distribution: distribution.into(),
            blocks: vocabulary.blocks.clone(),
            inputs: vocabulary.inputs.clone(),
            formats: vocabulary.formats.clone(),
            actions: vocabulary.actions.clone(),
            native: vocabulary.native.clone(),
            renderers: vocabulary.renderers.clone(),
            kinds: vocabulary.kinds.clone(),
            bundle,
        }
    }

    /// The sets as a vocabulary.
    pub fn vocabulary(&self) -> UiVocabulary {
        UiVocabulary {
            version: self.vocabulary,
            blocks: self.blocks.clone(),
            inputs: self.inputs.clone(),
            formats: self.formats.clone(),
            actions: self.actions.clone(),
            native: self.native.clone(),
            renderers: self.renderers.clone(),
            kinds: self.kinds.clone(),
        }
    }

    /// A descriptor as a client sent it: `raw` at most
    /// [`MAX_UI_DESCRIPTOR_BYTES`] as sent (unknown fields and repeated set
    /// entries count, so the bound holds for what is parsed, not only for
    /// what is kept), then read and validated ([`Self::validate`]).
    pub fn from_value(raw: &serde_json::Value) -> Result<Self> {
        let refuse = || ProtocolError::Denied("invalid client UI descriptor");
        if serde_json::to_vec(raw).map_or(true, |v| v.len() > MAX_UI_DESCRIPTOR_BYTES) {
            return Err(refuse());
        }
        let descriptor = Self::deserialize(raw).map_err(|_| refuse())?;
        descriptor.validate()?;
        Ok(descriptor)
    }

    /// At most [`MAX_UI_DESCRIPTOR_BYTES`] serialized, a positive vocabulary
    /// version, a channel of `[a-z0-9._-]{1,32}`, sets of at most 128 names
    /// of `[A-Za-z0-9._-]{1,64}`, and a bundle runtime with a positive bridge
    /// and an engine of `[a-z0-9-]{1,32}`.
    pub fn validate(&self) -> Result<()> {
        let lower = |s: &str, extra: &[u8]| {
            !s.is_empty()
                && s.len() <= 32
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || extra.contains(&b))
        };
        if self.vocabulary == 0
            || self.app_build > i64::MAX as u64
            || !lower(&self.distribution, b"._-")
            || self
                .bundle
                .as_ref()
                .is_some_and(|b| b.bridge == 0 || !lower(&b.engine, b"-"))
            || self.vocabulary().validate().is_err()
            || serde_json::to_vec(self).map_or(true, |v| v.len() > MAX_UI_DESCRIPTOR_BYTES)
        {
            return Err(ProtocolError::Denied("invalid client UI descriptor"));
        }
        Ok(())
    }

    /// `sha256:<hex>` of the canonical form: this type serialized again,
    /// fields in declaration order and sets sorted, so equal descriptors
    /// hash equal however they were written and whatever unknown fields
    /// they carried.
    pub fn hash(&self) -> String {
        let canonical = serde_json::to_vec(self).unwrap_or_default();
        format!("sha256:{:x}", Sha256::digest(&canonical))
    }
}
