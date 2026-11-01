//! Studio's tools for Claude (M14, S096): wowlua's (`wowlua mcp`'s, on a
//! session of Claude's own, its package commands on the wowlua built into
//! Studio) and Studio's own, the workspace's files and git, each checked
//! against the tool groups' permissions (S091, S095): `never` leaves a
//! tool out, `ask` waits for the user's answer in Studio. A test run is
//! the tests page's, so its results show there too.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value as Json, json};
use wowlua_debug::dap::{CommandOutput, Launcher, PackageCommand};
use wowlua_debug::session::{self, Options};
use wowlua_driver::Session;
use wowlua_driver::project::{LoadOptions, load_projects};
use wowlua_manifest::model::PackageKind;
use wowlua_studio_app::system::{ClaudeEvent, ClaudeReply, TestFile, TestOutcome, TestRequest};
use wowlua_test::runs::{self, Switches};

use crate::tests::first_error;

/// Asks the app (a question for the user, a test run) and waits for its
/// answer.
pub type AskApp = Arc<dyn Fn(ClaudeEvent) -> Result<ClaudeReply, String> + Send + Sync>;

/// The tool group of each tool (S095's keys).
fn group(tool: &str) -> &'static str {
    match tool {
        "read_file" | "list_files" | "search_files" => "files.read",
        "write_file" | "edit_file" | "delete_file" => "files.edit",
        "check" | "fmt" => "wowlua.check",
        "test" => "wowlua.test",
        "build" => "wowlua.build",
        "start_session" | "stop_session" | "screenshot" | "frame_tree" | "find_frames"
        | "inspect" | "pick" | "units" | "events" | "output" => "game.look",
        "click" | "hover" | "drag" | "type_text" | "press_key" | "slash" | "chat" | "advance"
        | "fire_event" | "world" | "set_screen" | "save_world" => "game.play",
        "git_status" | "git_diff" | "git_log" => "git.read",
        "git_commit" => "git.commit",
        "git_push" | "git_pull" => "git.push",
        "git_branch" => "git.branches",
        // wowlua's own docs: only read, always allowed (S099)
        "wowlua_docs" | "wowlua_reference" => "docs",
        // the debugger's and the profiler's
        _ => "debugger",
    }
}

/// One connection's tools.
pub struct StudioTools {
    wowlua: wowlua_debug::mcp::Server,
    root: PathBuf,
    permissions: Permissions,
    ask: AskApp,
    /// What Claude changes, kept for the review and the checkpoints (S098).
    changes: crate::claude_changes::Shared,
}

/// Each tool group's permission, shared with Studio: the conversation
/// changes them as it runs.
pub type Permissions = Arc<Mutex<BTreeMap<String, String>>>;

impl StudioTools {
    /// `permissions`: each group's (`allow`, `ask`, `never`), as the
    /// conversation started.
    pub fn new(
        root: PathBuf,
        permissions: Permissions,
        ask: AskApp,
        changes: crate::claude_changes::Shared,
    ) -> StudioTools {
        let launcher = Arc::new(StudioLauncher {
            root: root.clone(),
            ask: ask.clone(),
            changes: changes.clone(),
        });
        StudioTools {
            wowlua: wowlua_debug::mcp::Server::new(launcher),
            root,
            permissions,
            ask,
            changes,
        }
    }

