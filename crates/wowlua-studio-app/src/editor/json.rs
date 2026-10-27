//! JSON with schemas (S046): `wowlua.json` and world profiles
//! (`tests/profiles/*.json`) highlighted, their keys and values completed
//! and explained from the schemas wowlua's crates make. The text is read
//! by a scanner that bears unfinished JSON, as it is while you type.

use std::path::Path;

use serde_json::Value as Json;

use crate::system::{Highlight, Syntax};

/// Which schema a file has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Schema {
    Manifest,
    WorldProfile,
}

/// The schema of `path`: `wowlua.json`, or a JSON file in
/// `tests/profiles`.
pub fn schema_of(path: &Path) -> Option<Schema> {
    let name = path.file_name()?.to_str()?;
    if name == "wowlua.json" {
        return Some(Schema::Manifest);
    }
    let in_profiles = path
        .parent()
        .is_some_and(|d| d.ends_with("tests/profiles") || d.ends_with("tests\\profiles"));
    (name.ends_with(".json") && in_profiles).then_some(Schema::WorldProfile)
}

pub fn is_json(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "json")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    /// A string; `key` when a `:` follows it.
    Str {
        key: bool,
    },
    Num,
    Word,
    Punct(u8),
}

/// The tokens of a JSON text as byte ranges; unfinished ones end where the
/// text does.
fn tokens(text: &str) -> Vec<(usize, usize, Tok)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' && b[i] != b'\n' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i < b.len() && b[i] == b'"' {
                    i += 1;
                }
                let end = i.min(b.len());
                // A key: the next thing is `:`.
                let mut j = end;
                while j < b.len() && matches!(b[j], b' ' | b'\t' | b'\r' | b'\n') {
                    j += 1;
                }
                out.push((
                    start,
                    end,
                    Tok::Str {
                        key: j < b.len() && b[j] == b':',
                    },
                ));
            }
            b'{' | b'}' | b'[' | b']' | b':' | b',' => {
                out.push((i, i + 1, Tok::Punct(c)));
                i += 1;
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                while i < b.len() && matches!(b[i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    i += 1;
                }
                out.push((start, i, Tok::Num));
            }
            c if c.is_ascii_alphabetic() => {
                let start = i;
                while i < b.len() && b[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                out.push((start, i, Tok::Word));
            }
            _ => i += 1,
        }
    }
    out
}

/// The colours: keys as types, strings, numbers, `true`, `false` and
/// `null` as numbers (the literals), punctuation.
pub fn highlight(text: &str) -> Vec<Highlight> {
    tokens(text)
        .into_iter()
        .map(|(start, end, t)| {
            let syntax = match t {
                Tok::Str { key: true } => Syntax::Type,
                Tok::Str { key: false } => Syntax::String,
                Tok::Num | Tok::Word => Syntax::Number,
                Tok::Punct(_) => Syntax::Punct,
            };
            Highlight { start, end, syntax }
        })
        .collect()
}

/// Where the cursor is: the keys from the root down to the object or array
/// it is in, and whether it is on a key (or where one goes) or a value.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// Keys (`"x"`) and array items (`"[]"`), outermost first.
    pub path: Vec<String>,
    /// On a key, or where a key goes; else on a value of `key`.
    pub on_key: bool,
    /// The key the value belongs to.
    pub key: Option<String>,
    /// The keys the object has already.
    pub present: Vec<String>,
    /// The string or word typed so far at the cursor, and where it starts
    /// (bytes): what a suggestion replaces.
    pub typed: String,
    pub start: usize,
}

fn unquote(s: &str) -> String {
    s.trim_start_matches('"').trim_end_matches('"').to_owned()
}

