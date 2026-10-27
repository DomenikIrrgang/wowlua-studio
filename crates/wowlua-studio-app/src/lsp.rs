//! Studio's side of wowlua's language server: the protocol's messages as
//! JSON values (the host runs the server, S002). It opens, changes and
//! closes the editor's files with the server, asks what the editor asks,
//! and turns the answers into [`Reply`]s. Positions are UTF-16 code units
//! on the wire and characters in the editor.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde_json::{Value as Json, json};

use crate::editor::{Document, Pos};
use crate::system::System;

/// A position as the server counts: a line and UTF-16 code units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Wire {
    pub line: usize,
    pub character: usize,
}

/// A problem the server reported.
#[derive(Clone, Debug, PartialEq)]
pub struct Diag {
    pub start: Wire,
    pub end: Wire,
    /// 1 error, 2 warning, 3 note.
    pub severity: u8,
    pub code: Option<String>,
    pub message: String,
    /// As the server sent it: quick fixes send it back.
    pub raw: Json,
}

/// A quick fix: its title and its edits per file.
#[derive(Clone, Debug, PartialEq)]
pub struct Fix {
    pub title: String,
    pub edits: BTreeMap<PathBuf, Vec<TextEdit>>,
}

impl Diag {
    pub fn is_error(&self) -> bool {
        self.severity == 1
    }
}

/// A place in a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub path: PathBuf,
    pub start: Wire,
    pub end: Wire,
}

/// A suggestion of completion.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub detail: String,
    /// 1-based LSP kind (function 3, variable 6, …).
    pub kind: u8,
    /// What it inserts, and over which range (the word typed so far when
    /// none is given).
    pub insert: String,
    pub range: Option<(Wire, Wire)>,
}

/// A symbol of the outline.
#[derive(Clone, Debug, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub detail: String,
    pub kind: u8,
    pub start: Wire,
    pub depth: usize,
}

/// One text replacement of an edit the server asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct TextEdit {
    pub start: Wire,
    pub end: Wire,
    pub text: String,
}

/// What an answer was for, with what it said.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    Hover {
        path: PathBuf,
        at: Pos,
        text: String,
    },
    Completion {
        path: PathBuf,
        at: Pos,
        items: Vec<Item>,
    },
    Signature {
        path: PathBuf,
        text: String,
        active: Option<(usize, usize)>,
    },
    Definition(Vec<Location>),
    References {
        name: String,
        locations: Vec<Location>,
    },
    /// A rename's edits per file, before they are made.
    Renamed {
        old: String,
        new: String,
        files: BTreeMap<PathBuf, Vec<TextEdit>>,
    },
    /// The server refused a rename, and why.
    RenameRefused {
        old: String,
        message: String,
    },
    Symbols {
        path: PathBuf,
        symbols: Vec<Symbol>,
    },
    /// The file's formatting; `then_save` when a save waits for it.
    Format {
        path: PathBuf,
        edits: Vec<TextEdit>,
        then_save: bool,
    },
    /// A request failed; the save it held up goes ahead.
    Failed {
        save: Option<PathBuf>,
        message: String,
    },
    /// A message to show (`window/showMessage`).
    Message(String),
    /// Inlay hints: where, and what; for the version asked about.
    Hints {
        path: PathBuf,
        version: u64,
        hints: Vec<(Wire, String)>,
    },
    /// Lenses: the line, the title, where its command points.
    Lenses {
        path: PathBuf,
        version: u64,
        lenses: Vec<(usize, String, Wire)>,
    },
    /// The quick fixes at the caret.
    Fixes {
        path: PathBuf,
        fixes: Vec<Fix>,
    },
    /// A check published its problems of a file (by the server's path).
    Published(PathBuf),
}

#[derive(Clone, Debug)]
enum Ask {
    Initialize,
    Hover(PathBuf, Pos),
    Completion(PathBuf, Pos),
    Signature(PathBuf),
    Definition,
    References(String),
    Rename(String, String),
    Symbols(PathBuf),
    Format(PathBuf, bool),
    Hints(PathBuf, u64),
    Lenses(PathBuf, u64),
    Fixes(PathBuf),
}