    /// Runs `change` on the file at `path` (in the workspace), kept as it
    /// was before and as Claude left it.
    fn changing(&self, path: &Path, change: impl FnOnce() -> Json) -> Json {
        let rel = path
            .strip_prefix(&self.root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if let Ok(mut c) = self.changes.lock() {
            c.before(&rel);
        }
        let answer = change();
        if let Ok(mut c) = self.changes.lock() {
            c.after(&rel);
        }
        answer
    }

    fn permission(&self, tool: &str) -> String {
        if group(tool) == "docs" {
            return "allow".into();
        }
        self.permissions
            .lock()
            .ok()
            .and_then(|p| p.get(group(tool)).cloned())
            .unwrap_or_else(|| "ask".into())
    }

    /// Answers one JSON-RPC message (`None` for a notification).
    pub fn handle(&mut self, msg: &Json) -> Option<Json> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Json::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result = match method {
            "initialize" => {
                let mut r = self.wowlua.handle(msg)?["result"].clone();
                r["serverInfo"] =
                    json!({ "name": "wowlua-studio", "version": env!("CARGO_PKG_VERSION") });
                r["instructions"] = json!(format!(
                    "wowlua studio's tools for the workspace {}: its files (read_file, list_files, search_files; write_file, edit_file and delete_file may ask the user first), git (git_status, git_diff, git_log, git_commit, git_push, git_pull, git_branch), and wowlua's: {}",
                    self.root.display(),
                    r["instructions"].as_str().unwrap_or("")
                ));
                r
            }
            "tools/list" => {
                let listed = self.wowlua.handle(msg)?;
                let mut tools: Vec<Json> = listed["result"]["tools"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                tools.extend(own_tools());
                tools.retain(|t| self.permission(t["name"].as_str().unwrap_or("")) != "never");
                json!({ "tools": tools })
            }
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("").to_string();
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                self.call(&name, &args, msg)
            }
            _ => return self.wowlua.handle(msg),
        };
        id.map(|id| json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    fn call(&mut self, name: &str, args: &Json, msg: &Json) -> Json {
        match self.permission(name).as_str() {
            "never" => return error(format!("{name} is not allowed in this conversation")),
            "ask" => match (self.ask)(ClaudeEvent::Ask {
                group: group(name).to_string(),
                tool: name.to_string(),
                what: what(name, args),
            }) {
                Ok(ClaudeReply::Allowed(true)) => {}
                Ok(_) => return error(format!("the user did not allow {name}")),
                Err(e) => return error(e),
            },
            _ => {}
        }
        match name {
            "read_file" => self.read_file(args),
            "list_files" => self.list_files(args),
            "search_files" => self.search_files(args),
            "write_file" => self.write_file(args),
            "edit_file" => self.edit_file(args),
            "delete_file" => match self.inside(args["path"].as_str().unwrap_or("")) {
                Ok(p) => self.changing(&p, || match std::fs::remove_file(&p) {
                    Ok(()) => text("deleted"),
                    Err(e) => error(format!("{}: {e}", p.display())),
                }),
                Err(e) => error(e),
            },
            "git_status" => git_answer(crate::git::ok(
                &self.root,
                &["status", "--short", "--branch"],
            )),
            "git_diff" => {
                let mut a = vec!["diff"];
                if args["staged"].as_bool() == Some(true) {
                    a.push("--staged");
                }
                let path = args["path"].as_str().map(str::to_string);
                if let Some(p) = &path {
                    a.push("--");
                    a.push(p);
                }
                git_answer(crate::git::ok(&self.root, &a))
            }
            "git_log" => {
                let n = format!("-{}", args["count"].as_u64().unwrap_or(20).min(200));
                git_answer(crate::git::ok(
                    &self.root,
                    &["log", "--oneline", "--decorate", &n],
                ))
            }
            "git_commit" => {
                let Some(message) = args["message"].as_str().filter(|m| !m.trim().is_empty())
                else {
                    return error("git_commit needs a message");
                };
                if args["all"].as_bool() == Some(true)
                    && let Err(e) = crate::git::ok(&self.root, &["add", "-A"])
                {
                    return error(e);
                }
                match crate::git::commit(&self.root, message) {
                    Ok((hash, subject)) => text(format!("committed {hash} {subject}")),
                    Err(e) => error(e),
                }
            }
            "git_push" | "git_pull" => {
                let creds = crate::git::Credentials {
                    github_token: crate::settings::github_token(),
                };
                let r = if name == "git_push" {
                    crate::git::push(&self.root, &creds)
                } else {
                    crate::git::pull(&self.root, &creds)
                };
                match r {
                    Ok(out) => text(if out.trim().is_empty() {
                        "done".into()
                    } else {
                        out
                    }),
                    Err(e) => error(e.to_string()),
                }
            }
            "git_branch" => {
                let Some(branch) = args["name"].as_str() else {
                    return error("git_branch needs a name");
                };
                let r = match args["action"].as_str().unwrap_or("switch") {
                    "create" => crate::git::new_branch(&self.root, branch, None, true)
                        .map(|_| String::new()),
                    "delete" => crate::git::ok(&self.root, &["branch", "-d", branch]),
                    _ => crate::git::ok(&self.root, &["switch", branch]),
                };
                git_answer(r)
            }
            // wowlua's
            _ => self
                .wowlua
                .handle(msg)
                .map_or_else(|| error("no answer"), |r| r["result"].clone()),
        }
    }

    /// A path of the workspace (relative to it, or absolute in it); never
    /// one outside it, nor in `.git`.
    fn inside(&self, path: &str) -> Result<PathBuf, String> {
        let p = Path::new(path);
        if path.is_empty() {
            return Err("a path is needed".into());
        }
        let rel = if p.is_absolute() {
            p.strip_prefix(&self.root)
                .map_err(|_| format!("{path} is outside the workspace"))?
                .to_path_buf()
        } else {
            p.to_path_buf()
        };
        if rel.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        }) || rel
            .components()
            .next()
            .is_some_and(|c| c.as_os_str() == ".git")
        {
            return Err(format!("{path} is outside the workspace"));
        }
        Ok(self.root.join(rel))
    }

