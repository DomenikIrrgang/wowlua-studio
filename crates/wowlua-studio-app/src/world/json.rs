//! A world profile's JSON changed in place (S060): a value at a path
//! replaced, added or removed as an edit of the file's text, so its
//! formatting, its keys' order and the rest of it stay as they are.

use serde_json::Value as Json;
use std::ops::Range;

/// A step of a path: an object's key or an array's index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Name(String),
    Index(usize),
}

impl From<&str> for Key {
    fn from(s: &str) -> Key {
        Key::Name(s.to_string())
    }
}

impl From<usize> for Key {
    fn from(i: usize) -> Key {
        Key::Index(i)
    }
}

/// A value's bytes, and its members' (an object's with their keys').
#[derive(Debug)]
struct Node {
    span: Range<usize>,
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    Object(Vec<Member>),
    Array(Vec<Node>),
    Scalar,
}

#[derive(Debug)]
struct Member {
    key: String,
    key_start: usize,
    value: Node,
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|b| b.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn string(&mut self) -> Option<String> {
        let start = self.i;
        if self.s.get(self.i) != Some(&b'"') {
            return None;
        }
        self.i += 1;
        while let Some(&b) = self.s.get(self.i) {
            self.i += 1;
            match b {
                b'\\' => self.i += 1,
                b'"' => {
                    let raw = std::str::from_utf8(&self.s[start..self.i]).ok()?;
                    return serde_json::from_str(raw).ok();
                }
                _ => {}
            }
        }
        None
    }

    fn value(&mut self) -> Option<Node> {
        self.ws();
        let start = self.i;
        match *self.s.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut members = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        let key_start = self.i;
                        let key = self.string()?;
                        self.ws();
                        if self.s.get(self.i) != Some(&b':') {
                            return None;
                        }
                        self.i += 1;
                        let value = self.value()?;
                        members.push(Member {
                            key,
                            key_start,
                            value,
                        });
                        self.ws();
                        match self.s.get(self.i)? {
                            b',' => self.i += 1,
                            b'}' => {
                                self.i += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                Some(Node {
                    span: start..self.i,
                    kind: Kind::Object(members),
                })
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                } else {
                    loop {
                        items.push(self.value()?);
                        self.ws();
                        match self.s.get(self.i)? {
                            b',' => self.i += 1,
                            b']' => {
                                self.i += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                Some(Node {
                    span: start..self.i,
                    kind: Kind::Array(items),
                })
            }
            b'"' => {
                self.string()?;
                Some(Node {
                    span: start..self.i,
                    kind: Kind::Scalar,
                })
            }
            _ => {
                while self
                    .s
                    .get(self.i)
                    .is_some_and(|b| !b.is_ascii_whitespace() && !b",]}".contains(b))
                {
                    self.i += 1;
                }
                let raw = std::str::from_utf8(&self.s[start..self.i]).ok()?;
                serde_json::from_str::<Json>(raw).ok()?;
                Some(Node {
                    span: start..self.i,
                    kind: Kind::Scalar,
                })
            }
        }
    }
}

fn parse(text: &str) -> Option<Node> {
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
    };
    let root = p.value()?;
    p.ws();
    (p.i == text.len()).then_some(root)
}

fn child<'a>(node: &'a Node, key: &Key) -> Option<&'a Node> {
    match (&node.kind, key) {
        (Kind::Object(members), Key::Name(k)) => {
            members.iter().rev().find(|m| &m.key == k).map(|m| &m.value)
        }
        (Kind::Array(items), Key::Index(i)) => items.get(*i),
        _ => None,
    }
}

/// The bytes of the value at `path`.
pub fn span(text: &str, path: &[Key]) -> Option<Range<usize>> {
    let mut node = &parse(text)?;
    for k in path {
        node = child(node, k)?;
    }
    Some(node.span.clone())
}

/// The value at `path`.
pub fn get(text: &str, path: &[Key]) -> Option<Json> {
    serde_json::from_str(&text[span(text, path)?]).ok()
}

