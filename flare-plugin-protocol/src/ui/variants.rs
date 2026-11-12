//! Contribution variants: one contribution, several presentations, each with
//! what it needs from the client ([`UiRequires`]). Core picks the first
//! variant a client's descriptor satisfies and sends it as a plain
//! contribution ([`UiContribution::select`]); when none fits, or the build is
//! below `minClient`, it sends an upgrade placeholder ([`UiUpgrade`]).
//!
//! A variant overrides the presentation only: `schema` (with its
//! `presentation`), `detail`, `source`, `viewerState`, `availability`,
//! `summary` and `bundle`. The identity of the contribution, `kind`, `id`,
//! `requiredPermissions`, `entityPath`, `entityLifecycle`, `forwardable`,
//! `audience`, `order` and `icon`, is the same whichever variant renders, so
//! a card binds the same entity with the same permissions everywhere. A
//! field the variant leaves out is the base contribution's; `bundle` is
//! only ever a variant's.
use super::{
    ClientUiDescriptor, UiActionTarget, UiDetail, UiDocument, UiEntryAvailability, UiEntrySummary,
    UiPageSource, UiTranslation, UiViewerState, UiVocabulary, base_vocabulary,
};
use crate::{
    ProtocolError, Result,
    manifest::{UiContribution, UiContributionKind},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Variants of one contribution.
pub const MAX_UI_VARIANTS: usize = 8;
/// The bridge version a bundle reference needs at least.
pub const MIN_UI_BUNDLE_BRIDGE: u32 = 1;
/// Bytes of a bundle route.
pub const MAX_UI_BUNDLE_ROUTE_BYTES: usize = 256;

/// A web bundle (L3) of the plugin and the route it opens at. On a page the
/// page is the bundle; on a composer action the form is; on a card the card
/// stays declarative and its detail ("view all") opens the bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiBundleRef {
    /// `[a-z0-9][a-z0-9-]{0,63}`.
    pub id: String,
    /// `/`-rooted path inside the bundle, `[A-Za-z0-9/_.-]`, no empty, `.`
    /// or `..` segment past the root, at most 256 bytes.
    pub route: String,
}
fn valid_bundle_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
impl UiBundleRef {
    pub fn is_valid(&self) -> bool {
        valid_bundle_id(&self.id)
            && self.route.len() <= MAX_UI_BUNDLE_ROUTE_BYTES
            && self.route.starts_with('/')
            && self
                .route
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b))
            && (self.route == "/"
                || self.route[1..]
                    .split('/')
                    .all(|segment| !segment.is_empty() && segment != "." && segment != ".."))
    }
}

/// A variant's need of a web bundle runtime: the bundle it opens and the
/// bridge version it speaks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiRequiresBundle {
    pub id: String,
    pub bridge: u32,
}