    fn read_file(&self, args: &Json) -> Json {
        let p = match self.inside(args["path"].as_str().unwrap_or("")) {
            Ok(p) => p,
            Err(e) => return error(e),
        };
        let text_of = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(e) => return error(format!("{}: {e}", p.display())),
        };
        let from = args["from"].as_u64().unwrap_or(1).max(1) as usize;
        let count = args["lines"].as_u64().unwrap_or(2000) as usize;
        let lines: Vec<String> = text_of
            .lines()
            .enumerate()
            .skip(from - 1)
            .take(count)
            .map(|(i, l)| format!("{:>6}\t{l}", i + 1))
            .collect();
        text(lines.join("\n"))
    }

    fn list_files(&self, args: &Json) -> Json {
        let dir = match args["dir"].as_str() {
            Some(d) => match self.inside(d) {
                Ok(p) => p,
                Err(e) => return error(e),
            },
            None => self.root.clone(),
        };
        let mut out = Vec::new();
        for entry in ignore::WalkBuilder::new(&dir)
            .hidden(false)
            .build()
            .flatten()
        {
            if entry.path().components().any(|c| c.as_os_str() == ".git") {
                continue;
            }
            if entry.file_type().is_some_and(|t| t.is_file())
                && let Ok(rel) = entry.path().strip_prefix(&self.root)
            {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
            if out.len() >= 2000 {
                out.push("… (more)".into());
                break;
            }
        }
        out.sort();
        text(out.join("\n"))
    }

    fn search_files(&self, args: &Json) -> Json {
        let Some(query) = args["query"].as_str().filter(|q| !q.is_empty()) else {
            return error("search_files needs a query");
        };
        let re = match regex::Regex::new(query) {
            Ok(r) => r,
            Err(e) => return error(format!("not a pattern: {e}")),
        };
        let mut out = Vec::new();
        'files: for entry in ignore::WalkBuilder::new(&self.root).build().flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let Ok(t) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let rel = entry
                .path()
                .strip_prefix(&self.root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            for (i, line) in t.lines().enumerate() {
                if re.is_match(line) {
                    out.push(format!("{rel}:{}: {}", i + 1, line.trim()));
                    if out.len() >= 500 {
                        out.push("… (more)".into());
                        break 'files;
                    }
                }
            }
        }
        text(if out.is_empty() {
            "no match".into()
        } else {
            out.join("\n")
        })
    }

    fn write_file(&self, args: &Json) -> Json {
        let p = match self.inside(args["path"].as_str().unwrap_or("")) {
            Ok(p) => p,
            Err(e) => return error(e),
        };
        let Some(content) = args["content"].as_str() else {
            return error("write_file needs content");
        };
        if let Some(dir) = p.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            return error(format!("{}: {e}", dir.display()));
        }
        self.changing(&p, || match std::fs::write(&p, content) {
            Ok(()) => text(format!("wrote {} lines", content.lines().count())),
            Err(e) => error(format!("{}: {e}", p.display())),
        })
    }

    fn edit_file(&self, args: &Json) -> Json {
        let p = match self.inside(args["path"].as_str().unwrap_or("")) {
            Ok(p) => p,
            Err(e) => return error(e),
        };
        let (Some(old), Some(new)) = (args["old"].as_str(), args["new"].as_str()) else {
            return error("edit_file needs old and new");
        };
        let was = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(e) => return error(format!("{}: {e}", p.display())),
        };
        match was.matches(old).count() {
            0 => error("old is not in the file"),
            1 => self.changing(&p, || match std::fs::write(&p, was.replacen(old, new, 1)) {
                Ok(()) => text("edited"),
                Err(e) => error(format!("{}: {e}", p.display())),
            }),
            n => error(format!("old is in the file {n} times: give more of it")),
        }
    }
}

