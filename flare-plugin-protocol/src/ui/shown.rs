//! A view a host passes back to read its next page into.
//!
//! A host returns the view it shows (`previous`) with a next-page or refresh
//! call, and the client appends the page to it. Paging changes only the
//! view's `nextCursor` and its entries and rows blocks, so the shown view is
//! kept as the JSON the host sent: only what paging reads is checked, and
//! everything else goes back as it came. A client then never reads a
//! resolved view back into its types, which would compile a reader for every
//! resolved block into a client that only ever writes them.
use super::{ResolvedPluginView, ResolvedUiBlock, ResolvedUiEntries, ResolvedUiEntry};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// The fields of [`ResolvedPluginView`] in its JSON form.
const VIEW_FIELDS: &[&str] = &[
    "available",
    "presentation",
    "unavailableReason",
    "title",
    "fields",
    "actions",
    "menuActions",
    "blocks",
    "inputs",
    "detail",
    "nextCursor",
    "refreshMs",
    "minBuild",
    "tabs",
];

/// A view as a host passed it back ([`ResolvedPluginView`] in its JSON form).
#[derive(Debug, Clone, PartialEq)]
pub struct ShownPluginView(Map<String, Value>);

impl ShownPluginView {
    /// The shown view, when it reads as one: an object with a boolean
    /// `available`, whose `blocks` (if any) are objects naming their `kind`,
    /// entries blocks with an `items` array and rows blocks with a `rows`
    /// array.
    pub fn from_value(value: Value) -> Option<Self> {
        let Value::Object(view) = value else {
            return None;
        };
        // A field no view has is not one Core returned.
        if !view.keys().all(|key| VIEW_FIELDS.contains(&key.as_str())) {
            return None;
        }
        view.get("available")?.as_bool()?;
        if !view.get("nextCursor").is_none_or(Value::is_string) {
            return None;
        }
        match view.get("blocks") {
            None => {}
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    let list = match block.get("kind")?.as_str()? {
                        "entries" => "items",
                        "rows" => "rows",
                        _ => continue,
                    };
                    block.get(list)?.as_array()?;
                }
            }
            Some(_) => return None,
        }
        Some(Self(view))
    }

    /// The view as JSON, as a host shows it.
    pub fn into_value(self) -> Value {
        Value::Object(self.0)
    }

    pub fn available(&self) -> bool {
        self.0.get("available").and_then(Value::as_bool) == Some(true)
    }

    /// The cursor of the page after the shown one.
    pub fn next_cursor(&self) -> Option<&str> {
        self.0.get("nextCursor").and_then(Value::as_str)
    }

    /// Entries the view shows, over all of its entries blocks.
    pub fn entries(&self) -> usize {
        self.count("entries", "items")
    }

    /// Rows the view shows, over all of its rows blocks.
    pub fn rows(&self) -> usize {
        self.count("rows", "rows")
    }

    fn count(&self, kind: &str, list: &str) -> usize {
        self.blocks()
            .filter(|block| block.get("kind").and_then(Value::as_str) == Some(kind))
            .filter_map(|block| block.get(list).and_then(Value::as_array))
            .map(Vec::len)
            .sum()
    }

    fn blocks(&self) -> impl Iterator<Item = &Map<String, Value>> {
        self.0
            .get("blocks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
    }

    /// Append the next page, exactly as [`ResolvedPluginView::append_page`]
    /// appends it to the typed view.
    pub fn append_page(&mut self, page: ResolvedPluginView) {
        if !self.available() || !page.available {
            self.0.remove("nextCursor");
            return;
        }
        set_optional(&mut self.0, "nextCursor", page.next_cursor);
        let (mut entry_pages, mut row_pages) = (Vec::new(), Vec::new());
        for block in page.blocks {
            match block {
                ResolvedUiBlock::Entries(entries) => entry_pages.push(entries),
                ResolvedUiBlock::Rows(rows) => row_pages.push(rows),
                _ => {}
            }
        }
        let Some(Value::Array(blocks)) = self.0.get_mut("blocks") else {
            return;
        };
        let mut row_pages = row_pages.into_iter();
        for block in blocks.iter_mut().filter_map(Value::as_object_mut) {
            if block.get("kind").and_then(Value::as_str) != Some("rows") {
                continue;
            }
            let Some(next) = row_pages.next() else {
                break;
            };
            if let Some(Value::Array(rows)) = block.get_mut("rows") {
                let shown: BTreeSet<String> = rows
                    .iter()
                    .filter_map(|row| row.get("key").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect();
                rows.extend(
                    next.rows
                        .into_iter()
                        .filter(|row| !shown.contains(&row.key))
                        .map(to_value),
                );
            }
            set_optional(block, "nextCursor", next.next_cursor);
        }
        let mut pages = entry_pages.into_iter();
        for block in blocks.iter_mut().filter_map(Value::as_object_mut) {
            if block.get("kind").and_then(Value::as_str) != Some("entries") {
                continue;
            }
            let Some(next) = pages.next() else {
                break;
            };
            append_entries(block, next);
        }
    }
}

fn append_entries(block: &mut Map<String, Value>, next: ResolvedUiEntries) {
    let grouped = block.get("grouped").and_then(Value::as_bool) == Some(true);
    let total = block.get("total").and_then(Value::as_u64).unwrap_or(0);
    let Some(Value::Array(items)) = block.get_mut("items") else {
        return;
    };
    if grouped {
        append_grouped(items, next.items);
    } else {
        let offset = items.iter().filter_map(index).max().unwrap_or(0);
        items.extend(next.items.into_iter().map(|mut item| {
            item.index = item.index.saturating_add(offset);
            to_value(item)
        }));
    }
    let total = total.max(next.total).max(items.len() as u64);
    block.insert("total".into(), total.into());
}

/// [`ResolvedUiEntries::append_grouped`] on the shown items.
fn append_grouped(items: &mut Vec<Value>, page: Vec<ResolvedUiEntry>) {
    let header = |item: &Value| item.get("group").is_some_and(|group| !group.is_null());
    let last_header = items.iter().rposition(header);
    let last_key = last_header.and_then(|i| {
        items[i]["group"]
            .get("key")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let offset = items.last().and_then(index).unwrap_or(0);
    let mut continuing = true;
    for mut item in page {
        if let Some(group) = item.group.take() {
            if continuing && last_key.as_deref() == Some(group.key.as_str()) {
                if let Some(Value::Object(shown)) =
                    last_header.and_then(|i| items[i].get_mut("group"))
                {
                    for (key, value) in [
                        ("count", group.count),
                        ("total", group.total),
                        ("percent", group.percent.map(u64::from)),
                    ] {
                        if let Some(value) = value {
                            shown.insert(key.into(), value.into());
                        }
                    }
                }
            } else {
                continuing = false;
                item.group = Some(group);
            }
        }
        if continuing {
            item.index = item.index.saturating_add(offset);
        }
        items.push(to_value(item));
    }
}

fn index(item: &Value) -> Option<u32> {
    item.get("index")
        .and_then(Value::as_u64)
        .map(|index| u32::try_from(index).unwrap_or(u32::MAX))
}

fn set_optional(object: &mut Map<String, Value>, key: &str, value: Option<String>) {
    match value {
        Some(value) => {
            object.insert(key.into(), value.into());
        }
        None => {
            object.remove(key);
        }
    }
}

fn to_value(value: impl serde::Serialize) -> Value {
    // The resolved types serialize to JSON objects without fail.
    serde_json::to_value(value).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_view_fields_are_those_of_the_resolved_view() {
        let schema = serde_json::to_value(schemars::schema_for!(ResolvedPluginView)).unwrap();
        let mut fields: Vec<&str> = schema["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut known = VIEW_FIELDS.to_vec();
        fields.sort_unstable();
        known.sort_unstable();
        assert_eq!(fields, known);
    }

    #[test]
    fn only_a_view_reads_as_a_shown_view() {
        let view = json!({"available": true, "title": "t", "fields": [], "actions": [],
            "blocks": [{"kind": "entries", "items": []}, {"kind": "note", "text": "n"}]});
        let shown = ShownPluginView::from_value(view.clone()).unwrap();
        assert_eq!(
            (shown.entries(), shown.rows(), shown.available()),
            (0, 0, true)
        );
        assert_eq!(shown.into_value(), view);
        for broken in [
            json!([]),
            json!({"title": "t"}),
            json!({"available": "yes"}),
            json!({"available": true, "injected": 1}),
            json!({"available": true, "nextCursor": 1}),
            json!({"available": true, "blocks": {}}),
            json!({"available": true, "blocks": [1]}),
            json!({"available": true, "blocks": [{"text": "no kind"}]}),
            json!({"available": true, "blocks": [{"kind": "entries"}]}),
            json!({"available": true, "blocks": [{"kind": "rows", "rows": {}}]}),
        ] {
            assert!(
                ShownPluginView::from_value(broken.clone()).is_none(),
                "{broken}"
            );
        }
    }
}