#[derive(Default)]
pub struct Lsp {
    root: Option<PathBuf>,
    next: i64,
    waiting: HashMap<i64, Ask>,
    /// The open files the server has, with the version it has: by the
    /// editor's path, with the path the server names it by.
    opened: HashMap<PathBuf, (u64, PathBuf)>,
    /// Problems by the server's (canonical) path.
    pub diagnostics: BTreeMap<PathBuf, Vec<Diag>>,
    /// Lines of the server's log (`window/logMessage`), for the output
    /// panel.
    pub log: Vec<String>,
    /// The server answered `initialize`.
    pub ready: bool,
}

/// Files the server reads: wowlua and Lua.
pub fn served(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("wowlua" | "lua")
    )
}

impl Lsp {
    /// Starts the server for the workspace at `root`.
    pub fn start(&mut self, root: &Path, system: &dyn System) {
        *self = Lsp::default();
        let root = system.canonical(root);
        let uri = uri_of(&root);
        self.root = Some(root);
        self.request(
            system,
            Ask::Initialize,
            "initialize",
            json!({
                "processId": null,
                "rootUri": uri,
                "workspaceFolders": [{ "uri": uri, "name": "workspace" }],
                "capabilities": {
                    "textDocument": {
                        "hover": { "contentFormat": ["markdown", "plaintext"] },
                        "completion": { "completionItem": { "snippetSupport": false } },
                        "publishDiagnostics": {},
                    },
                    "general": { "positionEncodings": ["utf-16"] },
                },
            }),
        );
    }

    pub fn stop(&mut self, system: &dyn System) {
        if self.root.is_some() {
            system.lsp_stop();
        }
        *self = Lsp::default();
    }

    pub fn running(&self) -> bool {
        self.root.is_some()
    }

    fn request(&mut self, system: &dyn System, ask: Ask, method: &str, params: Json) {
        self.next += 1;
        self.waiting.insert(self.next, ask);
        system.lsp_send(
            json!({ "jsonrpc": "2.0", "id": self.next, "method": method, "params": params }),
        );
    }