/// What a tool would do, for the user's question.
fn what(tool: &str, args: &Json) -> String {
    let s = |k: &str| args[k].as_str().unwrap_or("").to_string();
    match tool {
        "write_file" => format!(
            "write {} ({} lines)",
            s("path"),
            args["content"].as_str().map_or(0, |c| c.lines().count())
        ),
        "edit_file" => format!(
            "edit {}: {} lines for {}",
            s("path"),
            args["new"].as_str().map_or(0, |c| c.lines().count()),
            args["old"].as_str().map_or(0, |c| c.lines().count())
        ),
        "delete_file" => format!("delete {}", s("path")),
        "git_commit" => format!("commit \"{}\"", s("message")),
        "git_push" => "push to the remote".into(),
        "git_pull" => "pull from the remote".into(),
        "git_branch" => format!("{} branch {}", s("action"), s("name")),
        "build" => "build the addon".into(),
        _ => format!("{tool} {args}"),
    }
}

/// Studio's own tools, with their input schemas.
fn own_tools() -> Vec<Json> {
    let t = |name: &str, desc: &str, props: Json, required: &[&str]| {
        json!({
            "name": name,
            "description": desc,
            "inputSchema": { "type": "object", "properties": props, "required": required },
        })
    };
    let path =
        json!({ "type": "string", "description": "A path in the workspace, relative to it" });
    vec![
        t(
            "read_file",
            "A file of the workspace, its lines numbered.",
            json!({ "path": path, "from": { "type": "integer" }, "lines": { "type": "integer" } }),
            &["path"],
        ),
        t(
            "list_files",
            "The workspace's files (or a folder's), as git would see them.",
            json!({ "dir": path }),
            &[],
        ),
        t(
            "search_files",
            "Lines of the workspace's files that match a regular expression.",
            json!({ "query": { "type": "string" } }),
            &["query"],
        ),
        t(
            "write_file",
            "Writes a whole file (made if new).",
            json!({ "path": path, "content": { "type": "string" } }),
            &["path", "content"],
        ),
        t(
            "edit_file",
            "Replaces the one place in a file where old is with new.",
            json!({ "path": path, "old": { "type": "string" }, "new": { "type": "string" } }),
            &["path", "old", "new"],
        ),
        t(
            "delete_file",
            "Deletes a file.",
            json!({ "path": path }),
            &["path"],
        ),
        t(
            "git_status",
            "The branch and the changed files.",
            json!({}),
            &[],
        ),
        t(
            "git_diff",
            "The changes not staged (or staged), of one file or all.",
            json!({ "path": path, "staged": { "type": "boolean" } }),
            &[],
        ),
        t(
            "git_log",
            "The latest commits.",
            json!({ "count": { "type": "integer" } }),
            &[],
        ),
        t(
            "git_commit",
            "Commits what is staged (all: every change).",
            json!({ "message": { "type": "string" }, "all": { "type": "boolean" } }),
            &["message"],
        ),
        t("git_push", "Pushes the branch.", json!({}), &[]),
        t("git_pull", "Pulls the branch.", json!({}), &[]),
        t(
            "git_branch",
            "Creates (and switches to), switches to or deletes a branch.",
            json!({ "name": { "type": "string" }, "action": { "type": "string", "enum": ["create", "switch", "delete"] } }),
            &["name"],
        ),
    ]
}

fn text(s: impl Into<String>) -> Json {
    json!({ "content": [{ "type": "text", "text": s.into() }] })
}

fn error(s: impl Into<String>) -> Json {
    json!({ "content": [{ "type": "text", "text": s.into() }], "isError": true })
}

