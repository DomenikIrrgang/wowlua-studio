//! A conversation with Claude (M14, S097): Studio runs the Claude Code the
//! user signed in to, `claude -p` with JSON in and out, its own tools off
//! and Studio's given over MCP (the link, S096), the workspace's own Claude
//! Code settings left out (`--setting-sources user`). What it tells comes
//! back as conversation events; a message is a JSON line on its input.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

use serde_json::{Value as Json, json};
use wowlua_studio_app::system::{ClaudeStart, ConversationEvent};

/// Studio's tools in Claude Code: the MCP server's name in the link's
/// config, and its tools' prefix.
const SERVER: &str = "wowlua-studio";

/// A running conversation: its process and its input.
pub struct Conversation {
    child: Child,
    input: ChildStdin,
}

impl Conversation {
    /// Starts Claude Code in `root` with the first message.
    pub fn start(
        root: &Path,
        start: &ClaudeStart,
        tell: impl Fn(ConversationEvent) + Send + 'static,
    ) -> Result<Conversation, String> {
        let mut c = Command::new(&start.claude);
        c.args(args(start))
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c
            .spawn()
            .map_err(|e| format!("{}: {e}", start.claude.display()))?;
        let input = child.stdin.take().ok_or("no input")?;
        let out = child.stdout.take().ok_or("no output")?;
        let err = child.stderr.take().ok_or("no errors")?;
        // (its last error lines, for when it ends badly)
        let said = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let kept = said.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if let Ok(mut s) = kept.lock() {
                    s.push(line);
                    let n = s.len();
                    if n > 20 {
                        s.drain(..n - 20);
                    }
                }
            }
        });
        std::thread::Builder::new()
            .name("claude".into())
            .spawn(move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    for event in parse(&line) {
                        tell(event);
                    }
                }
                let error = said
                    .lock()
                    .ok()
                    .map(|s| s.join("\n"))
                    .filter(|s| !s.trim().is_empty());
                tell(ConversationEvent::Ended(error));
            })
            .map_err(|e| e.to_string())?;
        let mut conversation = Conversation { child, input };
        conversation.send(&start.prompt)?;
        Ok(conversation)
    }

    /// The user's next message.
    pub fn send(&mut self, text: &str) -> Result<(), String> {
        let msg = json!({
            "type": "user",
            "message": { "role": "user", "content": text },
            "parent_tool_use_id": null,
        });
        writeln!(self.input, "{msg}")
            .and_then(|_| self.input.flush())
            .map_err(|e| format!("Claude Code does not listen: {e}"))
    }

    /// Ends Claude Code (its turn unfinished; the conversation goes on
    /// with `--resume`).
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Conversation {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Claude Code's arguments for a conversation.
pub fn args(start: &ClaudeStart) -> Vec<String> {
    let mut a: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--strict-mcp-config",
        "--setting-sources",
        "user",
        "--tools",
        "",
    ]
    .map(String::from)
    .to_vec();
    a.push("--mcp-config".into());
    a.push(start.mcp_config.clone());
    a.push("--allowedTools".into());
    a.push(format!("mcp__{SERVER}"));
    a.push("--model".into());
    a.push(start.model.clone());
    if let Some(effort) = &start.effort {
        a.push("--effort".into());
        a.push(effort.clone());
    }
    if let Some(session) = &start.resume {
        a.push("--resume".into());
        a.push(session.clone());
    }
    a
}

/// A tool's name as the conversation shows it: Studio's without the
/// server's prefix.
fn tool_name(name: &str) -> String {
    name.strip_prefix(&format!("mcp__{SERVER}__"))
        .unwrap_or(name)
        .to_string()
}

