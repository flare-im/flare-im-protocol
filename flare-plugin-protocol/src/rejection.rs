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

/// The explanation in `raw` per locale, or `None` unless `raw` is a JSON
/// object of `1..=4` locale tags (`^[a-z]{2,3}(-[A-Z]{2})?$`), each mapped to
/// `1..=300` characters of text without control characters.
pub fn parse_message(raw: &str) -> Option<BTreeMap<String, String>> {
    let message: BTreeMap<String, String> = serde_json::from_str(raw).ok()?;
    let valid = (1..=MAX_MESSAGE_LOCALES).contains(&message.len())
        && message.iter().all(|(locale, text)| {
            valid_locale(locale)
                && (1..=MAX_MESSAGE_CHARS).contains(&text.chars().count())
                && !text.chars().any(char::is_control)
        });
    valid.then_some(message)
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
    fn a_message_is_read_in_the_requested_language_or_chinese() {
        let raw = r#"{"zh-CN":"不能删除。","en-US":"Cannot delete."}"#;
        assert_eq!(message_for(raw, "en-US").as_deref(), Some("Cannot delete."));
        assert_eq!(message_for(raw, "fr-FR").as_deref(), Some("不能删除。"));
        assert_eq!(message_for(r#"{"en-US":"x"}"#, "ja-JP"), None);
        assert_eq!(message_for("plugin command rejected", "zh-CN"), None);
    }
}
