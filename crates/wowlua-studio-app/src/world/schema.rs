//! The form of a part without its own design, from the profile's schema
//! (S060, wowlua's `world-profile-0.1.json`): typed fields for an object,
//! key and value rows for a map, entries by key for a list of objects, a
//! JSON box for a free value.

use crate::editor::json::{alternatives, resolve};
use serde_json::Value as Json;

/// How a value is edited.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Text,
    Number,
    Bool,
    /// A string, number or boolean, typed as written.
    Scalar,
    /// Numbers, written `1, 2, 3`.
    Numbers,
    /// Any JSON.
    Json,
}

impl Kind {
    /// The type as the form names it.
    pub fn word(&self) -> &'static str {
        match self {
            Kind::Text => "string",
            Kind::Number => "number",
            Kind::Bool => "boolean",
            Kind::Scalar => "string, number or boolean",
            Kind::Numbers => "list of numbers",
            Kind::Json => "any JSON",
        }
    }
}

/// A field of an object.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub kind: Kind,
    pub help: String,
}

/// A part's form.
#[derive(Clone, Debug, PartialEq)]
pub enum Form {
    /// An object's fields; `other`: it keeps fields the schema doesn't name.
    Fields { fields: Vec<Field>, other: bool },
    /// The part is one value (`money`, `client`).
    Value(Kind),
    /// Keys and values (`cvars`, `bindings`, `stats`).
    Map(Kind),
    /// Entries by key (an ID) or place, each an object's fields (`items`).
    Entries { fields: Vec<Field>, other: bool },
    /// Any JSON.
    Json,
}

/// The schemas a value may follow, without `null`.
fn options<'a>(root: &'a Json, node: &'a Json) -> Vec<&'a Json> {
    alternatives(root, node)
        .into_iter()
        .filter(|n| {
            !(n.get("type") == Some(&Json::from("null"))
                || n.get("anyOf").is_some()
                || n.get("oneOf").is_some())
        })
        .collect()
}

/// The types a schema allows, without `null`.
fn types(node: &Json) -> Vec<&str> {
    match node.get("type") {
        Some(Json::String(t)) => vec![t.as_str()],
        Some(Json::Array(list)) => list
            .iter()
            .filter_map(Json::as_str)
            .filter(|t| *t != "null")
            .collect(),
        _ => Vec::new(),
    }
}

fn help(node: &Json) -> String {
    node.get("description")
        .and_then(Json::as_str)
        .unwrap_or("")
        .split("\n\n")
        .next()
        .unwrap_or("")
        .replace('\n', " ")
}

/// How a value following `node` is edited.
pub fn kind(root: &Json, node: &Json) -> Kind {
    let opts = options(root, node);
    let all: Vec<&str> = opts.iter().flat_map(|n| types(n)).collect();
    let has = |t: &str| all.contains(&t);
    if opts.len() > 1 && has("string") && has("number") && has("boolean") {
        return Kind::Scalar;
    }
    if all.len() == 1 {
        match all[0] {
            "string" => return Kind::Text,
            "number" | "integer" => return Kind::Number,
            "boolean" => return Kind::Bool,
            _ => {}
        }
    }
    // A list of numbers (`appearances.collected`).
    if opts.iter().any(|n| {
        types(n) == ["array"]
            && n.get("items")
                .is_some_and(|i| kind(root, i) == Kind::Number)
    }) {
        return Kind::Numbers;
    }
    Kind::Json
}

/// An object schema's fields and whether it keeps others.
fn fields(root: &Json, object: &Json) -> (Vec<Field>, bool) {
    let object = resolve(root, object);
    let list = object
        .get("properties")
        .and_then(Json::as_object)
        .map(|props| {
            props
                .iter()
                .map(|(name, node)| Field {
                    name: name.clone(),
                    kind: kind(root, node),
                    help: help(resolve(root, node)),
                })
                .collect()
        })
        .unwrap_or_default();
    let other = object.get("additionalProperties") != Some(&Json::Bool(false));
    (list, other)
}