/// What one line of Claude Code's output tells.
pub fn parse(line: &str) -> Vec<ConversationEvent> {
    let Ok(j) = serde_json::from_str::<Json>(line) else {
        return Vec::new();
    };
    // (a subagent's messages are the tool call's that started it)
    if !j["parent_tool_use_id"].is_null() {
        return Vec::new();
    }
    let s = |v: &Json| v.as_str().unwrap_or("").to_string();
    match j["type"].as_str().unwrap_or("") {
        "system" if j["subtype"] == "init" => vec![ConversationEvent::Started {
            session: s(&j["session_id"]),
            model: s(&j["model"]),
        }],
        "stream_event" => {
            let e = &j["event"];
            if e["type"] == "content_block_delta" && e["delta"]["type"] == "text_delta" {
                vec![ConversationEvent::TextDelta(s(&e["delta"]["text"]))]
            } else {
                Vec::new()
            }
        }
        "assistant" => j["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| match b["type"].as_str()? {
                "text" => Some(ConversationEvent::Text(s(&b["text"]))),
                "tool_use" => Some(ConversationEvent::ToolUse {
                    id: s(&b["id"]),
                    name: tool_name(b["name"].as_str().unwrap_or("")),
                    input: b["input"].to_string(),
                }),
                _ => None,
            })
            .collect(),
        "user" => j["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "tool_result")
            .map(|b| {
                let (mut text, mut image) = (String::new(), false);
                match &b["content"] {
                    Json::String(t) => text = t.clone(),
                    Json::Array(parts) => {
                        for p in parts {
                            match p["type"].as_str() {
                                Some("text") => {
                                    if !text.is_empty() {
                                        text.push('\n');
                                    }
                                    text.push_str(p["text"].as_str().unwrap_or(""));
                                }
                                Some("image") => image = true,
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                ConversationEvent::ToolResult {
                    id: s(&b["tool_use_id"]),
                    text,
                    error: b["is_error"] == true,
                    image,
                }
            })
            .collect(),
        "result" => vec![ConversationEvent::TurnDone {
            cost: j["total_cost_usd"].as_f64().unwrap_or(0.0),
            error: (j["is_error"] == true).then(|| {
                Some(s(&j["result"]))
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| s(&j["subtype"]))
            }),
            denied: j["permission_denials"].as_array().map_or(0, Vec::len),
        }],
        "rate_limit_event" => {
            let i = &j["rate_limit_info"];
            vec![ConversationEvent::Limit {
                rejected: i["status"] == "rejected",
                resets_at: i["resetsAt"].as_u64(),
                utilization: i["utilization"].as_f64(),
            }]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_s_lines_become_the_conversation() {
        let lines = [
            r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-fable-5-1","tools":[],"parent_tool_use_id":null}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I'll "}},"parent_tool_use_id":null,"session_id":"s1"}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"I'll read it."},{"type":"tool_use","id":"t1","name":"mcp__wowlua-studio__read_file","input":{"path":"src/a.wowlua"}}]},"parent_tool_use_id":null,"session_id":"s1"}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"     1\tprint(1)"}],"is_error":false}]},"parent_tool_use_id":null}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":[{"type":"image","source":{}},{"type":"text","text":"800x600"}]}]},"parent_tool_use_id":null}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"done","total_cost_usd":0.0123,"num_turns":2,"permission_denials":[]}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1793500000,"utilization":1.0},"session_id":"s1"}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"a subagent's"}]},"parent_tool_use_id":"t9"}"#,
            "not json",
        ];
        let events: Vec<ConversationEvent> = lines.iter().flat_map(|l| parse(l)).collect();
        assert_eq!(
            events,
            vec![
                ConversationEvent::Started {
                    session: "s1".into(),
                    model: "claude-fable-5-1".into()
                },
                ConversationEvent::TextDelta("I'll ".into()),
                ConversationEvent::Text("I'll read it.".into()),
                ConversationEvent::ToolUse {
                    id: "t1".into(),
                    name: "read_file".into(),
                    input: r#"{"path":"src/a.wowlua"}"#.into()
                },
                ConversationEvent::ToolResult {
                    id: "t1".into(),
                    text: "     1\tprint(1)".into(),
                    error: false,
                    image: false
                },
                ConversationEvent::ToolResult {
                    id: "t2".into(),
                    text: "800x600".into(),
                    error: false,
                    image: true
                },
                ConversationEvent::TurnDone {
                    cost: 0.0123,
                    error: None,
                    denied: 0
                },
                ConversationEvent::Limit {
                    rejected: true,
                    resets_at: Some(1793500000),
                    utilization: Some(1.0)
                },
            ]
        );
    }

    /// The whole chain with this computer's Claude Code, by hand
    /// (`--ignored`; it uses a little of the plan): Claude Code starts
    /// Studio's debug binary as the bridge, which reaches Studio's tools
    /// in this process; the tool calls and their answers come back.
    #[test]
    #[ignore]
    fn a_real_conversation_reaches_studio_s_tools() {
        use crate::claude_tools::{AskApp, StudioTools};
        use std::collections::BTreeMap;
        use std::sync::{Arc, Mutex};
        use wowlua_studio_app::system::ClaudeReply;

        let claude = crate::claude::find().expect("Claude Code is installed");
        let bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/debug/wowlua-studio");
        assert!(bin.exists(), "cargo build -p wowlua-studio first");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("wowlua.json"),
            r#"{ "package": { "name": "counter", "version": "1.0.0" }, "flavors": { "standard": {} }, "addon": { "title": "Counter" } }"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/counter.wowlua"),
            "print(\"counter\")\n",
        )
        .unwrap();
        let permissions: BTreeMap<String, String> = [
            ("files.read", "allow"),
            ("files.edit", "never"),
            ("git.push", "never"),
        ]
        .iter()
        .map(|(g, p)| (g.to_string(), p.to_string()))
        .collect();
        let permissions = Arc::new(Mutex::new(permissions));
        let ask: AskApp = Arc::new(|_| Ok(ClaudeReply::Allowed(true)));
        let root = dir.path().to_path_buf();
        let link = crate::mcp_link::Link::open(move || {
            let changes = Arc::new(Mutex::new(crate::claude_changes::Changes::new(&root)));
            let mut tools =
                StudioTools::new(root.clone(), permissions.clone(), ask.clone(), changes);
            Box::new(move |msg| tools.handle(msg))
        })
        .unwrap();
        let mcp_config = json!({ "mcpServers": { SERVER: {
            "type": "stdio", "command": bin, "args": ["mcp-bridge", link.socket, link.token],
        } } })
        .to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        let start = ClaudeStart {
            claude,
            mcp_config,
            model: "haiku".into(),
            effort: None,
            resume: None,
            prompt: "Use the list_files tool to list the workspace's files, then read src/counter.wowlua with the read_file tool, then answer with only the word done.".into(),
        };
        let mut conversation = Conversation::start(dir.path(), &start, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        let mut events = Vec::new();
        while let Ok(e) = rx.recv_timeout(std::time::Duration::from_secs(180)) {
            let done = matches!(
                e,
                ConversationEvent::TurnDone { .. } | ConversationEvent::Ended(_)
            );
            events.push(e);
            if done {
                break;
            }
        }
        conversation.stop();
        let names: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                ConversationEvent::ToolUse { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        println!("tools called: {names:?}");
        for e in &events {
            match e {
                ConversationEvent::ToolResult { text, error, .. } => {
                    println!(
                        "answer (error {error}): {}",
                        text.lines().next().unwrap_or("")
                    )
                }
                ConversationEvent::Text(t) => println!("text: {t}"),
                ConversationEvent::TurnDone { cost, error, .. } => {
                    println!("turn done: cost {cost}, error {error:?}")
                }
                ConversationEvent::Ended(e) => println!("ended: {e:?}"),
                _ => {}
            }
        }
        assert!(
            names.contains(&"list_files") && names.contains(&"read_file"),
            "{names:?}"
        );
        assert!(
            events.iter().any(|e| matches!(e, ConversationEvent::ToolResult { text, error: false, .. } if text.contains("src/counter.wowlua"))),
            "the files listed"
        );
        assert!(
            events.iter().any(|e| matches!(e, ConversationEvent::ToolResult { text, error: false, .. } if text.contains("print(\"counter\")"))),
            "the file read"
        );
    }

    #[test]
    fn only_studio_s_tools_and_the_user_s_own_settings() {
        let a = args(&ClaudeStart {
            claude: "claude".into(),
            mcp_config: "{}".into(),
            model: "fable".into(),
            effort: None,
            resume: Some("s1".into()),
            prompt: "hi".into(),
        });
        let joined = a.join(" ");
        assert!(joined.contains("--setting-sources user"), "{joined}");
        assert!(joined.contains("--strict-mcp-config"), "{joined}");
        assert!(
            joined.contains("--allowedTools mcp__wowlua-studio"),
            "{joined}"
        );
        assert!(joined.contains("--resume s1"), "{joined}");
        let tools = a.iter().position(|x| x == "--tools").unwrap();
        assert_eq!(a[tools + 1], "", "Claude Code's own tools off");
        assert!(!a.contains(&"--effort".to_string()));
    }
}
