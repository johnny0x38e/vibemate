//! Byte-preserving edits of one top-level `mcpServers.<name>` member in
//! Claude Code's `.claude.json`.
//!
//! Claude Code keeps account, onboarding, session and per-project state in the
//! same file, so re-serializing the whole document could reorder keys, change
//! number formatting or escape sequences in bytes vibemate does not own. After
//! serde_json validates the document, a small scanner locates the byte spans of
//! top-level members and of `mcpServers` members; edits splice only the owned
//! member (plus its separating comma/whitespace). Every other byte is copied.
//!
//! Insertions follow the surrounding indentation. Removing the last remaining
//! server leaves an empty `"mcpServers": {}` rather than deleting a top-level
//! key vibemate cannot prove it created.

use serde_json::Value;
use std::collections::BTreeSet;
use std::ops::Range;

/// The document is not a JSON object, `mcpServers` is not an object, or a key
/// relevant to the edit is duplicated (JSON.parse would silently keep the last
/// one, so the effective entry is ambiguous).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Invalid;

/// One `"key": value` member, as byte offsets into the source.
struct Member {
    key: String,
    /// Offset of the key's opening quote.
    key_start: usize,
    /// Byte range of the value.
    value: Range<usize>,
}

/// One object: its braces and members in source order.
struct Object {
    open: usize,
    close: usize,
    members: Vec<Member>,
}

struct Scanner<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Scanner<'_> {
    fn peek(&self) -> Result<u8, Invalid> {
        self.bytes.get(self.pos).copied().ok_or(Invalid)
    }
    fn whitespace(&mut self) {
        while matches!(self.bytes.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }
    fn expect(&mut self, byte: u8) -> Result<(), Invalid> {
        if self.peek()? != byte {
            return Err(Invalid);
        }
        self.pos += 1;
        Ok(())
    }
    fn string(&mut self) -> Result<Range<usize>, Invalid> {
        let start = self.pos;
        self.expect(b'"')?;
        loop {
            match self.peek()? {
                b'\\' => self.pos += 2,
                b'"' => {
                    self.pos += 1;
                    return Ok(start..self.pos);
                }
                _ => self.pos += 1,
            }
        }
    }
    fn value(&mut self) -> Result<Range<usize>, Invalid> {
        self.whitespace();
        let start = self.pos;
        match self.peek()? {
            b'{' => {
                self.object()?;
            }
            b'[' => self.array()?,
            b'"' => {
                self.string()?;
            }
            _ => {
                while let Some(byte) = self.bytes.get(self.pos) {
                    if matches!(byte, b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r') {
                        break;
                    }
                    self.pos += 1;
                }
                if self.pos == start {
                    return Err(Invalid);
                }
            }
        }
        Ok(start..self.pos)
    }
    fn array(&mut self) -> Result<(), Invalid> {
        self.expect(b'[')?;
        self.whitespace();
        if self.peek()? == b']' {
            self.pos += 1;
            return Ok(());
        }
        loop {
            self.value()?;
            self.whitespace();
            match self.peek()? {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return Err(Invalid),
            }
        }
    }
    fn object(&mut self) -> Result<Object, Invalid> {
        self.whitespace();
        let open = self.pos;
        self.expect(b'{')?;
        let mut members = Vec::new();
        self.whitespace();
        if self.peek()? == b'}' {
            self.pos += 1;
            return Ok(Object {
                open,
                close: self.pos - 1,
                members,
            });
        }
        loop {
            self.whitespace();
            let key_span = self.string()?;
            let key: String =
                serde_json::from_slice(&self.bytes[key_span.clone()]).map_err(|_| Invalid)?;
            self.whitespace();
            self.expect(b':')?;
            let value = self.value()?;
            members.push(Member {
                key,
                key_start: key_span.start,
                value,
            });
            self.whitespace();
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    return Ok(Object {
                        open,
                        close: self.pos - 1,
                        members,
                    });
                }
                _ => return Err(Invalid),
            }
        }
    }
}

/// A validated `.claude.json` with located top-level and `mcpServers` spans.
pub(super) struct Document<'a> {
    bytes: &'a [u8],
    root: Object,
    /// Index of the `mcpServers` member in `root` and its scanned object.
    servers: Option<(usize, Object)>,
}