fn git_answer(r: Result<String, String>) -> Json {
    match r {
        Ok(out) if out.trim().is_empty() => text("done"),
        Ok(out) => text(out),
        Err(e) => error(e),
    }
}

// --------------------------------------------------------------- launcher

/// Claude's sessions and the package's commands on the wowlua built into
/// Studio (wowlua T550).
struct StudioLauncher {
    root: PathBuf,
    ask: AskApp,
    changes: crate::claude_changes::Shared,
}

impl Launcher for StudioLauncher {
    fn root(&self) -> PathBuf {
        self.root.clone()
    }

    /// Claude's own session: built as the game page builds one, no window.
    fn run(
        &self,
        opts: &Options,
        session: &mut dyn FnMut(
            &wowlua_test::Suite,
            Option<wowlua_debug::controller::Screenshotter>,
        ),
    ) -> Result<(), String> {
        let mut driver = Session::new();
        let Some(projects) = load_projects(&mut driver, &load(&self.root)) else {
            return Err(
                first_error(&driver).unwrap_or_else(|| "the workspace does not load".into())
            );
        };
        let Some(project) = projects
            .iter()
            .find(|p| matches!(p.kind(), PackageKind::Addon))
            .or_else(|| projects.first())
        else {
            return Err("the workspace has no addon".into());
        };
        let flavor = match &opts.flavor {
            Some(f) if project.manifest.flavors.contains(f) => f.clone(),
            Some(f) => return Err(format!("{} has no flavor {f}", project.addon_name())),
            None => project
                .manifest
                .flavors
                .first()
                .cloned()
                .ok_or_else(|| format!("{} has no flavor", project.addon_name()))?,
        };
        let built = runs::build(
            &mut driver,
            project,
            vec![flavor.clone()],
            &flavor,
            "test",
            wowlua_driver::parallel::Jobs::resolve(None),
            &mut |_| {},
        );
        if let Some(e) = first_error(&driver) {
            return Err(e);
        }
        let (build, _) = built.ok_or_else(|| format!("{} does not build", project.addon_name()))?;
        let blizzard_ui = if project.manifest.manifest.test.blizzard_ui {
            Some(wowlua_test::ui_source(&flavor, &|_: &str| {})?)
        } else {
            None
        };
        let camera = wowlua_test::camera::camera(project, &build, &flavor);
        let instrument: &(dyn Fn(&mut wowlua_vm::Vm) + Sync) = &session::instrument;
        let mut suite = runs::suite(
            project,
            &build,
            &flavor,
            blizzard_ui,
            Switches::default(),
            Some(instrument),
            camera.clone(),
        );
        if let Some(p) = &opts.profile {
            suite.profile = Some(p.clone());
        }
        let screenshot: Option<wowlua_debug::controller::Screenshotter> = camera.map(|camera| {
            let shoot: wowlua_debug::controller::Screenshotter = Box::new(
                move |vm: &mut wowlua_vm::Vm, frame: Option<&str>, scale: Option<f64>| {
                    let id = match frame {
                        Some(f) => Some(
                            wowlua_debug::ui::widget_named(vm, f)
                                .ok_or_else(|| format!("no frame {f}"))?,
                        ),
                        None => None,
                    };
                    let png = camera.shoot(vm, id, scale.unwrap_or(1.0))?;
                    let img = wowlua_render::decode_png(&png)?;
                    Ok((png, img.width, img.height))
                },
            );
            shoot
        });
        session(&suite, screenshot);
        Ok(())
    }

