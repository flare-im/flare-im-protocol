//! Form inputs of a UI document (v2). A composer-action or card action names
//! the inputs it collects in [`super::UiAction::form`]; the inputs of an admin
//! page (`admin.pages`) are the filters sent to its rows source. The host renders the form,
//! the core SDK validates it with [`validate_form_values`], and the plugin
//! re-validates the same rules on its side.
use super::{
    UiLocationPrecision, UiReference, UiTranslation, translate, valid_name, valid_path, valid_text,
};
use crate::{ProtocolError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::collections::BTreeSet;

/// `text`: default and largest `maxLength` (characters).
pub const TEXT_DEFAULT_MAX_LENGTH: u32 = 256;
pub const TEXT_MAX_LENGTH: u32 = 1024;
/// `textarea`: default and largest `maxLength` (characters).
pub const TEXTAREA_DEFAULT_MAX_LENGTH: u32 = 2000;
pub const TEXTAREA_MAX_LENGTH: u32 = 8000;
/// `options`: default and largest `maxLength` of one item (characters).
pub const OPTION_DEFAULT_MAX_LENGTH: u32 = 100;
pub const OPTION_MAX_LENGTH: u32 = 256;
/// `options`: most items (and the default `max`).
pub const OPTIONS_MAX_ITEMS: u32 = 50;
/// `members`: most user ids (and the default `max`).
pub const MEMBERS_MAX_ITEMS: u32 = 500;
/// `select`: most choices.
pub const SELECT_MAX_CHOICES: usize = 1024;
/// Plugin form files are bounded independently from action payloads.
pub const UI_FILE_MAX_BYTES: u64 = 5 * 1024 * 1024;
pub const UI_FILE_MAX_ACCEPT: usize = 8;
pub const UI_INPUT_CONDITION_MAX_VALUES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum UiInputKind {
    /// One line of text; value is a string.
    Text,
    /// Secret text: no default, data binding or durable page command.
    Password,
    /// Multi-line text; value is a string (newlines allowed).
    Textarea,
    /// Value is a JSON number within `min..=max`.
    Number,
    /// Value is a boolean; never `required`.
    Toggle,
    /// Value is the `value` of one of `choices`.
    Select,
    /// Value is an instant in epoch milliseconds (integer); the host picks it
    /// in the viewer's time zone.
    Datetime,
    /// An editable list of option texts; value is an array of strings, item
    /// count within `min..=max`, each item non-empty and unique.
    Options,
    /// Conversation members picked from the list the host provides; value is
    /// an array of user ids, count within `min..=max`. `max: 1` is a single pick.
    Members,
    /// One completed private plugin upload; value is exactly `{fileId}`.
    File,
    /// An amount of money; value is exactly `{amount, currency}`, `amount` a
    /// non-negative decimal string with at most the input's scale of
    /// fraction digits (see [`UiMoneyInput`]).
    Money,
    /// One picture taken or picked by the host (`selectImage`, `camera`) and
    /// uploaded privately, like a file; value is exactly `{fileId}`.
    Image,
    /// One side of an identity document captured by the host
    /// (`documentCapture`) and uploaded privately; value is exactly
    /// `{fileId}`. Its `side` says which.
    Document,
    /// One position fix taken by the host (`locationOnce`) at the input's
    /// `precision`; value is exactly [`UiLocationValue`].
    Location,
}
impl UiInputKind {
    /// Every kind, in declaration order: the vocabulary source of
    /// [`super::engine_vocabulary`].
    pub const ALL: &'static [Self] = &[
        Self::Text,
        Self::Password,
        Self::Textarea,
        Self::Number,
        Self::Toggle,
        Self::Select,
        Self::Datetime,
        Self::Options,
        Self::Members,
        Self::File,
        Self::Money,
        Self::Image,
        Self::Document,
        Self::Location,
    ];
    /// The wire name (`text`, `money`, ...).
    pub fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Password => "password",
            Self::Textarea => "textarea",
            Self::Number => "number",
            Self::Toggle => "toggle",
            Self::Select => "select",
            Self::Datetime => "datetime",
            Self::Options => "options",
            Self::Members => "members",
            Self::File => "file",
            Self::Money => "money",
            Self::Image => "image",
            Self::Document => "document",
            Self::Location => "location",
        }
    }
    /// Kinds whose value is a private upload (`{fileId}`) and that declare
    /// its upload in `file`: `file`, `image` and `document`.
    pub fn is_upload(self) -> bool {
        matches!(self, Self::File | Self::Image | Self::Document)
    }
}

/// Which side of an identity document a `document` input or a
/// `documentCapture` takes.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum UiDocumentSide {
    Front,
    Back,
    /// The holder with the document in hand.
    Holding,
}