    fn notify(&self, system: &dyn System, method: &str, params: Json) {
        system.lsp_send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Tells the server about the open files: opened, changed (the whole
    /// text), closed.
    pub fn sync<'a>(&mut self, docs: impl Iterator<Item = &'a Document>, system: &dyn System) {
        if !self.running() {
            return;
        }
        let mut seen = Vec::new();
        for doc in docs.filter(|d| served(&d.path)) {
            seen.push(doc.path.clone());
            match self.opened.get(&doc.path) {
                Some((version, _)) if *version == doc.version => {}
                Some((_, canonical)) => {
                    let uri = uri_of(canonical);
                    self.notify(
                        system,
                        "textDocument/didChange",
                        json!({
                            "textDocument": { "uri": uri, "version": doc.version },
                            "contentChanges": [{ "text": doc.text() }],
                        }),
                    );
                    let canonical = canonical.clone();
                    self.opened
                        .insert(doc.path.clone(), (doc.version, canonical));
                }
                None => {
                    let canonical = system.canonical(&doc.path);
                    self.notify(
                        system,
                        "textDocument/didOpen",
                        json!({
                            "textDocument": {
                                "uri": uri_of(&canonical),
                                "languageId": "wowlua",
                                "version": doc.version,
                                "text": doc.text(),
                            },
                        }),
                    );
                    self.opened
                        .insert(doc.path.clone(), (doc.version, canonical));
                }
            }
        }
        let closed: Vec<PathBuf> = self
            .opened
            .keys()
            .filter(|p| !seen.contains(p))
            .cloned()
            .collect();
        for path in closed {
            if let Some((_, canonical)) = self.opened.remove(&path) {
                self.notify(
                    system,
                    "textDocument/didClose",
                    json!({ "textDocument": { "uri": uri_of(&canonical) } }),
                );
            }
        }
    }

    /// The file was written.
    pub fn saved(&self, path: &Path, system: &dyn System) {
        if let Some((_, canonical)) = self.opened.get(path) {
            self.notify(
                system,
                "textDocument/didSave",
                json!({ "textDocument": { "uri": uri_of(canonical) } }),
            );
        }
    }

    /// The server's name for an open file.
    pub fn canonical_of(&self, path: &Path) -> Option<&Path> {
        self.opened.get(path).map(|(_, c)| c.as_path())
    }

    /// The problems in an open file.
    pub fn diagnostics_of(&self, path: &Path) -> &[Diag] {
        self.canonical_of(path)
            .and_then(|c| self.diagnostics.get(c))
            .map_or(&[], Vec::as_slice)
    }

    fn at(&self, doc: &Document, pos: Pos) -> Option<Json> {
        let (_, canonical) = self.opened.get(&doc.path)?;
        Some(json!({
            "textDocument": { "uri": uri_of(canonical) },
            "position": wire_json(to_wire(doc, pos)),
        }))
    }

    pub fn hover(&mut self, doc: &Document, pos: Pos, system: &dyn System) {
        if let Some(params) = self.at(doc, pos) {
            self.request(
                system,
                Ask::Hover(doc.path.clone(), pos),
                "textDocument/hover",
                params,
            );
        }
    }

    pub fn complete(&mut self, doc: &Document, pos: Pos, system: &dyn System) {
        if let Some(params) = self.at(doc, pos) {
            self.request(
                system,
                Ask::Completion(doc.path.clone(), pos),
                "textDocument/completion",
                params,
            );
        }
    }

    pub fn signature(&mut self, doc: &Document, pos: Pos, system: &dyn System) {
        if let Some(params) = self.at(doc, pos) {
            self.request(
                system,
                Ask::Signature(doc.path.clone()),
                "textDocument/signatureHelp",
                params,
            );
        }
    }

    pub fn definition(&mut self, doc: &Document, pos: Pos, system: &dyn System) {
        if let Some(params) = self.at(doc, pos) {
            self.request(system, Ask::Definition, "textDocument/definition", params);
        }
    }

    pub fn references(&mut self, doc: &Document, pos: Pos, name: String, system: &dyn System) {
        if let Some(mut params) = self.at(doc, pos) {
            params["context"] = json!({ "includeDeclaration": true });
            self.request(
                system,
                Ask::References(name),
                "textDocument/references",
                params,
            );
        }
    }

    pub fn rename(
        &mut self,
        doc: &Document,
        pos: Pos,
        old: String,
        name: &str,
        system: &dyn System,
    ) {
        if let Some(mut params) = self.at(doc, pos) {
            params["newName"] = json!(name);
            self.request(
                system,
                Ask::Rename(old, name.to_owned()),
                "textDocument/rename",
                params,
            );
        }
    }

    pub fn symbols(&mut self, doc: &Document, system: &dyn System) {
        if let Some((_, canonical)) = self.opened.get(&doc.path) {
            let params = json!({ "textDocument": { "uri": uri_of(canonical) } });
            self.request(
                system,
                Ask::Symbols(doc.path.clone()),
                "textDocument/documentSymbol",
                params,
            );
        }
    }

    /// Asks for the whole file's inlay hints and lenses (after a check).
    pub fn decorations(&mut self, doc: &Document, system: &dyn System) {
        let Some((_, canonical)) = self.opened.get(&doc.path) else {
            return;
        };
        let uri = uri_of(canonical);
        let lines = doc.line_count();
        self.request(
            system,
            Ask::Hints(doc.path.clone(), doc.version),
            "textDocument/inlayHint",
            json!({
                "textDocument": { "uri": uri },
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": lines, "character": 0 } },
            }),
        );
        self.request(
            system,
            Ask::Lenses(doc.path.clone(), doc.version),
            "textDocument/codeLens",
            json!({ "textDocument": { "uri": uri } }),
        );
    }

    /// Asks for the fixes of the problems at `pos`.
    pub fn fixes(&mut self, doc: &Document, pos: Pos, system: &dyn System) -> bool {
        let wire = to_wire(doc, pos);
        let here: Vec<Json> = self
            .diagnostics_of(&doc.path)
            .iter()
            .filter(|d| d.start <= wire && wire <= d.end)
            .map(|d| d.raw.clone())
            .collect();
        let Some((_, canonical)) = self.opened.get(&doc.path) else {
            return false;
        };
        if here.is_empty() {
            return false;
        }
        let params = json!({
            "textDocument": { "uri": uri_of(canonical) },
            "range": { "start": wire_json(wire), "end": wire_json(wire) },
            "context": { "diagnostics": here },
        });
        self.request(
            system,
            Ask::Fixes(doc.path.clone()),
            "textDocument/codeAction",
            params,
        );
        true
    }

    /// Asks for the file's formatting; `then_save` when a save waits.
    /// Returns false when the server does not have the file.
    pub fn format(&mut self, doc: &Document, then_save: bool, system: &dyn System) -> bool {
        let Some((_, canonical)) = self.opened.get(&doc.path) else {
            return false;
        };
        let params = json!({
            "textDocument": { "uri": uri_of(canonical) },
            "options": { "tabSize": 4, "insertSpaces": true },
        });
        self.request(
            system,
            Ask::Format(doc.path.clone(), then_save),
            "textDocument/formatting",
            params,
        );
        true
    }

    /// Takes a message of the server.
    pub fn receive(&mut self, message: &Json, system: &dyn System) -> Option<Reply> {
        if let Some(method) = message["method"].as_str() {
            let params = &message["params"];
            match method {
                "textDocument/publishDiagnostics" => {
                    let path = params["uri"].as_str().and_then(path_of)?;
                    let list = params["diagnostics"]
                        .as_array()
                        .map(|a| a.iter().map(diag).collect())
                        .unwrap_or_default();
                    self.diagnostics.insert(path.clone(), list);
                    return Some(Reply::Published(path));
                }
                "window/logMessage" => {
                    if let Some(text) = params["message"].as_str() {
                        self.log.push(text.to_owned());
                    }
                }
                "window/showMessage" => {
                    return params["message"]
                        .as_str()
                        .map(|m| Reply::Message(m.to_owned()));
                }
                _ => {}
            }
            // A request of the server's: answered with nothing.
            if !message["id"].is_null() {
                system.lsp_send(json!({ "jsonrpc": "2.0", "id": message["id"], "result": null }));
            }
            return None;
        }
        let id = message["id"].as_i64()?;
        let ask = self.waiting.remove(&id)?;
        let result = &message["result"];
        if let Some(error) = message.get("error").filter(|e| !e.is_null()) {
            let text = error["message"]
                .as_str()
                .unwrap_or("the language server failed")
                .to_owned();
            if let Ask::Rename(old, _) = ask {
                return Some(Reply::RenameRefused { old, message: text });
            }
            return Some(Reply::Failed {
                save: match ask {
                    Ask::Format(path, true) => Some(path),
                    _ => None,
                },
                message: text,
            });
        }
        Some(match ask {
            Ask::Initialize => {
                self.ready = true;
                self.notify(system, "initialized", json!({}));
                return None;
            }
            Ask::Hover(path, at) => Reply::Hover {
                path,
                at,
                text: hover_text(&result["contents"]),
            },
            Ask::Completion(path, at) => {
                let items = result
                    .as_array()
                    .or_else(|| result["items"].as_array())
                    .map(|a| a.iter().map(item).collect())
                    .unwrap_or_default();
                Reply::Completion { path, at, items }
            }
            Ask::Signature(path) => {
                let sigs = result["signatures"].as_array()?;
                let active = result["activeSignature"].as_u64().unwrap_or(0) as usize;
                let sig = sigs.get(active).or(sigs.first())?;
                let label = sig["label"].as_str().unwrap_or("").to_owned();
                let param = result["activeParameter"]
                    .as_u64()
                    .or_else(|| sig["activeParameter"].as_u64())
                    .and_then(|i| sig["parameters"].get(i as usize));
                let active = param.and_then(|p| match &p["label"] {
                    Json::Array(r) => Some((
                        utf16_to_chars(&label, r.first()?.as_u64()? as usize),
                        utf16_to_chars(&label, r.get(1)?.as_u64()? as usize),
                    )),
                    Json::String(s) => label.find(s.as_str()).map(|b| {
                        let start = label[..b].chars().count();
                        (start, start + s.chars().count())
                    }),
                    _ => None,
                });
                Reply::Signature {
                    path,
                    text: label,
                    active,
                }
            }
            Ask::Definition => Reply::Definition(locations(result)),
            Ask::References(name) => Reply::References {
                name,
                locations: locations(result),
            },
            Ask::Rename(old, new) => Reply::Renamed {
                old,
                new,
                files: workspace_edit(result),
            },
            Ask::Symbols(path) => {
                let mut symbols = Vec::new();
                if let Some(list) = result.as_array() {
                    flatten(list, 0, &mut symbols);
                }
                Reply::Symbols { path, symbols }
            }
            Ask::Hints(path, version) => Reply::Hints {
                path,
                version,
                hints: result
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|h| {
                        let label = match &h["label"] {
                            Json::String(s) => s.clone(),
                            Json::Array(parts) => {
                                parts.iter().filter_map(|p| p["value"].as_str()).collect()
                            }
                            _ => String::new(),
                        };
                        (wire(&h["position"]), label)
                    })
                    .collect(),
            },
            Ask::Lenses(path, version) => Reply::Lenses {
                path,
                version,
                lenses: result
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|l| {
                        let title = l["command"]["title"].as_str()?.to_owned();
                        let at = wire(&l["command"]["arguments"][1]);
                        Some((l["range"]["start"]["line"].as_u64()? as usize, title, at))
                    })
                    .collect(),
            },
            Ask::Fixes(path) => Reply::Fixes {
                path,
                fixes: result
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|a| Fix {
                        title: a["title"].as_str().unwrap_or("fix").to_owned(),
                        edits: workspace_edit(&a["edit"]),
                    })
                    .collect(),
            },
            Ask::Format(path, then_save) => Reply::Format {
                path,
                edits: result
                    .as_array()
                    .map(|a| a.iter().map(text_edit).collect())
                    .unwrap_or_default(),
                then_save,
            },
        })
    }
}