/// Validate and scan. An absent file is represented by the caller as `{}`.
pub(super) fn parse(bytes: &[u8]) -> Result<Document<'_>, Invalid> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Invalid)?;
    if !value.is_object() {
        return Err(Invalid);
    }
    let root = Scanner { bytes, pos: 0 }.object()?;
    let mut found = None;
    for (index, member) in root.members.iter().enumerate() {
        if member.key == "mcpServers" {
            if found.is_some() {
                return Err(Invalid);
            }
            found = Some(index);
        }
    }
    let servers = match found {
        None => None,
        Some(index) => {
            let mut scanner = Scanner {
                bytes,
                pos: root.members[index].value.start,
            };
            if scanner.peek()? != b'{' {
                return Err(Invalid);
            }
            let object = scanner.object()?;
            let mut names = BTreeSet::new();
            if object.members.iter().any(|m| !names.insert(m.key.as_str())) {
                return Err(Invalid);
            }
            Some((index, object))
        }
    };
    Ok(Document {
        bytes,
        root,
        servers,
    })
}

/// Whitespace before `pos` on its line, or None when other content precedes it
/// (a single-line/compact layout).
fn line_indent(bytes: &[u8], pos: usize) -> Option<&str> {
    let start = bytes[..pos]
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |i| i + 1);
    let prefix = &bytes[start..pos];
    if prefix.iter().all(|b| *b == b' ' || *b == b'\t') {
        std::str::from_utf8(prefix).ok()
    } else {
        None
    }
}

fn render(value: &Value, indent: Option<&str>, unit: &str) -> Result<String, Invalid> {
    let Some(indent) = indent else {
        return serde_json::to_string(value).map_err(|_| Invalid);
    };
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(unit.as_bytes());
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    serde::Serialize::serialize(value, &mut serializer).map_err(|_| Invalid)?;
    let text = String::from_utf8(out).map_err(|_| Invalid)?;
    // JSON strings escape newlines, so every raw newline is layout.
    Ok(text.replace('\n', &format!("\n{indent}")))
}

impl Document<'_> {
    fn member(&self, name: &str) -> Option<(usize, &Member)> {
        let (_, servers) = self.servers.as_ref()?;
        servers
            .members
            .iter()
            .enumerate()
            .find(|(_, m)| m.key == name)
    }

    /// Whether the exact top-level server name exists (contents not inspected).
    pub(super) fn contains(&self, name: &str) -> bool {
        self.member(name).is_some()
    }

    /// Parsed value of `mcpServers.<name>`, if present.
    pub(super) fn entry(&self, name: &str) -> Result<Option<Value>, Invalid> {
        self.member(name)
            .map(|(_, m)| serde_json::from_slice(&self.bytes[m.value.clone()]).map_err(|_| Invalid))
            .transpose()
    }

    /// Indentation unit used by top-level members, defaulting to two spaces.
    fn unit(&self) -> String {
        self.root
            .members
            .first()
            .and_then(|m| line_indent(self.bytes, m.key_start))
            .filter(|i| !i.is_empty())
            .unwrap_or("  ")
            .to_owned()
    }

    fn splice(&self, range: Range<usize>, text: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes.len() + text.len());
        out.extend_from_slice(&self.bytes[..range.start]);
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(&self.bytes[range.end..]);
        out
    }

    /// Return new bytes with `mcpServers.<name>` set to `entry` (or removed when
    /// None). Bytes outside the spliced member are identical to the input.
    pub(super) fn with_entry(&self, name: &str, entry: Option<&Value>) -> Result<Vec<u8>, Invalid> {
        let out = self.splice_entry(name, entry)?;
        // Defensive: the output must still be a valid document with exactly
        // the requested entry.
        let check = parse(&out)?;
        if check.entry(name)?.as_ref() != entry {
            return Err(Invalid);
        }
        Ok(out)
    }

    fn splice_entry(&self, name: &str, entry: Option<&Value>) -> Result<Vec<u8>, Invalid> {
        let bytes = self.bytes;
        let unit = self.unit();
        let key = serde_json::to_string(name).map_err(|_| Invalid)?;
        match (&self.servers, entry) {
            (None, None) => Ok(bytes.to_vec()),
            (Some((index, servers)), Some(value)) => {
                if let Some((_, member)) = self.member(name) {
                    let indent = line_indent(bytes, member.key_start);
                    return Ok(self.splice(member.value.clone(), &render(value, indent, &unit)?));
                }
                if let Some(last) = servers.members.last() {
                    let text = match line_indent(bytes, last.key_start) {
                        Some(indent) => {
                            format!(",\n{indent}{key}: {}", render(value, Some(indent), &unit)?)
                        }
                        None => format!(",{key}:{}", render(value, None, &unit)?),
                    };
                    let at = last.value.end;
                    return Ok(self.splice(at..at, &text));
                }
                let text = match line_indent(bytes, self.root.members[*index].key_start) {
                    Some(parent) => {
                        let inner = format!("{parent}{unit}");
                        format!(
                            "\n{inner}{key}: {}\n{parent}",
                            render(value, Some(&inner), &unit)?
                        )
                    }
                    None => format!("{key}:{}", render(value, None, &unit)?),
                };
                Ok(self.splice(servers.open + 1..servers.close, &text))
            }
            (Some((_, servers)), None) => {
                let Some((i, _)) = self.member(name) else {
                    return Ok(bytes.to_vec());
                };
                let members = &servers.members;
                let range = if members.len() == 1 {
                    servers.open + 1..servers.close
                } else if i + 1 < members.len() {
                    members[i].key_start..members[i + 1].key_start
                } else {
                    members[i - 1].value.end..members[i].value.end
                };
                Ok(self.splice(range, ""))
            }
            (None, Some(value)) => {
                let mut map = serde_json::Map::new();
                map.insert(name.to_owned(), value.clone());
                let object = Value::Object(map);
                if let Some(last) = self.root.members.last() {
                    let text = match line_indent(bytes, last.key_start) {
                        Some(indent) => format!(
                            ",\n{indent}\"mcpServers\": {}",
                            render(&object, Some(indent), &unit)?
                        ),
                        None => format!(",\"mcpServers\":{}", render(&object, None, &unit)?),
                    };
                    let at = last.value.end;
                    return Ok(self.splice(at..at, &text));
                }
                let parent = line_indent(bytes, self.root.open).unwrap_or("");
                let inner = format!("{parent}{unit}");
                let text = format!(
                    "\n{inner}\"mcpServers\": {}\n{parent}",
                    render(&object, Some(&inner), &unit)?
                );
                Ok(self.splice(self.root.open + 1..self.root.close, &text))
            }
        }
    }
}