/// The value of a `location` input: one fix the host truncated to the
/// input's precision ([`UiLocationValue::fits`]). The plugin server checks
/// it again; a client is never trusted with a position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiLocationValue {
    pub latitude: f64,
    pub longitude: f64,
    /// Metres, whole.
    pub accuracy_m: f64,
    pub precision: UiLocationPrecision,
}
impl UiLocationValue {
    /// Decimal places of the degrees, and the smallest accuracy in metres, a
    /// fix of `precision` keeps: two and 1 000 m (about a kilometre) for
    /// coarse, five and 1 m for fine.
    pub fn rules(precision: UiLocationPrecision) -> (i32, f64) {
        match precision {
            UiLocationPrecision::Coarse => (2, 1000.0),
            UiLocationPrecision::Fine => (5, 1.0),
        }
    }
    /// A fix truncated to `precision`: degrees rounded to its places,
    /// accuracy raised to its floor and rounded to whole metres. `None` for
    /// a fix outside the globe or with a broken accuracy.
    pub fn truncate(
        latitude: f64,
        longitude: f64,
        accuracy_m: Option<f64>,
        precision: UiLocationPrecision,
    ) -> Option<Self> {
        let valid = latitude.is_finite()
            && longitude.is_finite()
            && (-90.0..=90.0).contains(&latitude)
            && (-180.0..=180.0).contains(&longitude)
            && accuracy_m.is_none_or(|a| a.is_finite() && a >= 0.0);
        if !valid {
            return None;
        }
        let (places, floor) = Self::rules(precision);
        let scale = 10f64.powi(places);
        let round = |degrees: f64| (degrees * scale).round() / scale;
        Some(Self {
            latitude: round(latitude),
            longitude: round(longitude),
            accuracy_m: accuracy_m.map_or(floor, |a| a.max(floor)).round(),
            precision,
        })
    }
    /// The value is a fix of exactly `precision`: on the globe, degrees with
    /// no more places than it keeps, a whole accuracy not below its floor.
    pub fn fits(&self, precision: UiLocationPrecision) -> bool {
        let (places, floor) = Self::rules(precision);
        let scale = 10f64.powi(places);
        let truncated = |degrees: f64| ((degrees * scale).round() / scale - degrees).abs() < 1e-9;
        self.precision == precision
            && self.latitude.is_finite()
            && self.longitude.is_finite()
            && (-90.0..=90.0).contains(&self.latitude)
            && (-180.0..=180.0).contains(&self.longitude)
            && truncated(self.latitude)
            && truncated(self.longitude)
            && self.accuracy_m.is_finite()
            && self.accuracy_m >= floor
            && self.accuracy_m.fract() == 0.0
    }
}

/// Largest fraction digits of a money input, and digits of its whole part.
pub const MAX_UI_MONEY_INPUT_SCALE: u8 = 18;
pub const MAX_UI_MONEY_WHOLE_DIGITS: usize = 30;
/// Default fraction digits of a money input.
pub const DEFAULT_UI_MONEY_INPUT_SCALE: u8 = 2;

/// The amount rules of a `money` input. Exactly one of `currency` and
/// `currencyPath`; the fraction digits are `scale`, or read from the data
/// (`scalePath`, the exponent of the asset being paid, `0..=18`); the bounds
/// are decimal strings, declared (`min`, `max`) or read from the data the
/// document is resolved against (`minPath`, `maxPath`, such as a wallet
/// balance), the read one taking the place of the declared one when it
/// holds a valid amount. A resolved input carries the currency, scale and
/// bounds it read, and no paths; one whose `currencyPath` read nothing has
/// no currency and accepts no value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiMoneyInput {
    /// Currency or asset code, `[A-Za-z0-9_.-]{1,32}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency_path: Option<String>,
    /// Fraction digits the amount may have (`0..=18`, default 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<u8>,
    /// Where the data holds the fraction digits (a number or digit string,
    /// `0..=18`); it takes the place of `scale` when it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_path: Option<String>,
}

/// The value of a `money` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiMoneyValue {
    pub amount: String,
    pub currency: String,
}

fn valid_currency(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 32
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}

/// A non-negative decimal string without sign, exponent or leading zeros
/// (`0`, `12`, `12.5`, `0.05`), at most 30 whole digits and `scale`
/// fraction digits.
pub fn is_ui_decimal(amount: &str, scale: u8) -> bool {
    let (whole, fraction) = match amount.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (amount, None),
    };
    !whole.is_empty()
        && whole.len() <= MAX_UI_MONEY_WHOLE_DIGITS
        && whole.bytes().all(|b| b.is_ascii_digit())
        && (whole == "0" || !whole.starts_with('0'))
        && fraction.is_none_or(|f| {
            !f.is_empty() && f.len() <= usize::from(scale) && f.bytes().all(|b| b.is_ascii_digit())
        })
}