fn wire(v: &Json) -> Wire {
    Wire {
        line: v["line"].as_u64().unwrap_or(0) as usize,
        character: v["character"].as_u64().unwrap_or(0) as usize,
    }
}

fn wire_json(w: Wire) -> Json {
    json!({ "line": w.line, "character": w.character })
}

fn diag(v: &Json) -> Diag {
    Diag {
        start: wire(&v["range"]["start"]),
        end: wire(&v["range"]["end"]),
        severity: v["severity"].as_u64().unwrap_or(1) as u8,
        code: match &v["code"] {
            Json::String(s) => Some(s.clone()),
            Json::Number(n) => Some(n.to_string()),
            _ => None,
        },
        message: v["message"].as_str().unwrap_or("").to_owned(),
        raw: v.clone(),
    }
}

fn item(v: &Json) -> Item {
    let label = v["label"].as_str().unwrap_or("").to_owned();
    let (insert, range) = match v.get("textEdit").filter(|t| !t.is_null()) {
        Some(edit) => {
            let r = if edit["range"].is_null() {
                &edit["replace"]
            } else {
                &edit["range"]
            };
            (
                edit["newText"].as_str().unwrap_or(&label).to_owned(),
                Some((wire(&r["start"]), wire(&r["end"]))),
            )
        }
        None => (v["insertText"].as_str().unwrap_or(&label).to_owned(), None),
    };
    Item {
        detail: v["detail"].as_str().unwrap_or("").to_owned(),
        kind: v["kind"].as_u64().unwrap_or(0) as u8,
        label,
        insert,
        range,
    }
}

