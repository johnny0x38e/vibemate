//! The `unmanagedKeys` list a state read returns for a hand-edited entry.
//!
//! The list is a display hint: the full key set already went into the entry
//! digest, so the row is `modified` and every write is refused whatever this
//! list holds. The interface schema accepts at most `MAX_KEYS` names of
//! 1 to `MAX_KEY_UNITS` UTF-16 code units each, so one odd TOML key (a quoted
//! empty key, a very long one, or very many keys) must not make the whole
//! state response fail to parse. Empty names are dropped, long names are cut
//! and end in `…`, duplicates after cutting are merged, and the list is capped.

use serde_json::Value;

/// Most names returned for one entry (interface limit).
pub(super) const MAX_KEYS: usize = 64;
/// Longest returned name in UTF-16 code units (interface limit).
pub(super) const MAX_KEY_UNITS: usize = 128;
/// Marks a cut name; one UTF-16 code unit.
const CUT_MARK: char = '…';

/// Display names from an entry's `unmanagedKeys`, within interface limits.
pub(super) fn display_names(entry: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let names = entry
        .get("unmanagedKeys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);
    for name in names {
        if out.len() == MAX_KEYS {
            break;
        }
        if name.is_empty() {
            continue;
        }
        let shown = fit(name);
        if !out.contains(&shown) {
            out.push(shown);
        }
    }
    out
}

/// Cut a name to `MAX_KEY_UNITS` UTF-16 units on a character boundary.
fn fit(name: &str) -> String {
    if name.encode_utf16().count() <= MAX_KEY_UNITS {
        return name.to_owned();
    }
    let budget = MAX_KEY_UNITS - CUT_MARK.len_utf16();
    let mut used = 0;
    let mut out = String::new();
    for character in name.chars() {
        used += character.len_utf16();
        if used > budget {
            break;
        }
        out.push(character);
    }
    out.push(CUT_MARK);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn units(text: &str) -> usize {
        text.encode_utf16().count()
    }

    #[test]
    fn empty_names_are_dropped_and_short_ones_kept_in_order() {
        let entry = json!({ "unmanagedKeys": ["", "alpha", "", "beta"] });
        assert_eq!(display_names(&entry), ["alpha", "beta"]);
        assert!(display_names(&json!({})).is_empty());
        assert!(display_names(&json!({ "unmanagedKeys": [""] })).is_empty());
    }

    #[test]
    fn long_names_are_cut_on_a_character_boundary_and_merged() {
        let long = "k".repeat(200);
        // Astral characters take two UTF-16 units each.
        let astral = "😀".repeat(100);
        let near_twin = format!("{}{}", "k".repeat(200), "tail");
        let entry = json!({ "unmanagedKeys": [long, astral, near_twin, "x".repeat(128)] });
        let names = display_names(&entry);
        assert_eq!(names.len(), 3, "two cut names collapse into one");
        for name in &names {
            assert!((1..=MAX_KEY_UNITS).contains(&units(name)), "{name}");
        }
        assert!(names[0].starts_with("kkk") && names[0].ends_with(CUT_MARK));
        assert_eq!(units(&names[0]), MAX_KEY_UNITS);
        assert!(names[1].ends_with(CUT_MARK));
        assert!(
            names[1]
                .trim_end_matches(CUT_MARK)
                .chars()
                .all(|c| c == '😀')
        );
        assert_eq!(
            names[2],
            "x".repeat(128),
            "exactly at the limit stays whole"
        );
    }

    #[test]
    fn the_list_is_capped() {
        let keys: Vec<String> = (0..70).map(|index| format!("key{index:02}")).collect();
        let names = display_names(&json!({ "unmanagedKeys": keys }));
        assert_eq!(names.len(), MAX_KEYS);
        assert_eq!(names[0], "key00");
        assert_eq!(names[63], "key63");
    }
}