/// What a variant needs from the client: each set a subset of the
/// descriptor's, and a bundle runtime speaking at least `bundle.bridge`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiRequires {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub blocks: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub inputs: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub formats: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub actions: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub native: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub renderers: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub kinds: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<UiRequiresBundle>,
}
impl UiRequires {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
    fn sets(&self) -> [&BTreeSet<String>; 7] {
        [
            &self.blocks,
            &self.inputs,
            &self.formats,
            &self.actions,
            &self.native,
            &self.renderers,
            &self.kinds,
        ]
    }
    fn sets_mut(&mut self) -> [&mut BTreeSet<String>; 7] {
        [
            &mut self.blocks,
            &mut self.inputs,
            &mut self.formats,
            &mut self.actions,
            &mut self.native,
            &mut self.renderers,
            &mut self.kinds,
        ]
    }
    /// Names `[A-Za-z0-9._-]{1,64}`, at most 128 per set, and a valid
    /// bundle id with a positive bridge.
    pub fn is_valid(&self) -> bool {
        self.sets().into_iter().all(|set| {
            set.len() <= super::MAX_UI_VOCABULARY_NAMES
                && set.iter().all(|name| {
                    !name.is_empty()
                        && name.len() <= super::MAX_UI_VOCABULARY_NAME_BYTES
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                })
        }) && self
            .bundle
            .as_ref()
            .is_none_or(|b| valid_bundle_id(&b.id) && b.bridge >= MIN_UI_BUNDLE_BRIDGE)
    }
    /// Whether a client with `descriptor` meets every requirement.
    pub fn satisfied_by(&self, descriptor: &ClientUiDescriptor) -> bool {
        let offered = UiRequires::from_vocabulary(&descriptor.vocabulary());
        self.sets()
            .into_iter()
            .zip(offered.sets())
            .all(|(needed, offered)| needed.is_subset(offered))
            && self.bundle.as_ref().is_none_or(|needed| {
                descriptor
                    .bundle
                    .as_ref()
                    .is_some_and(|runtime| runtime.bridge >= needed.bridge)
            })
    }
    fn from_vocabulary(vocabulary: &UiVocabulary) -> Self {
        Self {
            blocks: vocabulary.blocks.clone(),
            inputs: vocabulary.inputs.clone(),
            formats: vocabulary.formats.clone(),
            actions: vocabulary.actions.clone(),
            native: vocabulary.native.clone(),
            renderers: vocabulary.renderers.clone(),
            kinds: vocabulary.kinds.clone(),
            bundle: None,
        }
    }
    /// Add `other`'s requirements; the higher bridge of the same bundle wins.
    pub fn union(&mut self, other: &UiRequires) {
        for (mine, theirs) in self.sets_mut().into_iter().zip(other.sets()) {
            mine.extend(theirs.iter().cloned());
        }
        if let Some(theirs) = &other.bundle {
            if self.bundle.is_none() {
                self.bundle = Some(theirs.clone());
            } else if let Some(mine) = self.bundle.as_mut().filter(|mine| mine.id == theirs.id) {
                mine.bridge = mine.bridge.max(theirs.bridge);
            }
        }
    }
    /// The requirements `vocabulary` does not already meet: every client
    /// with a descriptor reads the [`base_vocabulary`], so an author writes
    /// only what goes beyond it. The bundle stays.
    pub fn beyond(&self, vocabulary: &UiVocabulary) -> UiRequires {
        let known = Self::from_vocabulary(vocabulary);
        let mut rest = self.clone();
        for (mine, theirs) in rest.sets_mut().into_iter().zip(known.sets()) {
            mine.retain(|name| !theirs.contains(name));
        }
        rest
    }
    /// What these requirements need that `written` does not state: the
    /// names it lacks, and the bundle when `written` names another one or a
    /// lower bridge. Empty when `written` covers them.
    pub fn missing_from(&self, written: &UiRequires) -> UiRequires {
        let mut missing = self.clone();
        for (mine, theirs) in missing.sets_mut().into_iter().zip(written.sets()) {
            mine.retain(|name| !theirs.contains(name));
        }
        if let (Some(needed), Some(stated)) = (&self.bundle, &written.bundle)
            && needed.id == stated.id
            && stated.bridge >= needed.bridge
        {
            missing.bundle = None;
        }
        missing
    }
}

/// One presentation of a contribution (see the module documentation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiContributionVariant {
    /// `[A-Za-z0-9_-]{1,32}`, unique in the contribution; the catalog names
    /// the chosen one ([`UiContribution::variant`]).
    pub id: String,
    /// What the variant needs, beyond the [`base_vocabulary`]: at least what
    /// its content uses ([`Self::missing_requires`] is empty), and possibly
    /// more, such as a native capability its flow cannot do without.
    #[serde(default, skip_serializing_if = "UiRequires::is_empty")]
    pub requires: UiRequires,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<UiDocument>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<UiDetail>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<UiPageSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer_state: Option<UiViewerState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability: Option<UiEntryAvailability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<UiEntrySummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<UiBundleRef>,
}
impl UiContributionVariant {
    /// What the variant's content uses, expanded over `base`
    /// ([`UiContribution::derived_requires`]).
    pub fn derived_requires(&self, base: &UiContribution) -> UiRequires {
        base.expand(self).derived_requires()
    }
    /// What the variant's content uses beyond the [`base_vocabulary`] and
    /// its `requires` does not state. Publication refuses a variant for which
    /// this is not empty.
    pub fn missing_requires(&self, base: &UiContribution) -> UiRequires {
        self.derived_requires(base)
            .beyond(&base_vocabulary())
            .missing_from(&self.requires)
    }
}

/// An upgrade placeholder: the catalog form of a contribution this client
/// build cannot show. Its document has the title (and title translations)
/// of the contribution and nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiUpgrade {
    /// The lowest build of the client's platform that shows it (the
    /// contribution's `minClient`), when the plugin names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_build: Option<u64>,
}