fn text_edit(v: &Json) -> TextEdit {
    TextEdit {
        start: wire(&v["range"]["start"]),
        end: wire(&v["range"]["end"]),
        text: v["newText"].as_str().unwrap_or("").to_owned(),
    }
}

fn locations(v: &Json) -> Vec<Location> {
    let one = |l: &Json| -> Option<Location> {
        let uri = l["uri"].as_str().or_else(|| l["targetUri"].as_str())?;
        let range = if l["range"].is_null() {
            &l["targetSelectionRange"]
        } else {
            &l["range"]
        };
        Some(Location {
            path: path_of(uri)?,
            start: wire(&range["start"]),
            end: wire(&range["end"]),
        })
    };
    match v {
        Json::Array(list) => list.iter().filter_map(one).collect(),
        Json::Object(_) => one(v).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn workspace_edit(v: &Json) -> BTreeMap<PathBuf, Vec<TextEdit>> {
    let mut out: BTreeMap<PathBuf, Vec<TextEdit>> = BTreeMap::new();
    if let Some(changes) = v["changes"].as_object() {
        for (uri, edits) in changes {
            if let Some(path) = path_of(uri) {
                out.entry(path)
                    .or_default()
                    .extend(edits.as_array().into_iter().flatten().map(text_edit));
            }
        }
    }
    for change in v["documentChanges"].as_array().into_iter().flatten() {
        if let Some(path) = change["textDocument"]["uri"].as_str().and_then(path_of) {
            out.entry(path).or_default().extend(
                change["edits"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(text_edit),
            );
        }
    }
    out
}

fn flatten(list: &[Json], depth: usize, out: &mut Vec<Symbol>) {
    for s in list {
        let range = if s["selectionRange"].is_null() {
            if s["range"].is_null() {
                &s["location"]["range"]
            } else {
                &s["range"]
            }
        } else {
            &s["selectionRange"]
        };
        out.push(Symbol {
            name: s["name"].as_str().unwrap_or("").to_owned(),
            detail: s["detail"].as_str().unwrap_or("").to_owned(),
            kind: s["kind"].as_u64().unwrap_or(0) as u8,
            start: wire(&range["start"]),
            depth,
        });
        if let Some(children) = s["children"].as_array() {
            flatten(children, depth + 1, out);
        }
    }
}

fn hover_text(contents: &Json) -> String {
    match contents {
        Json::String(s) => s.clone(),
        Json::Object(o) => o
            .get("value")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_owned(),
        Json::Array(list) => list.iter().map(hover_text).collect::<Vec<_>>().join("\n\n"),
        _ => String::new(),
    }
}

/// A position in characters as UTF-16 code units.
pub fn to_wire(doc: &Document, pos: Pos) -> Wire {
    let line = doc.line(pos.line);
    Wire {
        line: pos.line,
        character: line.chars().take(pos.col).map(char::len_utf16).sum(),
    }
}

/// UTF-16 code units as a position in characters.
pub fn from_wire(doc: &Document, w: Wire) -> Pos {
    Pos::new(w.line, utf16_to_chars(doc.line(w.line), w.character))
}

fn utf16_to_chars(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (i, c) in text.chars().enumerate() {
        if seen >= units {
            return i;
        }
        seen += c.len_utf16();
    }
    text.chars().count()
}

/// A path as a `file:` URI.
pub fn uri_of(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !text.starts_with('/') {
        // `C:/…` on Windows.
        out.push('/');
    }
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A `file:` URI's path.
pub fn path_of(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut bytes = Vec::with_capacity(rest.len());
    let raw = rest.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%'
            && i + 2 < raw.len()
            && let Ok(b) = u8::from_str_radix(std::str::from_utf8(&raw[i + 1..i + 3]).ok()?, 16)
        {
            bytes.push(b);
            i += 3;
        } else {
            bytes.push(raw[i]);
            i += 1;
        }
    }
    let text = String::from_utf8(bytes).ok()?;
    // `/C:/…` on Windows.
    let text = match text.as_bytes() {
        [b'/', d, b':', ..] if d.is_ascii_alphabetic() => text[1..].to_owned(),
        _ => text,
    };
    Some(PathBuf::from(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_and_paths() {
        let p = Path::new("/Users/me/My Addon/src/a.wowlua");
        let uri = uri_of(p);
        assert_eq!(uri, "file:///Users/me/My%20Addon/src/a.wowlua");
        assert_eq!(path_of(&uri).unwrap(), p);
    }

    #[test]
    fn positions_count_utf16_on_the_wire() {
        let doc = Document::new(PathBuf::from("a.wowlua"), "x = \"😀b\"");
        let w = to_wire(&doc, Pos::new(0, 7));
        assert_eq!(w.character, 8);
        assert_eq!(from_wire(&doc, w), Pos::new(0, 7));
    }

    #[test]
    fn diagnostics_and_completions_are_read() {
        let mut lsp = Lsp::default();
        let system = crate::system::NoSystem;
        lsp.receive(
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": { "uri": "file:///w/a.wowlua", "diagnostics": [{
                    "range": { "start": { "line": 2, "character": 4 }, "end": { "line": 2, "character": 8 } },
                    "severity": 1, "code": "E2003", "message": "no field `roel`",
                }] },
            }),
            &system,
        );
        let d = &lsp.diagnostics[Path::new("/w/a.wowlua")][0];
        assert_eq!(d.code.as_deref(), Some("E2003"));
        assert!(d.is_error());
    }
}