/// Order of two decimals accepted by [`is_ui_decimal`].
pub fn cmp_ui_decimal(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| -> (String, String) {
        let (whole, fraction) = s.split_once('.').unwrap_or((s, ""));
        (whole.to_owned(), fraction.trim_end_matches('0').to_owned())
    };
    let ((aw, af), (bw, bf)) = (split(a), split(b));
    aw.len()
        .cmp(&bw.len())
        .then_with(|| aw.cmp(&bw))
        .then_with(|| {
            let width = af.len().max(bf.len());
            format!("{af:0<width$}").cmp(&format!("{bf:0<width$}"))
        })
}

/// `amount` without trailing fraction zeros (`12.50` → `12.5`, `3.00` → `3`).
fn trim_ui_decimal(amount: &str) -> &str {
    if amount.contains('.') {
        amount.trim_end_matches('0').trim_end_matches('.')
    } else {
        amount
    }
}

/// `amount` cut to `scale` fraction digits (rounded down), when it is a
/// decimal with any number of fraction digits.
fn floor_ui_decimal(amount: &str, scale: u8) -> Option<String> {
    let trimmed = trim_ui_decimal(amount);
    if !is_ui_decimal(
        trimmed,
        MAX_UI_MONEY_INPUT_SCALE.max(u8::try_from(trimmed.len()).ok()?),
    ) {
        return None;
    }
    let cut = match trimmed.split_once('.') {
        Some((whole, fraction)) if fraction.len() > usize::from(scale) => {
            let kept = &fraction[..usize::from(scale)];
            if kept.is_empty() {
                whole.to_owned()
            } else {
                format!("{whole}.{kept}")
            }
        }
        _ => trimmed.to_owned(),
    };
    Some(trim_ui_decimal(&cut).to_owned())
}

impl UiMoneyInput {
    pub fn effective_scale(&self) -> u8 {
        self.scale.unwrap_or(DEFAULT_UI_MONEY_INPUT_SCALE)
    }
    /// The fraction digits a value is checked with: an input still reading
    /// its scale from data (not resolved) admits any up to the largest.
    fn check_scale(&self) -> u8 {
        if self.scale_path.is_some() {
            MAX_UI_MONEY_INPUT_SCALE
        } else {
            self.effective_scale()
        }
    }
    fn is_valid(&self) -> bool {
        let scale = self.check_scale();
        let bound = |b: &Option<String>| b.as_deref().is_none_or(|b| is_ui_decimal(b, scale));
        let path = |p: &Option<String>| p.as_deref().is_none_or(valid_path);
        (self.currency.is_some() != self.currency_path.is_some())
            && self.currency.as_deref().is_none_or(valid_currency)
            && scale <= MAX_UI_MONEY_INPUT_SCALE
            && bound(&self.min)
            && bound(&self.max)
            && path(&self.currency_path)
            && path(&self.scale_path)
            && path(&self.min_path)
            && path(&self.max_path)
            && match (&self.min, &self.max) {
                (Some(min), Some(max)) => cmp_ui_decimal(min, max).is_le(),
                _ => true,
            }
    }
    /// The rules as resolved over `data`: currency, scale and bounds read
    /// where the paths point, when they hold valid values; no paths left.
    /// Bounds hold the resolved scale: trailing zeros go, and a maximum
    /// with more fraction digits is rounded down to it (a minimum that does
    /// not fit is not taken). A `currencyPath` that reads no valid code
    /// leaves the input without a currency, which accepts no value.
    fn resolve(&self, data: &Value) -> Self {
        let read = |path: &Option<String>| {
            path.as_deref()
                .and_then(|p| data.pointer(p))
                .and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) if n.is_u64() => Some(n.to_string()),
                    _ => None,
                })
        };
        let mut resolved = self.clone();
        if self.currency_path.is_some() {
            resolved.currency = read(&self.currency_path).filter(|c| valid_currency(c));
        }
        if let Some(scale) = read(&self.scale_path)
            .and_then(|v| v.parse::<u8>().ok())
            .filter(|s| *s <= MAX_UI_MONEY_INPUT_SCALE)
        {
            resolved.scale = Some(scale);
        }
        resolved.scale_path = None;
        let scale = resolved.effective_scale();
        // Declared bounds hold the declared scale; one that does not fit the
        // resolved scale is handled like a read one.
        let fit_min =
            |v: &str| Some(trim_ui_decimal(v).to_owned()).filter(|v| is_ui_decimal(v, scale));
        resolved.min = read(&self.min_path)
            .and_then(|v| fit_min(&v))
            .or_else(|| self.min.as_deref().and_then(fit_min));
        resolved.max = read(&self.max_path)
            .and_then(|v| floor_ui_decimal(&v, scale))
            .or_else(|| self.max.as_deref().and_then(|v| floor_ui_decimal(v, scale)));
        resolved.currency_path = None;
        resolved.min_path = None;
        resolved.max_path = None;
        resolved
    }
    /// Check a submitted value.
    fn check(&self, value: &Value) -> std::result::Result<(), UiFormIssue> {
        let money: UiMoneyValue =
            crate::json::from_value(value.clone()).map_err(|_| UiFormIssue::InvalidType)?;
        if !is_ui_decimal(&money.amount, self.check_scale()) || !valid_currency(&money.currency) {
            return Err(UiFormIssue::InvalidType);
        }
        match (&self.currency, &self.currency_path) {
            (Some(currency), _) if *currency != money.currency => {
                return Err(UiFormIssue::NotAChoice);
            }
            // Resolved without a currency: its path read nothing.
            (None, None) => return Err(UiFormIssue::NotAChoice),
            _ => {}
        }
        if self
            .min
            .as_deref()
            .is_some_and(|min| cmp_ui_decimal(&money.amount, min).is_lt())
            || self
                .max
                .as_deref()
                .is_some_and(|max| cmp_ui_decimal(&money.amount, max).is_gt())
        {
            return Err(UiFormIssue::OutOfRange);
        }
        Ok(())
    }
}