/// What [`UiContribution::select`] gives a client.
#[derive(Debug, Clone, PartialEq)]
pub enum Selected {
    /// A variant expanded into a plain contribution (`variant` names it when
    /// the contribution has variants). `index` is its position: 0 is the
    /// first choice, anything else a fallback.
    Variant {
        contribution: UiContribution,
        index: usize,
    },
    /// The upgrade placeholder ([`UiContribution::upgrade`]).
    Upgrade(UiContribution),
}
impl Selected {
    pub fn contribution(&self) -> &UiContribution {
        match self {
            Self::Variant { contribution, .. } | Self::Upgrade(contribution) => contribution,
        }
    }
    pub fn into_contribution(self) -> UiContribution {
        match self {
            Self::Variant { contribution, .. } | Self::Upgrade(contribution) => contribution,
        }
    }
    /// `variant`, `fallback` or `upgrade`.
    pub fn outcome(&self) -> &'static str {
        match self {
            Self::Variant { index: 0, .. } => "variant",
            Self::Variant { .. } => "fallback",
            Self::Upgrade(_) => "upgrade",
        }
    }
}

fn target_kind(target: &UiActionTarget) -> &'static str {
    match target {
        UiActionTarget::Operation { .. } => "operation",
        UiActionTarget::Page { .. } => "page",
        UiActionTarget::Native { .. } => "native",
    }
}

impl UiContribution {
    /// The contribution as `variant` presents it: the variant's fields over
    /// this one's, no variants or `minClient`, and `variant` naming it.
    pub fn expand(&self, variant: &UiContributionVariant) -> UiContribution {
        let mut expanded = self.clone();
        expanded.variants.clear();
        expanded.min_client.clear();
        expanded.upgrade = None;
        expanded.variant = Some(variant.id.clone());
        if let Some(schema) = &variant.schema {
            expanded.schema = schema.clone();
        }
        if variant.detail.is_some() {
            expanded.detail = variant.detail.clone();
        }
        if variant.source.is_some() {
            expanded.source = variant.source.clone();
        }
        if variant.viewer_state.is_some() {
            expanded.viewer_state = variant.viewer_state.clone();
        }
        if variant.availability.is_some() {
            expanded.availability = variant.availability.clone();
        }
        if variant.summary.is_some() {
            expanded.summary = variant.summary.clone();
        }
        expanded.bundle = variant.bundle.clone();
        expanded
    }

    /// Every presentation of the contribution, in order, each with the
    /// requirements its author wrote: each variant expanded, or the
    /// contribution itself (no variants, nothing written) when it has none.
    pub fn expansions(&self) -> Vec<(UiContribution, UiRequires)> {
        if self.variants.is_empty() {
            let mut only = self.clone();
            only.min_client.clear();
            return vec![(only, UiRequires::default())];
        }
        self.variants
            .iter()
            .map(|variant| (self.expand(variant), variant.requires.clone()))
            .collect()
    }

    /// What this contribution's content uses, read from it: its kind, the
    /// kinds of every block (sections' included), input and field format of
    /// its document and detail document, its action target kinds, its
    /// renderer, and its bundle (bridge 1). Native capabilities are not
    /// derived: a client without one drops the action (Core does it in
    /// [`Self::select`]), so a variant names them in `requires` only when it
    /// cannot do without them. Its own `variants` are not looked at.
    pub fn derived_requires(&self) -> UiRequires {
        let mut requires = UiRequires::default();
        requires.kinds.insert(self.kind.name().to_owned());
        let documents =
            std::iter::once(&self.schema).chain(self.detail.as_ref().map(|d| &d.document));
        for document in documents {
            if let Some(presentation) = &document.presentation {
                requires.renderers.insert(presentation.renderer.clone());
            }
            for field in &document.fields {
                if let Some(format) = &field.format {
                    requires.formats.insert(format.kind_name().to_owned());
                }
            }
            for block in document.all_blocks() {
                requires.blocks.insert(block.kind_name().to_owned());
                for format in block.formats() {
                    requires.formats.insert(format.kind_name().to_owned());
                }
            }
            for input in &document.inputs {
                requires.inputs.insert(input.kind.name().to_owned());
            }
            for action in &document.actions {
                requires
                    .actions
                    .insert(target_kind(&action.target).to_owned());
            }
        }
        requires.bundle = self.bundle.as_ref().map(|bundle| UiRequiresBundle {
            id: bundle.id.clone(),
            bridge: MIN_UI_BUNDLE_BRIDGE,
        });
        requires
    }