/// The form of the part `name`.
pub fn form(schema: &Json, name: &str) -> Form {
    let Some(node) = schema.get("properties").and_then(|p| p.get(name)) else {
        return Form::Json;
    };
    let opts = options(schema, node);
    // A list of objects: an array of T or an object of T.
    let array_of = opts
        .iter()
        .find(|n| types(n) == ["array"])
        .and_then(|n| n.get("items"));
    let object_of = opts
        .iter()
        .find(|n| types(n) == ["object"] && n.get("properties").is_none())
        .and_then(|n| n.get("additionalProperties"))
        .filter(|a| a.is_object());
    if let Some(item) = array_of.or(object_of) {
        let item_opts = options(schema, item);
        if let Some(object) = item_opts.iter().find(|n| n.get("properties").is_some()) {
            let (fields, other) = fields(schema, object);
            return Form::Entries { fields, other };
        }
        if object_of.is_some() && array_of.is_none() {
            return Form::Map(kind(schema, item));
        }
    }
    if let Some(object) = opts.iter().find(|n| n.get("properties").is_some()) {
        let (fields, other) = fields(schema, object);
        return Form::Fields { fields, other };
    }
    if opts.iter().any(|n| types(n) == ["object"]) {
        let value = opts
            .iter()
            .find_map(|n| n.get("additionalProperties").filter(|a| a.is_object()));
        return Form::Map(value.map_or(Kind::Json, |v| kind(schema, v)));
    }
    match kind(schema, node) {
        Kind::Json => Form::Json,
        k => Form::Value(k),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parts as wowlua's schema has them (shortened).
    fn schema() -> Json {
        serde_json::json!({
            "properties": {
                "money": { "type": ["number", "null"], "description": "The character's money, in copper." },
                "client": { "type": ["string", "null"] },
                "cvars": { "type": ["object", "null"], "additionalProperties": { "$ref": "#/$defs/Scalar" } },
                "stats": { "type": ["object", "null"], "additionalProperties": true },
                "items": { "anyOf": [{ "$ref": "#/$defs/List" }, { "type": "null" }] },
                "position": { "anyOf": [{ "$ref": "#/$defs/Position" }, { "type": "null" }] },
                "appearances": { "anyOf": [{ "$ref": "#/$defs/Appearances" }, { "type": "null" }] },
                "leaderBoards": { "description": "kept, not applied" }
            },
            "$defs": {
                "Scalar": { "anyOf": [{ "type": "string" }, { "type": "number" }, { "type": "boolean" }] },
                "List": { "anyOf": [
                    { "type": "array", "items": { "$ref": "#/$defs/Item" } },
                    { "type": "object", "additionalProperties": { "$ref": "#/$defs/Item" } }
                ] },
                "Item": { "type": "object", "properties": {
                    "name": { "type": ["string", "null"] },
                    "quality": { "type": ["number", "null"] },
                    "cached": { "description": "In the client's cache.", "type": ["boolean", "null"] },
                    "icon": { "anyOf": [{ "$ref": "#/$defs/Scalar" }, { "type": "null" }] }
                }, "additionalProperties": true },
                "Position": { "type": "object", "properties": {
                    "zone": { "type": ["string", "null"] },
                    "world": { "description": "`UnitPosition`'s world coordinates." }
                }, "additionalProperties": true },
                "Numbers": { "anyOf": [
                    { "type": "array", "items": { "type": "number" } },
                    { "type": "object", "additionalProperties": { "type": "number" } }
                ] },
                "Appearances": { "type": "object", "properties": {
                    "collected": { "anyOf": [{ "$ref": "#/$defs/Numbers" }, { "type": "null" }] }
                } }
            }
        })
    }

    #[test]
    fn each_part_s_form_follows_its_schema() {
        let s = schema();
        assert_eq!(form(&s, "money"), Form::Value(Kind::Number));
        assert_eq!(form(&s, "client"), Form::Value(Kind::Text));
        assert_eq!(form(&s, "cvars"), Form::Map(Kind::Scalar));
        assert_eq!(form(&s, "stats"), Form::Map(Kind::Json));
        assert_eq!(form(&s, "leaderBoards"), Form::Json);
        let Form::Entries { fields, other } = form(&s, "items") else {
            panic!("{:?}", form(&s, "items"));
        };
        assert!(other);
        // (in the schema's order where serde_json keeps it: Studio's build)
        let mut kinds: Vec<(&str, Kind)> = fields
            .iter()
            .map(|f| (f.name.as_str(), f.kind.clone()))
            .collect();
        kinds.sort_by_key(|(n, _)| *n);
        assert_eq!(
            kinds,
            [
                ("cached", Kind::Bool),
                ("icon", Kind::Scalar),
                ("name", Kind::Text),
                ("quality", Kind::Number)
            ]
        );
        let cached = fields.iter().find(|f| f.name == "cached").unwrap();
        assert_eq!(cached.help, "In the client's cache.");
        let Form::Fields { fields, other } = form(&s, "position") else {
            panic!();
        };
        assert!(other);
        assert_eq!(
            fields.iter().find(|f| f.name == "world").unwrap().kind,
            Kind::Json
        );
        let Form::Fields { fields, .. } = form(&s, "appearances") else {
            panic!();
        };
        assert_eq!(fields[0].kind, Kind::Numbers);
    }
}