/// Reads the text up to `offset` (bytes).
pub fn place(text: &str, offset: usize) -> Place {
    let toks = tokens(text);
    // Each open container: `{` with its keys and the key being written, or `[`.
    struct Frame {
        object: bool,
        key: Option<String>,
        keys: Vec<String>,
        after_colon: bool,
    }
    let mut stack: Vec<Frame> = Vec::new();
    let mut typed = String::new();
    let mut start = offset;
    let mut on_token = false;
    for &(s, e, t) in &toks {
        if s >= offset {
            break;
        }
        // The token the cursor is in or at the end of.
        if e >= offset && matches!(t, Tok::Str { .. } | Tok::Word | Tok::Num) {
            typed = text[s..offset].trim_start_matches('"').to_owned();
            start = s;
            on_token = true;
            if let (Tok::Str { key: true }, Some(f)) = (t, stack.last_mut()) {
                f.after_colon = false;
            }
            break;
        }
        match t {
            Tok::Punct(b'{') => stack.push(Frame {
                object: true,
                key: None,
                keys: Vec::new(),
                after_colon: false,
            }),
            Tok::Punct(b'[') => stack.push(Frame {
                object: false,
                key: None,
                keys: Vec::new(),
                after_colon: false,
            }),
            Tok::Punct(b'}') | Tok::Punct(b']') => {
                stack.pop();
            }
            Tok::Punct(b':') => {
                if let Some(f) = stack.last_mut() {
                    f.after_colon = true;
                }
            }
            Tok::Punct(b',') => {
                if let Some(f) = stack.last_mut() {
                    f.after_colon = false;
                }
            }
            Tok::Str { key: true } => {
                if let Some(f) = stack.last_mut() {
                    let k = unquote(&text[s..e]);
                    f.keys.push(k.clone());
                    f.key = Some(k);
                    f.after_colon = false;
                }
            }
            _ => {}
        }
    }
    // The keys the object has after the cursor too.
    let mut present: Vec<String> = stack.last().map(|f| f.keys.clone()).unwrap_or_default();
    let mut depth = 0i32;
    for &(s, e, t) in toks.iter().filter(|t| t.0 >= offset) {
        match t {
            Tok::Punct(b'{') | Tok::Punct(b'[') => depth += 1,
            Tok::Punct(b'}') | Tok::Punct(b']') => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            Tok::Str { key: true } if depth == 0 && s > offset => {
                present.push(unquote(&text[s..e]))
            }
            _ => {}
        }
    }
    let _ = on_token;
    let top = stack.last();
    let on_key = top.is_some_and(|f| f.object && !f.after_colon);
    let mut path = Vec::new();
    for (i, f) in stack.iter().enumerate() {
        let last = i + 1 == stack.len();
        if f.object {
            if !last && let Some(k) = &f.key {
                path.push(k.clone());
            }
        } else if !last {
            path.push("[]".to_owned());
        }
    }
    // The innermost array's items are below it too.
    if top.is_some_and(|f| !f.object) {
        path.push("[]".to_owned());
    }
    Place {
        path,
        on_key,
        key: top.and_then(|f| f.key.clone()).filter(|_| !on_key),
        present,
        typed,
        start,
    }
}

/// `node`, its `$ref` followed (`#/definitions/…`, `#/$defs/…`).
fn resolve<'a>(root: &'a Json, node: &'a Json) -> &'a Json {
    let mut node = node;
    for _ in 0..16 {
        let Some(r) = node.get("$ref").and_then(Json::as_str) else {
            break;
        };
        let Some(target) = r
            .strip_prefix("#/")
            .and_then(|p| root.pointer(&format!("/{p}")))
        else {
            break;
        };
        node = target;
    }
    node
}

/// The schemas a value may follow: `node` and those its `anyOf`, `oneOf`
/// and `allOf` name.
fn alternatives<'a>(root: &'a Json, node: &'a Json) -> Vec<&'a Json> {
    let node = resolve(root, node);
    let mut out = vec![node];
    for k in ["anyOf", "oneOf", "allOf"] {
        for n in node.get(k).and_then(Json::as_array).into_iter().flatten() {
            out.extend(alternatives(root, n));
        }
    }
    out
}

/// The schema of the value at `path`.
fn at_path<'a>(root: &'a Json, path: &[String]) -> Vec<&'a Json> {
    let mut nodes = alternatives(root, root);
    for seg in path {
        let mut next = Vec::new();
        for n in &nodes {
            if seg == "[]" {
                if let Some(items) = n.get("items") {
                    next.extend(alternatives(root, items));
                }
            } else if let Some(p) = n.get("properties").and_then(|p| p.get(seg)) {
                next.extend(alternatives(root, p));
            } else if let Some(a) = n.get("additionalProperties").filter(|a| a.is_object()) {
                next.extend(alternatives(root, a));
            }
        }
        nodes = next;
    }
    nodes
}

/// A suggestion from the schema: what it inserts, what it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub label: String,
    pub insert: String,
    pub detail: String,
}

fn description(node: &Json) -> String {
    node.get("description")
        .and_then(Json::as_str)
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .to_owned()
}