    fn command(&self, command: &PackageCommand) -> Result<CommandOutput, String> {
        match command {
            PackageCommand::Check { flavor } => Ok(build(&self.root, "dev", flavor, false)),
            PackageCommand::Build { release, flavor } => Ok(build(
                &self.root,
                if *release { "release" } else { "dev" },
                flavor,
                true,
            )),
            PackageCommand::Fmt { check, paths } => fmt(&self.root, *check, paths, &self.changes),
            PackageCommand::Test {
                filter, coverage, ..
            } => {
                let request = TestRequest {
                    filters: filter.iter().cloned().collect(),
                    coverage: *coverage,
                    ..TestRequest::default()
                };
                match (self.ask)(ClaudeEvent::Test { request })? {
                    ClaudeReply::Tests { files, done } => {
                        let failed = done.error.is_some() || files.iter().any(TestFile::failed);
                        let coverage = if *coverage {
                            std::fs::read(self.root.join("dist/test/coverage.json"))
                                .ok()
                                .and_then(|b| serde_json::from_slice(&b).ok())
                        } else {
                            None
                        };
                        Ok(CommandOutput {
                            ok: !failed,
                            text: done.error.clone().unwrap_or_default(),
                            results: done.error.is_none().then(|| results_json(&files)),
                            coverage,
                        })
                    }
                    ClaudeReply::Failed(e) => Err(e),
                    ClaudeReply::Allowed(_) => Err("no test run".into()),
                }
            }
        }
    }
}

fn load(root: &Path) -> LoadOptions {
    LoadOptions {
        cwd: root.to_path_buf(),
        lock_in_memory: true,
        offline: true,
        all_members: true,
        ..LoadOptions::default()
    }
}

/// `check` (nothing written, the test side too) or `build`: its problems,
/// one a line.
fn build(root: &Path, profile: &str, flavor: &Option<String>, write: bool) -> CommandOutput {
    let mut session = Session::new();
    let mut out = CommandOutput::default();
    let built = match load_projects(&mut session, &load(root)) {
        Some(projects) => {
            let mut all = true;
            let mut dirs = Vec::new();
            for project in projects
                .iter()
                .filter(|p| matches!(p.kind(), PackageKind::Addon | PackageKind::Library))
            {
                let mut request = wowlua_driver::BuildRequest::new(profile);
                request.flavors = flavor.clone().map(|f| vec![f]);
                request.write = write;
                request.tests = !write;
                match wowlua_driver::build(&mut session, project, &request) {
                    Some(b) => dirs.push(b.dir.clone()),
                    None => all = false,
                }
                project.apply_lints(&mut session);
            }
            all.then_some(dirs)
        }
        None => None,
    };
    let (ok, problems) = crate::ship::diagnostics(&mut session, root);
    out.ok = ok && built.is_some();
    let mut lines: Vec<String> = problems
        .iter()
        .map(|d| {
            let at = match (&d.path, d.line, d.column) {
                (Some(p), Some(l), Some(c)) => {
                    format!("{}:{l}:{c}: ", p.strip_prefix(root).unwrap_or(p).display())
                }
                (Some(p), _, _) => format!("{}: ", p.strip_prefix(root).unwrap_or(p).display()),
                _ => String::new(),
            };
            let code = d
                .code
                .as_ref()
                .map(|c| format!("[{c}]"))
                .unwrap_or_default();
            format!("{at}{}{code}: {}", d.severity, d.message)
        })
        .collect();
    if out.ok {
        if write {
            for dir in built.into_iter().flatten() {
                lines.push(format!("built into {}", dir.display()));
            }
        } else if lines.is_empty() {
            lines.push("no problems".into());
        }
    }
    out.text = lines.join("\n");
    out
}

/// `fmt`: the package's `.wowlua` files (or `paths`) as wowlua fmt writes
/// them; with `check`, only the ones that would change.
fn fmt(
    root: &Path,
    check: bool,
    paths: &[String],
    changes: &crate::claude_changes::Shared,
) -> Result<CommandOutput, String> {
    let files: Vec<PathBuf> = if paths.is_empty() {
        ignore::WalkBuilder::new(root)
            .build()
            .flatten()
            .map(|e| e.into_path())
            .filter(|p| p.extension().is_some_and(|e| e == "wowlua"))
            .filter(|p| !p.components().any(|c| c.as_os_str() == "dist"))
            .collect()
    } else {
        paths.iter().map(|p| root.join(p)).collect()
    };
    let mut changed = Vec::new();
    for path in files {
        let was = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let Some(now) = crate::git::formatted(&path, &was) else {
            continue;
        };
        if now != was {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if !check {
                if let Ok(mut c) = changes.lock() {
                    c.before(&rel);
                }
                std::fs::write(&path, &now).map_err(|e| format!("{}: {e}", path.display()))?;
                if let Ok(mut c) = changes.lock() {
                    c.after(&rel);
                }
            }
            changed.push(rel);
        }
    }
    Ok(CommandOutput {
        ok: !check || changed.is_empty(),
        text: if changed.is_empty() {
            String::new()
        } else if check {
            format!("would change:\n{}", changed.join("\n"))
        } else {
            format!("formatted:\n{}", changed.join("\n"))
        },
        ..CommandOutput::default()
    })
}

