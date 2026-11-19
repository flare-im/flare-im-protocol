//! The explanation a plugin may give with a definite rejection.
//!
//! Besides its `error_code`, a rejected call's `error_message` may be a JSON
//! object mapping locale tags to plain text, such as
//! `{"en-US":"Cannot delete.","zh-CN":"不能删除。"}`. Anything else in that
//! field means the rejection carries no explanation. Readers check the shape
//! here; writers need not depend on this crate.
use std::collections::BTreeMap;

/// Locales one explanation may carry.
pub const MAX_MESSAGE_LOCALES: usize = 4;
/// Characters of one locale's text.
pub const MAX_MESSAGE_CHARS: usize = 300;
/// Bytes of the whole `error_message`, as Core forwards it: two locales of
/// 300 Chinese characters are already more.
pub const MAX_MESSAGE_BYTES: usize = 1024;

/// The explanation in `raw` per locale, or `None` unless `raw` is at most
/// 1024 bytes of a JSON object of `1..=4` locale tags
/// (`^[a-z]{2,3}(-[A-Z]{2})?$`), each mapped to `1..=300` characters of text
/// without control characters, format characters (bidirectional embeddings,
/// overrides and isolates, zero-width characters, the byte order mark, tag
/// characters) or line and paragraph separators.
pub fn parse_message(raw: &str) -> Option<BTreeMap<String, String>> {
    if raw.len() > MAX_MESSAGE_BYTES {
        return None;
    }
    let message: BTreeMap<String, String> = serde_json::from_str(raw).ok()?;
    let valid = (1..=MAX_MESSAGE_LOCALES).contains(&message.len())
        && message.iter().all(|(locale, text)| {
            valid_locale(locale)
                && (1..=MAX_MESSAGE_CHARS).contains(&text.chars().count())
                && !text.chars().any(hidden_character)
        });
    valid.then_some(message)
}

/// Unicode format characters (general category `Cf`), the set
/// `flare_server_core::auth::stated_reason` refuses in an audit reason; this
/// crate does not depend on it.
const FORMAT: &[(char, char)] = &[
    ('\u{00AD}', '\u{00AD}'),
    ('\u{0600}', '\u{0605}'),
    ('\u{061C}', '\u{061C}'),
    ('\u{06DD}', '\u{06DD}'),
    ('\u{070F}', '\u{070F}'),
    ('\u{0890}', '\u{0891}'),
    ('\u{08E2}', '\u{08E2}'),
    ('\u{180E}', '\u{180E}'),
    ('\u{200B}', '\u{200F}'),
    ('\u{202A}', '\u{202E}'),
    ('\u{2060}', '\u{2064}'),
    ('\u{2066}', '\u{206F}'),
    ('\u{FEFF}', '\u{FEFF}'),
    ('\u{FFF9}', '\u{FFFB}'),
    ('\u{110BD}', '\u{110BD}'),
    ('\u{110CD}', '\u{110CD}'),
    ('\u{13430}', '\u{1343F}'),
    ('\u{1BCA0}', '\u{1BCA3}'),
    ('\u{1D173}', '\u{1D17A}'),
    ('\u{E0001}', '\u{E0001}'),
    ('\u{E0020}', '\u{E007F}'),
];

/// A control (`Cc`) or format (`Cf`) character, or the line or paragraph
/// separator (`Zl`, `Zp`): each changes how the text around it renders, the
/// code shown after it too.
fn hidden_character(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{2028}' | '\u{2029}')
        || FORMAT.iter().any(|(low, high)| (*low..=*high).contains(&c))
}

/// The explanation in `locale`, else in `zh-CN`, else `None`.
pub fn message_for(raw: &str, locale: &str) -> Option<String> {
    let mut message = parse_message(raw)?;
    message.remove(locale).or_else(|| message.remove("zh-CN"))
}

fn valid_locale(tag: &str) -> bool {
    let (language, region) = tag.split_once('-').unwrap_or((tag, "AA"));
    (2..=3).contains(&language.len())
        && language.bytes().all(|b| b.is_ascii_lowercase())
        && region.len() == 2
        && region.bytes().all(|b| b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_messages_parse_only_the_documented_shape() {
        let message = parse_message(r#"{"zh-CN":"不能删除。","en-US":"Cannot delete."}"#).unwrap();
        assert_eq!(message.len(), 2);
        assert_eq!(message["zh-CN"], "不能删除。");
        assert_eq!(message["en-US"], "Cannot delete.");
        assert!(parse_message(r#"{"zh":"不能删除。"}"#).is_some());
        let five = r#"{"zh-CN":"a","en-US":"a","ja-JP":"a","fr-FR":"a","de-DE":"a"}"#;
        let long = serde_json::json!({ "zh-CN": "字".repeat(301) }).to_string();
        let at_limit = serde_json::json!({ "zh-CN": "字".repeat(300) }).to_string();
        assert!(parse_message(&at_limit).is_some());
        for raw in [
            "plugin command rejected",
            "{}",
            five,
            long.as_str(),
            r#"{"zh-CN":"第一行\n第二行"}"#,
            r#"{"zh_cn":"不能删除。"}"#,
            r#"{"zh-CN":1}"#,
            r#"{"zh-CN":""}"#,
            r#"{"zh-":"a"}"#,
            r#"{"zh-cn":"a"}"#,
            r#"["zh-CN","a"]"#,
        ] {
            assert_eq!(parse_message(raw), None, "{raw}");
        }
    }

    #[test]
    fn a_message_fits_in_1024_bytes() {
        // Two locales within their characters: 101 + 300 × 3 bytes of text
        // and 23 of JSON.
        let message = |english: usize| {
            format!(
                r#"{{"en-US":"{}","zh-CN":"{}"}}"#,
                "a".repeat(english),
                "字".repeat(300)
            )
        };
        assert_eq!(message(101).len(), MAX_MESSAGE_BYTES);
        assert!(parse_message(&message(101)).is_some());
        assert_eq!(message(102).len(), MAX_MESSAGE_BYTES + 1);
        assert_eq!(parse_message(&message(102)), None);
    }

    #[test]
    fn a_message_holds_no_character_that_reorders_the_text() {
        for hidden in [
            '\u{202E}',
            '\u{2066}',
            '\u{200B}',
            '\u{200D}',
            '\u{FEFF}',
            '\u{2028}',
            '\u{E0041}',
        ] {
            let raw = serde_json::json!({ "zh-CN": format!("不能{hidden}删除。") }).to_string();
            assert_eq!(parse_message(&raw), None, "{:04X}", u32::from(hidden));
        }
        // Visible characters around them still read.
        assert!(parse_message(r#"{"zh-CN":"不能删除：（1）"}"#).is_some());
    }

    #[test]
    fn a_message_is_read_in_the_requested_language_or_chinese() {
        let raw = r#"{"zh-CN":"不能删除。","en-US":"Cannot delete."}"#;
        assert_eq!(message_for(raw, "en-US").as_deref(), Some("Cannot delete."));
        assert_eq!(message_for(raw, "fr-FR").as_deref(), Some("不能删除。"));
        assert_eq!(message_for(r#"{"en-US":"x"}"#, "ja-JP"), None);
        assert_eq!(message_for("plugin command rejected", "zh-CN"), None);
    }
}