/// What can stand at the place: the keys the object may still have, or
/// the values the key's schema names (`enum`, `const`, booleans).
pub fn suggestions(schema: &Json, place: &Place) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    if place.on_key {
        for node in at_path(schema, &place.path) {
            for (name, prop) in node
                .get("properties")
                .and_then(Json::as_object)
                .into_iter()
                .flatten()
            {
                if place.present.contains(name) && *name != place.typed {
                    continue;
                }
                if out.iter().any(|s| s.label == *name) {
                    continue;
                }
                out.push(Suggestion {
                    label: name.clone(),
                    insert: format!("\"{name}\": "),
                    detail: description(resolve(schema, prop)),
                });
            }
        }
    } else if let Some(key) = &place.key {
        let mut path = place.path.clone();
        path.push(key.clone());
        for node in at_path(schema, &path) {
            let mut values: Vec<Json> = node
                .get("enum")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(c) = node.get("const") {
                values.push(c.clone());
            }
            if node.get("type") == Some(&Json::from("boolean")) {
                values.extend([Json::Bool(true), Json::Bool(false)]);
            }
            for v in values {
                let label = v.to_string();
                if !out.iter().any(|s| s.label == label) {
                    out.push(Suggestion {
                        insert: label.clone(),
                        label,
                        detail: description(node),
                    });
                }
            }
        }
    }
    let typed = place.typed.to_lowercase();
    out.retain(|s| s.label.trim_matches('"').to_lowercase().starts_with(&typed));
    out
}

/// The description of the key at `offset`, with its path.
pub fn explain(schema: &Json, text: &str, offset: usize) -> Option<String> {
    let toks = tokens(text);
    let &(s, e, _) = toks
        .iter()
        .find(|(s, e, t)| *s <= offset && offset <= *e && matches!(t, Tok::Str { key: true }))?;
    let key = unquote(&text[s..e]);
    let place = place(text, s + 1);
    let mut path = place.path.clone();
    path.push(key.clone());
    let node = at_path(schema, &path)
        .into_iter()
        .find(|n| n.get("description").is_some())?;
    let text = node.get("description")?.as_str()?;
    Some(format!("```text\n{}\n```\n\n{text}", path.join(".")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Json {
        json!({
            "type": "object",
            "properties": {
                "package": { "$ref": "#/definitions/Package", "description": "The package." },
                "check": { "type": "object", "properties": { "mode": { "enum": ["nocheck", "basic", "strict"], "description": "How strictly." } } },
                "flavors": { "type": "object", "additionalProperties": { "type": "object", "properties": { "min": { "type": "boolean" } } } },
            },
            "definitions": {
                "Package": { "type": "object", "properties": { "name": { "type": "string", "description": "Its name." }, "version": { "type": "string" } } },
            },
        })
    }

    #[test]
    fn keys_are_suggested_where_a_key_goes() {
        let text = "{ \"package\": { \"name\": \"x\", \"ve";
        let p = place(text, text.len());
        assert!(p.on_key);
        assert_eq!(p.path, ["package"]);
        assert_eq!(p.typed, "ve");
        let s = suggestions(&schema(), &p);
        assert_eq!(s.len(), 1, "{s:?}");
        assert_eq!(s[0].insert, "\"version\": ");
    }

    #[test]
    fn values_come_from_enums_and_booleans() {
        let text = "{ \"check\": { \"mode\": \"st";
        let p = place(text, text.len());
        assert!(!p.on_key);
        assert_eq!(p.key.as_deref(), Some("mode"));
        let s = suggestions(&schema(), &p);
        assert_eq!(
            s.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(),
            ["\"strict\""]
        );
        // A flavor's table: `additionalProperties`.
        let text = "{ \"flavors\": { \"camelot\": { \"min\": ";
        let p = place(text, text.len());
        assert_eq!(suggestions(&schema(), &p).len(), 2);
    }

    #[test]
    fn present_keys_are_left_out_and_keys_explained() {
        let text = "{ \"package\": {}, \"\n}";
        let p = place(text, text.len() - 2);
        let labels: Vec<String> = suggestions(&schema(), &p)
            .into_iter()
            .map(|s| s.label)
            .collect();
        assert_eq!(labels, ["check", "flavors"]);
        let text = "{ \"package\": { \"name\": \"x\" } }";
        let at = text.find("name").unwrap();
        assert!(explain(&schema(), text, at).unwrap().contains("Its name."));
    }

    #[test]
    fn the_schema_of_a_path() {
        assert_eq!(
            schema_of(Path::new("/a/wowlua.json")),
            Some(Schema::Manifest)
        );
        assert_eq!(
            schema_of(Path::new("/a/tests/profiles/raid.json")),
            Some(Schema::WorldProfile)
        );
        assert_eq!(schema_of(Path::new("/a/locales/enUS.json")), None);
    }
}