/// A durable reference to a completed upload. URLs, grants and local paths are
/// transport state and never form values or journal payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiFileReference {
    #[schemars(length(min = 1, max = 128))]
    pub file_id: String,
}
impl UiFileReference {
    pub fn is_valid(&self) -> bool {
        crate::resource::valid_resource_id(&self.file_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiFileUpload {
    pub prepare_operation: String,
    pub complete_operation: String,
    /// Plugin-owned purpose, pinned by the declaration, never by the form.
    pub purpose: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiFileInput {
    /// Exact MIME types, without wildcards or parameters.
    #[schemars(length(min = 1, max = UI_FILE_MAX_ACCEPT))]
    pub accept: Vec<String>,
    #[schemars(range(min = 1, max = UI_FILE_MAX_BYTES))]
    pub max_bytes: u64,
    pub upload: UiFileUpload,
}
impl UiFileInput {
    pub fn is_valid(&self) -> bool {
        let operation = |s: &str| {
            !s.is_empty()
                && s.len() <= 128
                && s.split('.').all(|part| {
                    !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                })
        };
        let mut seen = BTreeSet::new();
        (1..=UI_FILE_MAX_BYTES).contains(&self.max_bytes)
            && !self.accept.is_empty()
            && self.accept.len() <= UI_FILE_MAX_ACCEPT
            && self
                .accept
                .iter()
                .all(|mime| valid_file_mime(mime) && seen.insert(mime))
            && operation(&self.upload.prepare_operation)
            && operation(&self.upload.complete_operation)
            && self.upload.prepare_operation != self.upload.complete_operation
            && valid_name(&self.upload.purpose)
    }
}

fn valid_file_mime(mime: &str) -> bool {
    let Some((kind, subtype)) = mime.split_once('/') else {
        return false;
    };
    mime.len() <= 128
        && !kind.is_empty()
        && !subtype.is_empty()
        && [kind, subtype].iter().all(|part| {
            part.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"!#$&^_.+-".contains(&b))
        })
}

/// Dynamic form visibility reads only an earlier unconditional Select of the
/// same collected form. Page data and arbitrary expressions are not involved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiInputShowWhen {
    pub field: String,
    #[schemars(length(min = 1, max = UI_INPUT_CONDITION_MAX_VALUES))]
    pub any_of: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiInputChoice {
    /// `1..=128` bytes, unique within the input.
    pub value: String,
    pub label: String,
}

/// A form input. Fields that do not apply to `kind` are rejected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiInput {
    /// The key of the value in the action input: `[A-Za-z0-9_]{1,64}`, unique
    /// in the document.
    pub id: String,
    pub kind: UiInputKind,
    pub label: String,
    /// A value is required: non-blank text, at least one item.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// Characters: text (`1..=1024`, default 256), textarea (`1..=8000`,
    /// default 2000), options per item (`1..=256`, default 100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    /// number: smallest value. options, members: fewest items (integer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Number>,
    /// number: largest value. options (`<= 50`), members (`<= 500`): most items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Number>,
    /// options and members only: the most items read from the data the
    /// document is resolved against instead (a composer form's
    /// `/config/<key>`). When it holds a positive integer the resolved input's
    /// `max` is that integer, capped at 50 or 500; it keeps the declared `max`
    /// when the value is absent, not such an integer, or would make the input
    /// invalid (below `min`, or below the item count of `default`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_path: Option<String>,
    /// Initial value of the input's type; options may hold empty placeholder
    /// items. Not allowed on members.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    /// Initial value read from the card data instead (for editing an existing
    /// entry); falls back to `default` when absent or of the wrong type. A row
    /// action's form reads it from the row (`ResolvedUiRowAction::values`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_path: Option<String>,
    /// select: `1..=1024` choices. options: `0..=1024`; when present the items
    /// are picked from them instead of typed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<UiInputChoice>,
    /// Read bounded select choices from the authenticated source projection.
    /// Removed before rendering; every host consumes the same resolved list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices_path: Option<String>,
    /// select and options only, instead of `choices`: the host reads the
    /// choices with the viewer's own permission. Admin pages only. The value
    /// is checked for shape here; the plugin decides whether it exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<UiReference>,
    /// file, image and document: the private upload. For image and
    /// document every `accept` type is an `image/` type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<UiFileInput>,
    /// money only, and required there: currency, scale and bounds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub money: Option<UiMoneyInput>,
    /// document only, and required there: which side it captures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<UiDocumentSide>,
    /// location only, and required there: the precision of the fix it
    /// takes. A `locationOnce` writing into it declares the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<UiLocationPrecision>,
    /// Hosts seed visible defaults once. Every controller value change clears
    /// all dependants, including uploads still visible under the new value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_when: Option<UiInputShowWhen>,
}

/// One problem with a submitted form value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum UiFormIssue {
    Required,
    /// Not an input of this form.
    Unknown,
    InvalidType,
    TooLong,
    OutOfRange,
    NotAChoice,
    TooFewItems,
    TooManyItems,
    /// An empty, too long or malformed item.
    InvalidItem,
    DuplicateItem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiFormError {
    pub input: String,
    pub issue: UiFormIssue,
}

fn count(n: &Number) -> Option<u32> {
    n.as_u64().and_then(|v| u32::try_from(v).ok())
}

fn single_line(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

fn multi_line(s: &str) -> bool {
    !s.chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}

impl UiInput {
    pub fn effective_max_length(&self) -> u32 {
        self.max_length.unwrap_or(match self.kind {
            UiInputKind::Textarea => TEXTAREA_DEFAULT_MAX_LENGTH,
            UiInputKind::Options => OPTION_DEFAULT_MAX_LENGTH,
            _ => TEXT_DEFAULT_MAX_LENGTH,
        })
    }

    /// `(fewest, most)` items of options and members.
    fn item_bounds(&self) -> (u32, u32) {
        let cap = if self.kind == UiInputKind::Members {
            MEMBERS_MAX_ITEMS
        } else {
            OPTIONS_MAX_ITEMS
        };
        (
            self.min.as_ref().and_then(count).unwrap_or(0),
            self.max.as_ref().and_then(count).unwrap_or(cap),
        )
    }

    pub(crate) fn validate(&self) -> Result<()> {
        use UiInputKind::*;
        let invalid = Err(ProtocolError::Denied("invalid UI input"));
        let text_like = matches!(self.kind, Text | Password | Textarea | Options);
        let ranged = matches!(self.kind, Number | Options | Members);
        if !valid_name(&self.id)
            || !valid_text(&self.label)
            || self.placeholder.as_deref().is_some_and(|p| !valid_text(p))
            || self.default_path.as_deref().is_some_and(|p| !valid_path(p))
            || self
                .max_path
                .as_deref()
                .is_some_and(|p| !valid_path(p) || !matches!(self.kind, Options | Members))
            || (self.kind == Password && (self.default.is_some() || self.default_path.is_some()))
            || (self.required && self.kind == Toggle)
            || (self.max_length.is_some() && !text_like)
            || ((self.min.is_some() || self.max.is_some()) && !ranged)
            || (self.default.is_some() && self.kind == Members)
            || self.kind.is_upload() != self.file.is_some()
            || self.file.as_ref().is_some_and(|file| {
                !file.is_valid()
                    || (matches!(self.kind, Image | Document)
                        && !file.accept.iter().all(|mime| mime.starts_with("image/")))
            })
            || (self.kind == Money) != self.money.is_some()
            || self.money.as_ref().is_some_and(|money| !money.is_valid())
            || (self.kind == Document) != self.side.is_some()
            || (self.kind == Location) != self.precision.is_some()
            || self.show_when.as_ref().is_some_and(|when| {
                !valid_name(&when.field)
                    || when.field == self.id
                    || when.any_of.is_empty()
                    || when.any_of.len() > UI_INPUT_CONDITION_MAX_VALUES
                    || when
                        .any_of
                        .iter()
                        .any(|v| v.is_empty() || v.len() > 128 || !single_line(v))
                    || when.any_of.iter().collect::<BTreeSet<_>>().len() != when.any_of.len()
            })
            || (self.kind == Select
                && self.reference.is_none()
                && self.choices_path.is_none()
                && self.choices.is_empty())
            || (!matches!(self.kind, Select | Options) && !self.choices.is_empty())
            || self.reference.as_ref().is_some_and(|reference| {
                !reference.is_valid()
                    || !matches!(self.kind, Select | Options)
                    || !self.choices.is_empty()
                    || self.choices_path.is_some()
            })
            || self
                .choices_path
                .as_deref()
                .is_some_and(|p| self.kind != Select || !valid_path(p))
        {
            return invalid;
        }
        if let Some(max_length) = self.max_length {
            let cap = match self.kind {
                Text | Password => TEXT_MAX_LENGTH,
                Textarea => TEXTAREA_MAX_LENGTH,
                _ => OPTION_MAX_LENGTH,
            };
            if !(1..=cap).contains(&max_length) {
                return invalid;
            }
        }
        match self.kind {
            Number => {
                let bound = |n: &Option<serde_json::Number>| {
                    n.as_ref().map(|n| n.as_f64().filter(|v| v.is_finite()))
                };
                let (min, max) = (bound(&self.min), bound(&self.max));
                if matches!(min, Some(None))
                    || matches!(max, Some(None))
                    || matches!((min.flatten(), max.flatten()), (Some(a), Some(b)) if a > b)
                {
                    return invalid;
                }
            }
            Options | Members => {
                let cap = if self.kind == Members {
                    MEMBERS_MAX_ITEMS
                } else {
                    OPTIONS_MAX_ITEMS
                };
                let (fewest, most) = self.item_bounds();
                if self.min.as_ref().is_some_and(|n| count(n).is_none())
                    || self.max.as_ref().is_some_and(|n| count(n).is_none())
                    || most == 0
                    || most > cap
                    || fewest > most
                    || !self.valid_choices()
                {
                    return invalid;
                }
            }
            Select if !self.valid_choices() => return invalid,
            _ => {}
        }
        if self
            .default
            .as_ref()
            .is_some_and(|default| !self.default_fits(default))
        {
            return invalid;
        }
        Ok(())
    }

    fn valid_choices(&self) -> bool {
        let mut values = BTreeSet::new();
        self.choices.len() <= SELECT_MAX_CHOICES
            && self.choices.iter().all(|c| {
                !c.value.is_empty()
                    && c.value.len() <= 128
                    && valid_text(&c.label)
                    && values.insert(c.value.as_str())
            })
    }

    /// Whether `value` is a usable initial value (looser than a submission:
    /// options may hold empty placeholder items, text may be blank).
    fn default_fits(&self, value: &Value) -> bool {
        use UiInputKind::*;
        let max = self.effective_max_length() as usize;
        match (self.kind, value) {
            (Text, Value::String(s)) => s.chars().count() <= max && single_line(s),
            (Textarea, Value::String(s)) => s.chars().count() <= max && multi_line(s),
            (Number, Value::Number(n)) => self.number_in_range(n),
            (Toggle, Value::Bool(_)) => true,
            (Select, Value::String(s)) => self.is_choice(s),
            (Datetime, Value::Number(n)) => n.as_i64().is_some_and(|v| v >= 0),
            (File | Image | Document, Value::Object(_)) => {
                crate::json::from_value::<UiFileReference>(value.clone())
                    .is_ok_and(|file| file.is_valid())
            }
            (Money, Value::Object(_)) => self
                .money
                .as_ref()
                .is_some_and(|money| money.check(value).is_ok()),
            (Location, Value::Object(_)) => self.location_fits(value),
            (Options, Value::Array(items)) => {
                items.len() <= self.item_bounds().1 as usize
                    && items.iter().all(|item| {
                        item.as_str()
                            .is_some_and(|s| s.chars().count() <= max && single_line(s))
                    })
            }
            _ => false,
        }
    }

    /// A `location` value of this input's precision.
    fn location_fits(&self, value: &Value) -> bool {
        self.precision.is_some_and(|precision| {
            crate::json::from_value::<UiLocationValue>(value.clone())
                .is_ok_and(|fix| fix.fits(precision))
        })
    }

    /// A referenced value cannot be listed here: only its shape is checked.
    fn is_choice(&self, value: &str) -> bool {
        if self.reference.is_some() {
            return !value.is_empty() && value.len() <= 128 && single_line(value);
        }
        self.choices.iter().any(|c| c.value == value)
    }

    fn number_in_range(&self, n: &Number) -> bool {
        let Some(value) = n.as_f64().filter(|v| v.is_finite()) else {
            return false;
        };
        self.min
            .as_ref()
            .and_then(Number::as_f64)
            .is_none_or(|min| value >= min)
            && self
                .max
                .as_ref()
                .and_then(Number::as_f64)
                .is_none_or(|max| value <= max)
    }

    /// The input as the host renders it: labels in the view language, `max`
    /// read from `data` when `maxPath` holds a usable one, then the initial
    /// value read from `data` when `defaultPath` holds one that fits.
    pub(crate) fn resolve(&self, data: &Value, translation: Option<&UiTranslation>) -> UiInput {
        let mut input = self.clone();
        if let Some(choices) = self
            .choices_path
            .as_deref()
            .and_then(|p| data.pointer(p))
            .and_then(|v| crate::json::from_value::<Vec<UiInputChoice>>(v.clone()).ok())
        {
            let mut candidate = input.clone();
            candidate.choices = choices;
            candidate.choices_path = None;
            if candidate.validate().is_ok() {
                input.choices = candidate.choices;
            }
        }
        input.choices_path = None;
        input.max_path = None;
        input.money = self.money.as_ref().map(|money| money.resolve(data));
        if let Some(most) = self
            .max_path
            .as_deref()
            .and_then(|path| data.pointer(path))
            .and_then(Value::as_u64)
        {
            let cap = if self.kind == UiInputKind::Members {
                MEMBERS_MAX_ITEMS
            } else {
                OPTIONS_MAX_ITEMS
            };
            let mut candidate = input.clone();
            candidate.max = Some(Number::from(most.min(u64::from(cap))));
            if candidate.validate().is_ok() {
                input.max = candidate.max;
            }
        }
        input.label = translate(translation, &self.label);
        input.placeholder = self
            .placeholder
            .as_deref()
            .map(|p| translate(translation, p));
        for choice in &mut input.choices {
            choice.label = translate(translation, &choice.label);
        }
        if let Some(value) = input.initial_at(data, self.default_path.as_deref()) {
            input.default = Some(value);
        }
        input.default_path = None;
        input
    }

    /// The value `data` holds at `path` when it is a usable initial value of
    /// this input as resolved.
    pub(crate) fn initial_at(&self, data: &Value, path: Option<&str>) -> Option<Value> {
        data.pointer(path?)
            .filter(|value| {
                !value.is_null()
                    && (self.default_fits(value)
                        || (self.kind == UiInputKind::Members && self.check(value).is_ok()))
            })
            .cloned()
    }

    /// Check one submitted value; returns the value to forward.
    fn check(&self, value: &Value) -> std::result::Result<Value, UiFormIssue> {
        use UiFormIssue::*;
        use UiInputKind::*;
        let max = self.effective_max_length() as usize;
        match (self.kind, value) {
            (Text | Password | Textarea, Value::String(s)) => {
                let shape_ok = if matches!(self.kind, Text | Password) {
                    single_line(s)
                } else {
                    multi_line(s)
                };
                if !shape_ok {
                    Err(InvalidType)
                } else if s.chars().count() > max {
                    Err(TooLong)
                } else if self.required && s.trim().is_empty() {
                    Err(Required)
                } else {
                    Ok(value.clone())
                }
            }
            (Number, Value::Number(n)) => {
                if self.number_in_range(n) {
                    Ok(value.clone())
                } else {
                    Err(OutOfRange)
                }
            }
            (Toggle, Value::Bool(_)) => Ok(value.clone()),
            (Select, Value::String(s)) => {
                if self.is_choice(s) {
                    Ok(value.clone())
                } else {
                    Err(NotAChoice)
                }
            }
            (Datetime, Value::Number(n)) => match n.as_i64() {
                Some(v) if v >= 0 => Ok(value.clone()),
                Some(_) => Err(OutOfRange),
                None => Err(InvalidType),
            },
            (File | Image | Document, Value::Object(_)) => {
                crate::json::from_value::<UiFileReference>(value.clone())
                    .ok()
                    .filter(UiFileReference::is_valid)
                    .map(|_| value.clone())
                    .ok_or(InvalidType)
            }
            (Money, Value::Object(_)) => match &self.money {
                Some(money) => money.check(value).map(|()| value.clone()),
                None => Err(InvalidType),
            },
            (Location, Value::Object(_)) => {
                if self.location_fits(value) {
                    Ok(value.clone())
                } else {
                    Err(InvalidType)
                }
            }
            (Options | Members, Value::Array(items)) => {
                let (fewest, most) = self.item_bounds();
                let mut seen = BTreeSet::new();
                for item in items {
                    let Some(text) = item.as_str() else {
                        return Err(InvalidItem);
                    };
                    let key = if self.kind == Options {
                        let trimmed = text.trim();
                        if trimmed.is_empty() || text.chars().count() > max || !single_line(text) {
                            return Err(InvalidItem);
                        }
                        if !self.choices.is_empty() && !self.is_choice(text) {
                            return Err(NotAChoice);
                        }
                        trimmed
                    } else {
                        if !crate::resource::valid_resource_id(text) {
                            return Err(InvalidItem);
                        }
                        text
                    };
                    if !seen.insert(key) {
                        return Err(DuplicateItem);
                    }
                }
                if self.required && items.is_empty() {
                    Err(Required)
                } else if items.len() < fewest as usize {
                    Err(TooFewItems)
                } else if items.len() > most as usize {
                    Err(TooManyItems)
                } else {
                    Ok(value.clone())
                }
            }
            _ => Err(InvalidType),
        }
    }
}

/// Validate controller references within a document, action form or filter
/// scope. Earlier-only, unconditional Select controllers exclude chains/cycles.
pub fn validate_input_conditions(inputs: &[&UiInput]) -> Result<()> {
    for (index, input) in inputs.iter().enumerate() {
        if let Some(when) = &input.show_when {
            let valid = inputs[..index].iter().any(|controller| {
                controller.id == when.field
                    && controller.kind == UiInputKind::Select
                    && controller.show_when.is_none()
            });
            if !valid {
                return Err(ProtocolError::Denied("invalid UI input condition"));
            }
        }
    }
    Ok(())
}

/// Determine visibility from the current controller value (or its ordinary
/// default). Call only on a validated collected form scope.
pub fn form_input_is_visible(input: &UiInput, inputs: &[&UiInput], values: &Value) -> bool {
    let Some(when) = &input.show_when else {
        return true;
    };
    let Some(controller) = inputs
        .iter()
        .find(|i| i.id == when.field && i.kind == UiInputKind::Select && i.show_when.is_none())
    else {
        return false;
    };
    values
        .get(&when.field)
        .filter(|v| !v.is_null())
        .or(controller.default.as_ref())
        .and_then(|value| controller.check(value).ok())
        .and_then(|value| value.as_str().map(str::to_owned))
        .is_some_and(|value| when.any_of.contains(&value))
}

/// Validate the submitted `values` (an object keyed by input id) of `inputs`.
/// An absent or null value takes the input's `default`; the result holds every
/// input that has a value, ready to merge into the action input. All problems
/// are reported, one per input.
pub fn validate_form_values(
    inputs: &[&UiInput],
    values: &Value,
) -> std::result::Result<Map<String, Value>, Vec<UiFormError>> {
    let empty = Map::new();
    let Some(values) = values.as_object().or(values.is_null().then_some(&empty)) else {
        return Err(vec![UiFormError {
            input: String::new(),
            issue: UiFormIssue::InvalidType,
        }]);
    };
    if validate_input_conditions(inputs).is_err() {
        return Err(vec![UiFormError {
            input: String::new(),
            issue: UiFormIssue::InvalidType,
        }]);
    }
    let mut errors: Vec<UiFormError> = values
        .keys()
        .filter(|key| !inputs.iter().any(|input| &input.id == *key))
        .map(|key| UiFormError {
            input: key.clone(),
            issue: UiFormIssue::Unknown,
        })
        .collect();
    let mut output = Map::new();
    let visibility_values = Value::Object(values.clone());
    for input in inputs {
        if !form_input_is_visible(input, inputs, &visibility_values) {
            continue;
        }
        let value = values
            .get(&input.id)
            .filter(|v| !v.is_null())
            // A conditional default is only an initial host seed. Falling
            // back here could resurrect an old QR/account after a type change.
            .or_else(|| {
                input
                    .show_when
                    .is_none()
                    .then_some(input.default.as_ref())
                    .flatten()
            });
        let Some(value) = value else {
            if input.required {
                errors.push(UiFormError {
                    input: input.id.clone(),
                    issue: UiFormIssue::Required,
                });
            }
            continue;
        };
        match input.check(value) {
            Ok(value) => {
                output.insert(input.id.clone(), value);
            }
            Err(issue) => errors.push(UiFormError {
                input: input.id.clone(),
                issue,
            }),
        }
    }
    if errors.is_empty() {
        Ok(output)
    } else {
        Err(errors)
    }
}
