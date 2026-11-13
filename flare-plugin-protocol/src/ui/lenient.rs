//! Tolerant reading of one catalog contribution: the second line of defence
//! behind Core's variant selection, for a stale descriptor, a selection
//! defect or another Core.
//!
//! The protocol types stay strict (`deny_unknown_fields`, closed enums): a
//! manifest is published strictly. A client reads a catalog contribution
//! through [`prepare_contribution`] first, which takes out of the raw JSON
//! what the client's vocabulary does not know, and only then reads the
//! types:
//! - a block of an unknown kind, or of a known kind with a field this client
//!   does not know, goes when it is not `required`, together with the
//!   actions it owns; a `required` one makes the contribution unreadable. A
//!   known block is never read with the unknown field ignored: the field may
//!   be a condition (like `hiddenWhen`) that hides content;
//! - a section left without blocks goes the same way;
//! - a field format this client does not know goes, so the value reads as
//!   its plain text, which every resolved value carries;
//! - a renderer this client does not know goes, so the generic blocks show;
//! - an input this client does not know makes the contribution unreadable:
//!   the forms that collect it would be incomplete;
//! - an action this client cannot read or run (an unknown target, a native
//!   capability the host does not offer) goes, and every reference to it
//!   with it, translations included ([`super::UiDocument::retain_actions`],
//!   the function Core trims with).
//!
//! An unreadable contribution is not dropped silently: it becomes an upgrade
//! placeholder built from its identity ([`Prepared::Upgrade`]).
use super::{
    UiAction, UiActionTarget, UiBlock, UiDocument, UiFieldFormat, UiInput, UiTranslation,
    UiUpgrade, UiVocabulary, valid_text,
};
use crate::manifest::{DEFAULT_UI_ORDER, UiContribution, UiContributionKind, is_ui_icon_name};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// What a client makes of one catalog contribution.
#[derive(Debug, Clone, PartialEq)]
pub enum Prepared {
    /// Readable: the contribution with what this client does not know taken
    /// out. Not yet validated: the catalog's own checks still apply.
    Ready(UiContribution),
    /// Shown as `upgrade_required`: Core's upgrade placeholder as sent, or
    /// one built from the identity of a contribution this client cannot
    /// read (`minBuild` from its `upgrade`, if Core sent one).
    Upgrade(UiContribution),
    /// Not even its identity reads (no object, a kind this client does not
    /// know, no id or schema version): nothing to show.
    Dropped,
}

/// Read one raw catalog contribution with `vocabulary`, the client's
/// ([`super::UiVocabulary::for_host`]); see the module documentation.
pub fn prepare_contribution(raw: &Value, vocabulary: &UiVocabulary) -> Prepared {
    let Some(object) = raw.as_object() else {
        return Prepared::Dropped;
    };
    let Some(kind) = object
        .get("kind")
        .and_then(Value::as_str)
        .filter(|kind| vocabulary.kinds.contains(*kind))
        .and_then(|kind| crate::json::from_value::<UiContributionKind>(Value::from(kind)).ok())
    else {
        return Prepared::Dropped;
    };
    let Some(fallback) = placeholder(object, kind) else {
        return Prepared::Dropped;
    };
    let mut cleaned = raw.clone();
    if prepare_documents(&mut cleaned, vocabulary).is_none() {
        return Prepared::Upgrade(fallback);
    }
    let Ok(mut contribution) = crate::json::from_value::<UiContribution>(cleaned) else {
        return Prepared::Upgrade(fallback);
    };
    contribution.schema.retain_actions(&mut |_| true);
    if let Some(detail) = &mut contribution.detail {
        detail.document.retain_actions(&mut |_| true);
    }
    if contribution.upgrade.is_some() {
        Prepared::Upgrade(contribution)
    } else {
        Prepared::Ready(contribution)
    }
}

/// The documents of a raw contribution: `schema` and `detail.document`.
fn prepare_documents(raw: &mut Value, vocabulary: &UiVocabulary) -> Option<()> {
    prepare_document(raw.get_mut("schema")?, vocabulary)?;
    if let Some(detail) = raw.get_mut("detail").filter(|detail| !detail.is_null()) {
        prepare_document(detail.get_mut("document")?, vocabulary)?;
    }
    Some(())
}