/// A run's files as the `json` reporter writes them.
fn results_json(files: &[TestFile]) -> Json {
    let files: Vec<Json> = files
        .iter()
        .map(|f| {
            let tests: Vec<Json> = f
                .tests
                .iter()
                .map(|t| {
                    let (status, message, location) = match &t.outcome {
                        TestOutcome::Pass => ("passed", None, None),
                        TestOutcome::Skip(why) => ("skipped", Some(why.clone()), None),
                        TestOutcome::Fail { message, stack } => {
                            let (m, at) = wowlua_test::split_failure(message, stack);
                            ("failed", Some(m), at)
                        }
                    };
                    json!({ "name": t.name, "status": status, "message": message, "location": location, "seconds": t.seconds })
                })
                .collect();
            json!({ "file": f.file, "flavor": f.flavor, "error": f.error, "seconds": f.seconds, "tests": tests })
        })
        .collect();
    json!({ "files": files })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowlua_studio_app::system::{TestCase, TestDone};

    fn workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("wowlua.json"),
            r#"{ "package": { "name": "counter", "version": "1.0.0" }, "flavors": { "standard": {} }, "addon": { "title": "Counter" } }"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/counter.wowlua"),
            "local x: number = \"one\"\nprint(x)\n",
        )
        .unwrap();
        dir
    }

    /// Every group allowed but those given.
    fn tools(root: &Path, set: &[(&str, &str)], ask: AskApp) -> StudioTools {
        let mut permissions: BTreeMap<String, String> = [
            "files.read",
            "files.edit",
            "wowlua.check",
            "wowlua.test",
            "wowlua.build",
            "game.look",
            "game.play",
            "debugger",
            "git.read",
            "git.commit",
            "git.push",
            "git.branches",
        ]
        .iter()
        .map(|g| (g.to_string(), "allow".to_string()))
        .collect();
        for (g, p) in set {
            permissions.insert(g.to_string(), p.to_string());
        }
        StudioTools::new(
            root.to_path_buf(),
            Arc::new(Mutex::new(permissions)),
            ask,
            Arc::new(Mutex::new(crate::claude_changes::Changes::new(root))),
        )
    }

    fn no_app() -> AskApp {
        Arc::new(|_| Err("no app".into()))
    }

    /// A tool's text and whether it failed.
    fn call(t: &mut StudioTools, name: &str, args: Json) -> (String, bool) {
        let r = t
            .handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": args } }))
            .unwrap();
        let text = r["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        (text, r["result"]["isError"] == true)
    }

    #[test]
    fn never_leaves_a_tool_out_and_ask_waits_for_the_user() {
        let dir = workspace();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let kept = asked.clone();
        let ask: AskApp = Arc::new(move |event| {
            let ClaudeEvent::Ask { what, .. } = event else {
                return Err("not a question".into());
            };
            let allow = !what.contains("secret");
            kept.lock().unwrap().push(what);
            Ok(ClaudeReply::Allowed(allow))
        });
        let mut t = tools(
            dir.path(),
            &[("git.push", "never"), ("files.edit", "ask")],
            ask,
        );
        let listed = t
            .handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
            .unwrap();
        let names: Vec<&str> = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(
            names.contains(&"read_file")
                && names.contains(&"screenshot")
                && names.contains(&"check")
        );
        assert!(
            !names.contains(&"git_push") && !names.contains(&"git_pull"),
            "{names:?}"
        );
        let (said, failed) = call(&mut t, "git_push", json!({}));
        assert!(failed && said.contains("not allowed"), "{said}");
        // asked, allowed: written
        let (_, failed) = call(
            &mut t,
            "write_file",
            json!({ "path": "src/new.wowlua", "content": "print(1)\n" }),
        );
        assert!(!failed);
        assert!(dir.path().join("src/new.wowlua").exists());
        // asked, denied: not written
        let (said, failed) = call(
            &mut t,
            "write_file",
            json!({ "path": "src/secret.wowlua", "content": "x" }),
        );
        assert!(failed && said.contains("did not allow"), "{said}");
        assert!(!dir.path().join("src/secret.wowlua").exists());
        assert_eq!(asked.lock().unwrap()[0], "write src/new.wowlua (1 lines)");
    }

    #[test]
    fn the_files_are_the_workspace_s_only() {
        let dir = workspace();
        let mut t = tools(dir.path(), &[], no_app());
        let (read, _) = call(&mut t, "read_file", json!({ "path": "src/counter.wowlua" }));
        assert!(read.contains("     2\tprint(x)"), "{read}");
        let (listed, _) = call(&mut t, "list_files", json!({}));
        assert!(
            listed.contains("src/counter.wowlua") && listed.contains("wowlua.json"),
            "{listed}"
        );
        let (found, _) = call(&mut t, "search_files", json!({ "query": "print\\(" }));
        assert_eq!(found, "src/counter.wowlua:2: print(x)");
        let (_, failed) = call(
            &mut t,
            "edit_file",
            json!({ "path": "src/counter.wowlua", "old": "\"one\"", "new": "1" }),
        );
        assert!(!failed);
        assert!(
            std::fs::read_to_string(dir.path().join("src/counter.wowlua"))
                .unwrap()
                .starts_with("local x: number = 1")
        );
        for outside in ["../x", "/etc/passwd", ".git/config"] {
            let (said, failed) = call(&mut t, "read_file", json!({ "path": outside }));
            assert!(
                failed && said.contains("outside the workspace"),
                "{outside}: {said}"
            );
        }
    }

    #[test]
    fn check_and_fmt_run_on_studio_s_wowlua() {
        let dir = workspace();
        let mut t = tools(dir.path(), &[], no_app());
        let (checked, failed) = call(&mut t, "check", json!({}));
        assert!(failed, "{checked}");
        assert!(checked.contains("src/counter.wowlua:1:"), "{checked}");
        assert!(checked.contains("[E2001]"), "{checked}");
        std::fs::write(
            dir.path().join("src/counter.wowlua"),
            "local   x: number = 1\nprint( x )\n",
        )
        .unwrap();
        let (checked, failed) = call(&mut t, "check", json!({}));
        assert!(!failed, "{checked}");
        let (told, failed) = call(&mut t, "fmt", json!({ "check": true }));
        assert!(failed && told.contains("src/counter.wowlua"), "{told}");
        call(&mut t, "fmt", json!({}));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/counter.wowlua")).unwrap(),
            "local x: number = 1\nprint(x)\n"
        );
    }

    #[test]
    fn a_test_run_is_the_tests_page_s() {
        let dir = workspace();
        let ask: AskApp = Arc::new(|event| {
            let ClaudeEvent::Test { request } = event else {
                return Err("not a run".into());
            };
            assert_eq!(request.filters, vec!["counts".to_string()]);
            let case = |name: &str, outcome| TestCase {
                name: name.into(),
                outcome,
                seconds: 0.0,
                functions: Vec::new(),
            };
            Ok(ClaudeReply::Tests {
                files: vec![TestFile {
                    package: PathBuf::new(),
                    file: "tests/counter.test.wowlua".into(),
                    flavor: "standard".into(),
                    tests: vec![
                        case("counts up", TestOutcome::Pass),
                        case(
                            "counts wrong",
                            TestOutcome::Fail {
                                message: "expected 2 to be 3".into(),
                                stack: "tests/counter.test.wowlua:10: in function".into(),
                            },
                        ),
                    ],
                    error: None,
                    printed: String::new(),
                    seconds: 0.0,
                }],
                done: TestDone::default(),
            })
        });
        let mut t = tools(dir.path(), &[], ask);
        let (ran, failed) = call(&mut t, "test", json!({ "filter": "counts" }));
        assert!(failed, "{ran}");
        assert!(ran.starts_with("1 passed, 1 failed, 0 skipped"), "{ran}");
        assert!(ran.contains("expected 2 to be 3"), "{ran}");
    }
}