/// The indentation of the line `at` is on.
fn indent_at(text: &str, at: usize) -> String {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    text[line_start..]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

/// `value` as text at an indentation: scalars and empty containers on one
/// line, others pretty with two spaces more per level.
fn render(value: &Json, indent: &str) -> String {
    let pretty = serde_json::to_string_pretty(value).unwrap_or_default();
    pretty.replace('\n', &format!("\n{indent}"))
}

/// `value` under the missing `rest` of a path (objects for names, an
/// array for index 0).
fn nest(rest: &[Key], value: Json) -> Json {
    rest.iter().rev().fold(value, |inner, k| match k {
        Key::Name(n) => {
            let mut map = serde_json::Map::new();
            map.insert(n.clone(), inner);
            Json::Object(map)
        }
        Key::Index(_) => Json::Array(vec![inner]),
    })
}

/// The edit that sets the value at `path` to `value` (none: removes it):
/// the bytes to replace and their replacement. None when the text is no
/// JSON, or the path goes through something that is no container.
pub fn set(text: &str, path: &[Key], value: Option<&Json>) -> Option<(Range<usize>, String)> {
    let root = parse(text)?;
    let mut node = &root;
    let mut parents: Vec<&Node> = Vec::new();
    for (depth, k) in path.iter().enumerate() {
        match child(node, k) {
            Some(next) => {
                parents.push(node);
                node = next;
            }
            None => {
                // Missing from here: added to `node`.
                let value = value?;
                return add(text, node, &path[depth..], value.clone());
            }
        }
    }
    let Some(value) = value else {
        let parent = parents.last()?;
        return remove(parent, path.last()?);
    };
    let indent = indent_at(text, node.span.start);
    Some((node.span.clone(), render(value, &indent)))
}

/// Adds `rest` (the first step a member of `node`) with `value`.
fn add(text: &str, node: &Node, rest: &[Key], value: Json) -> Option<(Range<usize>, String)> {
    let (first, deeper) = rest.split_first()?;
    let inner = nest(deeper, value);
    let base = indent_at(text, node.span.start);
    match (&node.kind, first) {
        (Kind::Object(members), Key::Name(k)) => {
            let key = serde_json::to_string(k).ok()?;
            match members.last() {
                Some(last) => {
                    let indent = indent_at(text, last.key_start);
                    let one_line = !text[node.span.clone()].contains('\n');
                    let sep = if one_line {
                        " ".to_string()
                    } else {
                        format!("\n{indent}")
                    };
                    let at = last.value.span.end;
                    Some((at..at, format!(",{sep}{key}: {}", render(&inner, &indent))))
                }
                None => {
                    let indent = format!("{base}  ");
                    Some((
                        node.span.clone(),
                        format!("{{\n{indent}{key}: {}\n{base}}}", render(&inner, &indent)),
                    ))
                }
            }
        }
        (Kind::Array(items), Key::Index(i)) if *i == items.len() => match items.last() {
            Some(last) => {
                let one_line = !text[node.span.clone()].contains('\n');
                let indent = indent_at(text, last.span.start);
                let sep = if one_line {
                    " ".to_string()
                } else {
                    format!("\n{indent}")
                };
                let at = last.span.end;
                Some((at..at, format!(",{sep}{}", render(&inner, &indent))))
            }
            None => Some((node.span.clone(), format!("[{}]", render(&inner, &base)))),
        },
        _ => None,
    }
}

/// Removes the member `key` of `parent`, with the comma that joins it.
fn remove(parent: &Node, key: &Key) -> Option<(Range<usize>, String)> {
    let spans: Vec<Range<usize>> = match &parent.kind {
        Kind::Object(members) => members
            .iter()
            .map(|m| m.key_start..m.value.span.end)
            .collect(),
        Kind::Array(items) => items.iter().map(|n| n.span.clone()).collect(),
        Kind::Scalar => return None,
    };
    let at = match (&parent.kind, key) {
        (Kind::Object(members), Key::Name(k)) => members.iter().rposition(|m| &m.key == k)?,
        (Kind::Array(_), Key::Index(i)) => *i,
        _ => return None,
    };
    let this = spans.get(at)?.clone();
    let range = if spans.len() == 1 {
        // The only one: the container left empty.
        let open = parent.span.start + 1;
        let close = parent.span.end - 1;
        open..close
    } else if let Some(next) = spans.get(at + 1) {
        this.start..next.start
    } else {
        spans[at - 1].end..this.end
    };
    Some((range, String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn apply(text: &str, path: &[Key], value: Option<Json>) -> String {
        let (range, with) = set(text, path, value.as_ref()).expect("an edit");
        let mut out = text.to_string();
        out.replace_range(range, &with);
        out
    }

    const PROFILE: &str = r#"{
  "character": { "name": "Player", "level": 60 },
  "units": [
    { "name": "Ogre", "level": 62 },
    { "name": "Rogue" }
  ]
}"#;

    #[test]
    fn a_value_changes_and_nothing_else() {
        let out = apply(
            PROFILE,
            &["character".into(), "level".into()],
            Some(json!(61)),
        );
        assert_eq!(out, PROFILE.replace("\"level\": 60", "\"level\": 61"));
        let out = apply(
            PROFILE,
            &["units".into(), 1.into(), "name".into()],
            Some(json!("Rogue \"Shadow\"")),
        );
        assert!(out.contains(r#"{ "name": "Rogue \"Shadow\"" }"#), "{out}");
    }

    #[test]
    fn a_missing_key_is_added_after_the_last_member() {
        // On one line, on one line.
        let out = apply(
            PROFILE,
            &["character".into(), "class".into()],
            Some(json!("MAGE")),
        );
        assert!(
            out.contains(r#"{ "name": "Player", "level": 60, "class": "MAGE" }"#),
            "{out}"
        );
        // A part the profile lacks: at the top, on its own line.
        let out = apply(PROFILE, &["money".into()], Some(json!(500)));
        assert!(out.ends_with("  ],\n  \"money\": 500\n}"), "{out}");
        // Through missing objects.
        let out = apply(
            PROFILE,
            &["position".into(), "zone".into()],
            Some(json!("Elwynn Forest")),
        );
        let v: Json = serde_json::from_str(&out).unwrap();
        assert_eq!(v["position"]["zone"], "Elwynn Forest");
        // An item at the end of an array.
        let out = apply(
            PROFILE,
            &["units".into(), 2.into()],
            Some(json!({ "name": "Wolf" })),
        );
        let v: Json = serde_json::from_str(&out).unwrap();
        assert_eq!(v["units"][2]["name"], "Wolf");
        // Into an empty object.
        let out = apply("{}", &["money".into()], Some(json!(5)));
        assert_eq!(out, "{\n  \"money\": 5\n}");
    }

    #[test]
    fn a_member_is_removed_with_its_comma() {
        let out = apply(PROFILE, &["character".into(), "level".into()], None);
        assert!(
            out.contains(r#""character": { "name": "Player" }"#),
            "{out}"
        );
        let out = apply(PROFILE, &["character".into(), "name".into()], None);
        assert!(out.contains(r#""character": { "level": 60 }"#), "{out}");
        let out = apply(PROFILE, &["units".into(), 0.into()], None);
        let v: Json = serde_json::from_str(&out).unwrap();
        assert_eq!(v["units"], json!([{ "name": "Rogue" }]));
        let out = apply(r#"{ "a": { "b": 1 } }"#, &["a".into(), "b".into()], None);
        assert_eq!(out, r#"{ "a": {} }"#);
    }

    #[test]
    fn no_edit_where_the_text_is_no_json() {
        assert!(set("{ \"a\": ", &["a".into()], Some(&json!(1))).is_none());
        assert!(
            set(
                PROFILE,
                &["character".into(), "name".into(), "x".into()],
                Some(&json!(1))
            )
            .is_none()
        );
        assert_eq!(
            get(PROFILE, &["units".into(), 0.into(), "level".into()]),
            Some(json!(62))
        );
    }
}