/// Key-sorted compact JSON, independent of serde_json's map ordering feature,
/// so ownership digests survive Claude Code re-serializing the file.
pub(super) fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut members: Vec<_> = map.iter().collect();
            members.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (index, (key, item)) in members.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write_canonical(item, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Assert `after` equals `before` with exactly one contiguous insertion.
    fn single_insertion(before: &[u8], after: &[u8]) -> String {
        assert!(after.len() > before.len());
        let start = before
            .iter()
            .zip(after)
            .position(|(a, b)| a != b)
            .unwrap_or(before.len());
        let added = after.len() - before.len();
        assert_eq!(&after[..start], &before[..start]);
        assert_eq!(&after[start + added..], &before[start..]);
        String::from_utf8(after[start..start + added].to_vec()).unwrap()
    }

    const PRETTY: &str = "{\n  \"numStartups\": 12,\n  \"weird\" :  { \"a\\\"}{\" : [1, 2.50, \"}\"] },\n  \"projects\": {\n    \"/tmp/p\": {\n      \"mcpServers\": {\"local\": {\"command\": \"x\"}}\n    }\n  },\n  \"mcpServers\": {\n    \"other\": {\n      \"type\": \"stdio\",\n      \"command\": \"o\",\n      \"args\": []\n    }\n  },\n  \"emoji\": \"\\u00e9\u{1f600}\"\n}\n";

    #[test]
    fn insert_replace_remove_touch_only_the_member() {
        let entry = json!({"type": "http", "url": "https://e.test/mcp"});
        let doc = parse(PRETTY.as_bytes()).unwrap();
        assert!(doc.contains("other") && !doc.contains("local"));
        let added = doc.with_entry("new", Some(&entry)).unwrap();
        let inserted = single_insertion(PRETTY.as_bytes(), &added);
        assert_eq!(
            inserted,
            ",\n    \"new\": {\n      \"type\": \"http\",\n      \"url\": \"https://e.test/mcp\"\n    }"
        );
        let replaced = parse(&added)
            .unwrap()
            .with_entry(
                "new",
                Some(&json!({"type": "stdio", "command": "c", "args": []})),
            )
            .unwrap();
        let old = parse(&added).unwrap();
        let (_, member) = old.member("new").unwrap();
        assert_eq!(
            &replaced[..member.value.start],
            &added[..member.value.start]
        );
        assert!(replaced.ends_with(&added[member.value.end..]));
        let removed = parse(&replaced).unwrap().with_entry("new", None).unwrap();
        assert_eq!(removed, PRETTY.as_bytes());
    }

    #[test]
    fn first_middle_and_last_member_removal_is_exact() {
        let source = "{\"mcpServers\": {\n    \"a\": 1,\n    \"b\": {\"x\": \"]\"},\n    \"c\": [3]\n  }, \"z\": null}";
        let doc = parse(source.as_bytes()).unwrap();
        let a = String::from_utf8(doc.with_entry("a", None).unwrap()).unwrap();
        assert_eq!(
            a,
            "{\"mcpServers\": {\n    \"b\": {\"x\": \"]\"},\n    \"c\": [3]\n  }, \"z\": null}"
        );
        let b = String::from_utf8(doc.with_entry("b", None).unwrap()).unwrap();
        assert_eq!(
            b,
            "{\"mcpServers\": {\n    \"a\": 1,\n    \"c\": [3]\n  }, \"z\": null}"
        );
        let c = String::from_utf8(doc.with_entry("c", None).unwrap()).unwrap();
        assert_eq!(
            c,
            "{\"mcpServers\": {\n    \"a\": 1,\n    \"b\": {\"x\": \"]\"}\n  }, \"z\": null}"
        );
        let only = parse(b"{\"mcpServers\":{\"a\":1},\"k\":2}").unwrap();
        assert_eq!(
            only.with_entry("a", None).unwrap(),
            b"{\"mcpServers\":{},\"k\":2}"
        );
        assert_eq!(
            only.with_entry("missing", None).unwrap(),
            b"{\"mcpServers\":{\"a\":1},\"k\":2}"
        );
    }

    #[test]
    fn container_creation_follows_layout() {
        let entry = json!({"type": "stdio", "command": "c", "args": ["a"]});
        let compact = parse(b"{\"userID\":\"u\",\"n\":1}").unwrap();
        let out = compact.with_entry("s", Some(&entry)).unwrap();
        assert_eq!(
            single_insertion(b"{\"userID\":\"u\",\"n\":1}", &out),
            ",\"mcpServers\":{\"s\":{\"args\":[\"a\"],\"command\":\"c\",\"type\":\"stdio\"}}"
        );
        let empty = parse(b"{}").unwrap().with_entry("s", Some(&entry)).unwrap();
        assert_eq!(
            String::from_utf8(empty).unwrap(),
            "{\n  \"mcpServers\": {\n    \"s\": {\n      \"args\": [\n        \"a\"\n      ],\n      \"command\": \"c\",\n      \"type\": \"stdio\"\n    }\n  }\n}"
        );
        let tabs = "{\n\t\"mcpServers\": {}\n}";
        let out = parse(tabs.as_bytes())
            .unwrap()
            .with_entry("s", Some(&json!({"type": "http", "url": "u"})))
            .unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "{\n\t\"mcpServers\": {\n\t\t\"s\": {\n\t\t\t\"type\": \"http\",\n\t\t\t\"url\": \"u\"\n\t\t}\n\t}\n}"
        );
    }

    #[test]
    fn ambiguous_or_invalid_documents_are_refused() {
        for source in [
            "",
            "[]",
            "null",
            "{\"mcpServers\": []}",
            "{\"mcpServers\": null}",
            "{\"mcpServers\": {}, \"mcpServers\": {}}",
            "{\"mcpServers\": {\"a\": 1, \"a\": 2}}",
            "{\"a\": 1,}",
            "\u{feff}{}",
        ] {
            assert!(parse(source.as_bytes()).is_err(), "{source:?}");
        }
        // Escaped keys decode to the same name as Claude Code's JSON.parse.
        let doc = parse(b"{\"mcp\\u0053ervers\": {\"\\u0061\": {\"url\": \"u\"}}}").unwrap();
        assert!(doc.contains("a"));
    }

    #[test]
    fn canonical_form_ignores_key_order() {
        assert_eq!(
            canonical(&json!({"b": [1, {"d": 1, "c": "\u{e9}"}], "a": null})),
            "{\"a\":null,\"b\":[1,{\"c\":\"\u{e9}\",\"d\":1}]}"
        );
    }
}