    /// What Core sends a client with `descriptor` for this contribution:
    /// - an upgrade placeholder when `minClient` names a build above the
    ///   client's for its platform;
    /// - else the first presentation ([`Self::expansions`]) whose written
    ///   and derived requirements the descriptor satisfies, with every native
    ///   action the descriptor does not offer removed
    ///   ([`Self::retain_targets`]);
    /// - else an upgrade placeholder with the platform's `minClient`, if any.
    pub fn select(&self, descriptor: &ClientUiDescriptor) -> Selected {
        let floor = self.min_client.get(&descriptor.platform).copied();
        if floor.is_some_and(|floor| descriptor.app_build < floor) {
            return Selected::Upgrade(self.upgrade_placeholder(floor));
        }
        for (index, (mut contribution, written)) in self.expansions().into_iter().enumerate() {
            let mut needs = contribution.derived_requires();
            needs.union(&written);
            if !needs.satisfied_by(descriptor) {
                continue;
            }
            contribution.retain_targets(|target| match target {
                UiActionTarget::Native { capability, .. } => {
                    descriptor.native.contains(capability.name())
                }
                _ => true,
            });
            return Selected::Variant {
                contribution,
                index,
            };
        }
        Selected::Upgrade(self.upgrade_placeholder(floor))
    }

    /// The upgrade placeholder of this contribution: its identity, a
    /// document holding only its title and title translations, and
    /// `upgrade` with `min_build`. Nothing in it calls the plugin.
    pub fn upgrade_placeholder(&self, min_build: Option<u64>) -> UiContribution {
        let translations: BTreeMap<String, UiTranslation> = self
            .schema
            .translations
            .iter()
            .map(|(locale, translation)| {
                (
                    locale.clone(),
                    UiTranslation {
                        title: translation.title.clone(),
                        fields: Vec::new(),
                        actions: BTreeMap::new(),
                        values: BTreeMap::new(),
                        texts: BTreeMap::new(),
                    },
                )
            })
            .collect();
        UiContribution {
            kind: self.kind,
            id: self.id.clone(),
            schema_version: self.schema_version,
            schema: UiDocument {
                title: self.schema.title.clone(),
                presentation: None,
                fields: Vec::new(),
                actions: Vec::new(),
                blocks: Vec::new(),
                inputs: Vec::new(),
                translations,
            },
            required_permissions: self.required_permissions.clone(),
            icon: self.icon.clone(),
            order: self.order,
            entity_path: self.entity_path.clone(),
            forwardable: self.forwardable,
            audience: self.audience.clone(),
            detail: None,
            viewer_state: None,
            entity_lifecycle: self.entity_lifecycle.clone(),
            summary: None,
            availability: None,
            source: None,
            preview: None,
            variants: Vec::new(),
            min_client: BTreeMap::new(),
            variant: None,
            upgrade: Some(UiUpgrade { min_build }),
            bundle: None,
        }
    }

    /// The publication rules of variants and the catalog-only fields:
    /// - `variant` and `upgrade` are set by Core only, and `bundle` belongs
    ///   to a variant: none of the three in a manifest;
    /// - at most [`MAX_UI_VARIANTS`] variants with unique valid ids, each
    ///   with valid `requires` that state at least what its content uses
    ///   beyond the base vocabulary ([`UiContributionVariant::missing_requires`]);
    /// - a bundle only on a page, composer-action or message-content
    ///   variant, valid, never on the last variant (a declarative variant
    ///   follows it), and `requires.bundle` only with a bundle of that id;
    /// - `minClient` builds at most `i64::MAX`.
    ///
    /// Each expansion is then checked like any contribution by the manifest.
    pub(crate) fn validate_variants(&self) -> Result<()> {
        let refuse = |reason: &'static str| Err(ProtocolError::Denied(reason));
        if self.variant.is_some() || self.upgrade.is_some() || self.bundle.is_some() {
            return refuse("catalog-only contribution field in a manifest");
        }
        if self
            .min_client
            .values()
            .any(|build| *build > i64::MAX as u64)
        {
            return refuse("invalid contribution minClient");
        }
        if self.variants.len() > MAX_UI_VARIANTS {
            return refuse("too many contribution variants");
        }
        let bundle_kind = matches!(
            self.kind,
            UiContributionKind::Page
                | UiContributionKind::ComposerAction
                | UiContributionKind::MessageContent
        );
        let mut ids = BTreeSet::new();
        for (index, variant) in self.variants.iter().enumerate() {
            let last = index + 1 == self.variants.len();
            let valid_id = !variant.id.is_empty()
                && variant.id.len() <= 32
                && variant
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b));
            if !valid_id || !ids.insert(variant.id.as_str()) || !variant.requires.is_valid() {
                return refuse("invalid contribution variant");
            }
            match (&variant.bundle, &variant.requires.bundle) {
                (Some(bundle), needed)
                    if bundle_kind
                        && !last
                        && bundle.is_valid()
                        && needed.as_ref().is_none_or(|n| n.id == bundle.id) => {}
                (None, None) => {}
                _ => return refuse("invalid contribution variant bundle"),
            }
            if !variant.missing_requires(self).is_empty() {
                return refuse("contribution variant requires less than it uses");
            }
        }
        Ok(())
    }
}