fn prepare_document(document: &mut Value, vocabulary: &UiVocabulary) -> Option<()> {
    let document = document.as_object_mut()?;
    let renderer_known = document
        .get("presentation")
        .and_then(|p| p.get("renderer"))
        .and_then(Value::as_str)
        .is_none_or(|renderer| vocabulary.renderers.contains(renderer));
    if !renderer_known {
        document.remove("presentation");
    }
    if let Some(Value::Array(fields)) = document.get_mut("fields") {
        for field in fields {
            degrade_format(field, "format", vocabulary);
        }
    }
    let mut dropped = BTreeSet::new();
    if let Some(Value::Array(blocks)) = document.get_mut("blocks") {
        prepare_blocks(blocks, vocabulary, &mut dropped)?;
        // An action a kept block owns too stays.
        let mut kept = BTreeSet::new();
        for block in blocks.iter() {
            owned_actions(block, &mut kept);
        }
        dropped.retain(|id| !kept.contains(id));
    }
    if let Some(Value::Array(inputs)) = document.get("inputs") {
        for input in inputs {
            let known = input
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| vocabulary.inputs.contains(kind));
            if !known || crate::json::from_value::<UiInput>(input.clone()).is_err() {
                return None;
            }
        }
    }
    if let Some(Value::Array(actions)) = document.get_mut("actions") {
        actions.retain(|action| {
            let runnable = crate::json::from_value::<UiAction>(action.clone())
                .is_ok_and(|action| target_known(&action.target, vocabulary));
            let id = action.get("id").and_then(Value::as_str);
            if !runnable && let Some(id) = id {
                dropped.insert(id.to_owned());
            }
            runnable && id.is_none_or(|id| !dropped.contains(id))
        });
    }
    Some(())
}

/// Keep the blocks this client reads; `None` when a `required` one is not.
fn prepare_blocks(
    blocks: &mut Vec<Value>,
    vocabulary: &UiVocabulary,
    dropped: &mut BTreeSet<String>,
) -> Option<()> {
    let mut kept = Vec::with_capacity(blocks.len());
    for mut block in blocks.drain(..) {
        if prepare_block(&mut block, vocabulary, dropped)? {
            kept.push(block);
        }
    }
    *blocks = kept;
    Some(())
}

/// Whether `block` stays (`Some(true)`), goes (`Some(false)`, its owned
/// actions added to `dropped`), or makes the contribution unreadable.
fn prepare_block(
    block: &mut Value,
    vocabulary: &UiVocabulary,
    dropped: &mut BTreeSet<String>,
) -> Option<bool> {
    let required = block.get("required") == Some(&Value::Bool(true));
    let kind = block.get("kind").and_then(Value::as_str).map(str::to_owned);
    let mut known = kind
        .as_deref()
        .is_some_and(|kind| vocabulary.blocks.contains(kind));
    if known {
        if kind.as_deref() == Some("section")
            && let Some(Value::Array(children)) = block.get_mut("blocks")
        {
            prepare_blocks(children, vocabulary, dropped)?;
            known = !children.is_empty();
        }
        degrade_formats(block, vocabulary);
    }
    if known && crate::json::from_value::<UiBlock>(block.clone()).is_ok() {
        return Some(true);
    }
    if required {
        return None;
    }
    owned_actions(block, dropped);
    Some(false)
}

/// The actions a raw block owns: `action` (choices, protected media),
/// `actions` (rows, hero), `actionId` (banner), and those of the blocks a
/// section holds.
fn owned_actions(block: &Value, dropped: &mut BTreeSet<String>) {
    for key in ["action", "actionId"] {
        if let Some(id) = block.get(key).and_then(Value::as_str) {
            dropped.insert(id.to_owned());
        }
    }
    if let Some(Value::Array(ids)) = block.get("actions") {
        dropped.extend(ids.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    if let Some(Value::Array(children)) = block.get("blocks") {
        for child in children {
            owned_actions(child, dropped);
        }
    }
}

/// Degrade the formats of a known block: those of its fields, columns
/// (`valueFormat`), stat items and its own (`format`, chart and hero).
fn degrade_formats(block: &mut Value, vocabulary: &UiVocabulary) {
    degrade_format(block, "format", vocabulary);
    for (list, key) in [
        ("fields", "format"),
        ("items", "format"),
        ("columns", "valueFormat"),
    ] {
        if let Some(Value::Array(items)) = block.get_mut(list) {
            for item in items {
                degrade_format(item, key, vocabulary);
            }
        }
    }
}

/// Remove the format object at `key` of `object` when this client does not
/// know its kind or cannot read it. A string there (a column's `format`) is
/// not a field format and stays.
fn degrade_format(object: &mut Value, key: &str, vocabulary: &UiVocabulary) {
    let Some(object) = object.as_object_mut() else {
        return;
    };
    let unreadable = object.get(key).is_some_and(|format| {
        format.is_object()
            && (!format
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| vocabulary.formats.contains(kind))
                || crate::json::from_value::<UiFieldFormat>(format.clone()).is_err())
    });
    if unreadable {
        object.remove(key);
    }
}

/// Whether this client can run an action with `target`.
pub(crate) fn target_known(target: &UiActionTarget, vocabulary: &UiVocabulary) -> bool {
    match target {
        UiActionTarget::Operation { .. } => vocabulary.actions.contains("operation"),
        UiActionTarget::Page { .. } => vocabulary.actions.contains("page"),
        UiActionTarget::Native { capability, .. } => {
            vocabulary.actions.contains("native") && vocabulary.native.contains(capability.name())
        }
    }
}

/// The upgrade placeholder of a raw contribution, from its identity: id,
/// schema version, title and title translations (the id when the title does
/// not read), permissions, icon and order. `None` without an id or a schema
/// version.
fn placeholder(raw: &Map<String, Value>, kind: UiContributionKind) -> Option<UiContribution> {
    let id = raw
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 160)?
        .to_owned();
    let schema_version = u32::try_from(raw.get("schemaVersion")?.as_u64()?).ok()?;
    let text = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .filter(|text| valid_text(text))
            .map(str::to_owned)
    };
    let title = text(raw.get("schema").and_then(|s| s.get("title"))).unwrap_or_else(|| id.clone());
    let translations: BTreeMap<String, UiTranslation> = raw
        .get("schema")
        .and_then(|s| s.get("translations"))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(locale, _)| {
            !locale.is_empty()
                && locale.len() <= 32
                && locale
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        .take(32)
        .filter_map(|(locale, translation)| {
            Some((
                locale.clone(),
                UiTranslation {
                    title: text(translation.get("title"))?,
                    fields: Vec::new(),
                    actions: BTreeMap::new(),
                    values: BTreeMap::new(),
                    texts: BTreeMap::new(),
                },
            ))
        })
        .collect();
    let required_permissions = raw
        .get("requiredPermissions")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let upgrade = raw.get("upgrade");
    let unsupported =
        upgrade.and_then(|upgrade| upgrade.get("unsupported")) == Some(&Value::Bool(true));
    let min_build = upgrade
        .and_then(|upgrade| upgrade.get("minBuild"))
        .and_then(Value::as_u64)
        .filter(|_| !unsupported);
    Some(UiContribution {
        kind,
        id,
        schema_version,
        schema: UiDocument {
            title,
            presentation: None,
            fields: Vec::new(),
            actions: Vec::new(),
            blocks: Vec::new(),
            inputs: Vec::new(),
            translations,
        },
        required_permissions,
        icon: raw
            .get("icon")
            .and_then(Value::as_str)
            .filter(|icon| is_ui_icon_name(icon))
            .map(str::to_owned),
        order: raw
            .get("order")
            .and_then(Value::as_i64)
            .and_then(|order| i32::try_from(order).ok())
            .unwrap_or(DEFAULT_UI_ORDER),
        entity_path: None,
        forwardable: false,
        audience: None,
        detail: None,
        viewer_state: None,
        entity_lifecycle: None,
        summary: None,
        availability: None,
        source: None,
        preview: None,
        variants: Vec::new(),
        min_client: BTreeMap::new(),
        variant: None,
        upgrade: Some(UiUpgrade {
            min_build,
            unsupported,
        }),
        bundle: None,
    })
}
