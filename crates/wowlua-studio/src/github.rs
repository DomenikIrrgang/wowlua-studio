//! GitHub for Studio (M12): the workspace's repository on github.com,
//! through GitHub's REST API with the sign-in's token (S062): pull requests
//! with their comments, reviews and files, Actions runs with their jobs,
//! steps, logs and annotations, and what the user does with them (comment,
//! reply, review, merge, create, re-run, cancel). Each answer maps GitHub's
//! JSON into the model the app draws.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value as Json, json};
use wowlua_driver::publish::http::Http;
use wowlua_studio_app::github::model::{
    Annotation, Comment, GithubError, Job, OnLine, Pull, PullDetail, PullFile, PullSummary, Repo,
    Review, Run, Step,
};
use wowlua_studio_app::system::{GithubEvent, GithubRequest};

const API: &str = "https://api.github.com";

/// The lines of a job's log kept (its end, where a failure is).
const LOG_LINES: usize = 2000;

/// The repository the workspace's remote names on github.com: the branch's
/// upstream remote, else `origin`.
pub fn repo_of(root: &Path) -> Option<Repo> {
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    let remote = git(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
        .and_then(|u| u.split_once('/').map(|(r, _)| r.to_owned()))
        .unwrap_or_else(|| "origin".to_owned());
    Repo::of_remote(&git(&["remote", "get-url", &remote])?)
}

/// GitHub's REST API as the signed-in user.
pub struct Client {
    http: Http,
    token: String,
}

impl Client {
    pub fn new(token: String) -> Client {
        Client {
            http: Http::default(),
            token,
        }
    }

    fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("accept", "application/vnd.github+json".to_owned()),
            ("authorization", format!("Bearer {}", self.token)),
            ("user-agent", "wowlua-studio".to_owned()),
            ("x-github-api-version", "2022-11-28".to_owned()),
        ]
    }

    /// A request; its JSON (none for an empty body) or what went wrong.
    fn send(&self, method: &str, path: &str, body: Option<&Json>) -> Result<Json, GithubError> {
        let url = if path.starts_with("https://") {
            path.to_owned()
        } else {
            format!("{API}{path}")
        };
        let mut headers = self.headers();
        let bytes = body.map(|b| {
            headers.push(("content-type", "application/json".to_owned()));
            b.to_string().into_bytes()
        });
        let response = self
            .http
            .request(method, &url, &headers, bytes.as_deref())
            .map_err(|e| GithubError::Other(format!("cannot reach GitHub: {e}")))?;
        if response.ok() {
            return Ok(response.json().unwrap_or(Json::Null));
        }
        Err(error_of(response.status, &response.body))
    }

    fn get(&self, path: &str) -> Result<Json, GithubError> {
        self.send("GET", path, None)
    }

    /// A job's log as text.
    fn log(&self, path: &str) -> Result<String, GithubError> {
        let response = self
            .http
            .request("GET", &format!("{API}{path}"), &self.headers(), None)
            .map_err(|e| GithubError::Other(format!("cannot reach GitHub: {e}")))?;
        if response.ok() {
            Ok(response.body)
        } else {
            Err(error_of(response.status, &response.body))
        }
    }
}

/// What a failed answer means: signed out, not there, refused (with
/// GitHub's reason).
fn error_of(status: u16, body: &str) -> GithubError {
    let json: Json = serde_json::from_str(body).unwrap_or(Json::Null);
    let mut why = json["message"].as_str().unwrap_or("").to_owned();
    if let Some(errors) = json["errors"].as_array() {
        let more: Vec<String> = errors
            .iter()
            .filter_map(|e| e["message"].as_str().or(e.as_str()).map(str::to_owned))
            .collect();
        if !more.is_empty() {
            why = format!("{why}: {}", more.join("; "));
        }
    }
    match status {
        401 => GithubError::SignedOut,
        404 => GithubError::NotFound,
        403 | 405 | 409 | 422 => GithubError::Refused(if why.is_empty() {
            format!("GitHub refused it (HTTP {status})")
        } else {
            why
        }),
        _ => GithubError::Other(format!("GitHub answered HTTP {status}: {why}")),
    }
}

// --------------------------------------------------------------- times --

/// Unix seconds of GitHub's `2026-10-30T10:15:00Z` (none for null).
pub(crate) fn time(v: &Json) -> Option<i64> {
    let s = v.as_str()?;
    let (date, clock) = s.split_once('T')?;
    let mut d = date.split('-').map(|n| n.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let clock = clock.trim_end_matches('Z');
    let clock = clock.split(['.', '+']).next()?;
    let mut c = clock.split(':').map(|n| n.parse::<i64>().ok());
    let (h, min, sec) = (c.next()??, c.next()??, c.next().flatten().unwrap_or(0));
    // (days from the civil date: Howard Hinnant's algorithm)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + min * 60 + sec)
}

fn text(v: &Json) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn login(v: &Json) -> String {
    text(&v["login"])
}

// ------------------------------------------------------------- mapping --

pub(crate) fn pull_summary(j: &Json) -> PullSummary {
    PullSummary {
        number: j["number"].as_u64().unwrap_or(0),
        title: text(&j["title"]),
        author: login(&j["user"]),
        head: text(&j["head"]["ref"]),
        base: text(&j["base"]["ref"]),
        draft: j["draft"].as_bool().unwrap_or(false),
        updated: time(&j["updated_at"]).unwrap_or(0),
    }
}

pub(crate) fn pull(j: &Json) -> Pull {
    Pull {
        summary: pull_summary(j),
        body: text(&j["body"]),
        state: text(&j["state"]),
        merged: j["merged"].as_bool().unwrap_or(false),
        mergeable: j["mergeable"].as_bool(),
        mergeable_state: text(&j["mergeable_state"]),
        head_sha: text(&j["head"]["sha"]),
        commits: j["commits"].as_u64().unwrap_or(0),
        additions: j["additions"].as_u64().unwrap_or(0),
        deletions: j["deletions"].as_u64().unwrap_or(0),
        changed_files: j["changed_files"].as_u64().unwrap_or(0),
        created: time(&j["created_at"]).unwrap_or(0),
        url: text(&j["html_url"]),
    }
}

/// A comment of the conversation, or one on a line (`review`).
pub(crate) fn comment(j: &Json, review: bool) -> Comment {
    Comment {
        id: j["id"].as_u64().unwrap_or(0),
        author: login(&j["user"]),
        body: text(&j["body"]),
        created: time(&j["created_at"]).unwrap_or(0),
        on: review.then(|| OnLine {
            path: text(&j["path"]),
            line: j["line"].as_u64().or(j["original_line"].as_u64()),
            side: j["side"].as_str().unwrap_or("RIGHT").to_owned(),
            reply_to: j["in_reply_to_id"].as_u64(),
            hunk: text(&j["diff_hunk"]),
        }),
    }
}

pub(crate) fn review(j: &Json) -> Review {
    Review {
        id: j["id"].as_u64().unwrap_or(0),
        author: login(&j["user"]),
        state: text(&j["state"]),
        body: text(&j["body"]),
        submitted: time(&j["submitted_at"]).unwrap_or(0),
    }
}

pub(crate) fn file(j: &Json) -> PullFile {
    PullFile {
        path: text(&j["filename"]),
        status: text(&j["status"]),
        additions: j["additions"].as_u64().unwrap_or(0),
        deletions: j["deletions"].as_u64().unwrap_or(0),
        patch: j["patch"].as_str().map(str::to_owned),
    }
}

pub(crate) fn run(j: &Json) -> Run {
    Run {
        id: j["id"].as_u64().unwrap_or(0),
        workflow: text(&j["name"]),
        title: text(&j["display_title"]),
        branch: text(&j["head_branch"]),
        event: text(&j["event"]),
        status: text(&j["status"]),
        conclusion: j["conclusion"].as_str().map(str::to_owned),
        head_sha: text(&j["head_sha"]),
        actor: login(&j["actor"]),
        created: time(&j["created_at"]).unwrap_or(0),
        updated: time(&j["updated_at"]).unwrap_or(0),
        url: text(&j["html_url"]),
        pulls: j["pull_requests"]
            .as_array()
            .map(|a| a.iter().filter_map(|p| p["number"].as_u64()).collect())
            .unwrap_or_default(),
    }
}

pub(crate) fn job(j: &Json) -> Job {
    Job {
        id: j["id"].as_u64().unwrap_or(0),
        name: text(&j["name"]),
        status: text(&j["status"]),
        conclusion: j["conclusion"].as_str().map(str::to_owned),
        started: time(&j["started_at"]).unwrap_or(0),
        completed: time(&j["completed_at"]),
        url: text(&j["html_url"]),
        steps: j["steps"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| Step {
                        number: s["number"].as_u64().unwrap_or(0),
                        name: text(&s["name"]),
                        status: text(&s["status"]),
                        conclusion: s["conclusion"].as_str().map(str::to_owned),
                        started: time(&s["started_at"]),
                        completed: time(&s["completed_at"]),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

pub(crate) fn annotation(j: &Json) -> Annotation {
    Annotation {
        path: text(&j["path"]),
        line: j["start_line"].as_u64().unwrap_or(0),
        column: j["start_column"].as_u64(),
        level: text(&j["annotation_level"]),
        title: text(&j["title"]),
        message: text(&j["message"]),
    }
}

/// A job's log without GitHub's timestamps, its last lines.
pub(crate) fn log_text(raw: &str) -> String {
    let lines: Vec<&str> = raw
        .lines()
        .map(|l| {
            // (`2026-10-30T10:15:00.1234567Z ` before each line)
            match l.split_once(' ') {
                Some((stamp, rest))
                    if stamp.len() >= 20 && stamp.ends_with('Z') && stamp.contains('T') =>
                {
                    rest
                }
                _ => l,
            }
        })
        .collect();
    let from = lines.len().saturating_sub(LOG_LINES);
    lines[from..].join("\n")
}

fn array(j: Json) -> Vec<Json> {
    match j {
        Json::Array(a) => a,
        _ => Vec::new(),
    }
}

// ------------------------------------------------------------ requests --

/// The answer to a question GitHub wasn't asked: why not.
fn failed(request: GithubRequest, why: GithubError) -> GithubEvent {
    match request {
        GithubRequest::Repo => GithubEvent::Repo(Err(why)),
        GithubRequest::Pulls => GithubEvent::Pulls(Err(why)),
        GithubRequest::Pull(number) => GithubEvent::Pull {
            number,
            result: Err(why),
        },
        GithubRequest::Runs => GithubEvent::Runs(Err(why)),
        GithubRequest::Jobs(run) => GithubEvent::Jobs {
            run,
            result: Err(why),
        },
        GithubRequest::Log(job) => GithubEvent::Log {
            job,
            result: Err(why),
        },
        _ => GithubEvent::Done {
            what: "github".into(),
            result: Err(why),
        },
    }
}

/// Studio's question answered (on a worker thread); `token` none: signed
/// out.
pub fn answer(root: &Path, request: GithubRequest, token: Option<String>) -> GithubEvent {
    let Some(repo) = repo_of(root) else {
        return failed(
            request,
            GithubError::Other("the workspace has no remote on github.com".into()),
        );
    };
    let Some(token) = token else {
        return failed(request, GithubError::SignedOut);
    };
    let c = Client::new(token);
    let r = format!("/repos/{}/{}", repo.owner, repo.name);
    let done = |what: &str, result: Result<String, GithubError>| GithubEvent::Done {
        what: what.to_owned(),
        result,
    };
    match request {
        GithubRequest::Repo => GithubEvent::Repo((|| {
            let info = c.get(&r)?;
            let me = c.get("/user")?;
            Ok((repo.clone(), text(&info["default_branch"]), login(&me)))
        })()),
        GithubRequest::Pulls => GithubEvent::Pulls(
            c.get(&format!("{r}/pulls?state=open&per_page=50"))
                .map(|j| array(j).iter().map(pull_summary).collect()),
        ),
        GithubRequest::Pull(number) => GithubEvent::Pull {
            number,
            result: (|| {
                let p = pull(&c.get(&format!("{r}/pulls/{number}"))?);
                let mut comments: Vec<Comment> =
                    array(c.get(&format!("{r}/issues/{number}/comments?per_page=100"))?)
                        .iter()
                        .map(|j| comment(j, false))
                        .collect();
                comments.extend(
                    array(c.get(&format!("{r}/pulls/{number}/comments?per_page=100"))?)
                        .iter()
                        .map(|j| comment(j, true)),
                );
                comments.sort_by_key(|c| c.created);
                let reviews = array(c.get(&format!("{r}/pulls/{number}/reviews?per_page=100"))?)
                    .iter()
                    .map(review)
                    .collect();
                let files = array(c.get(&format!("{r}/pulls/{number}/files?per_page=100"))?)
                    .iter()
                    .map(file)
                    .collect();
                Ok(Box::new(PullDetail {
                    pull: p,
                    comments,
                    reviews,
                    files,
                }))
            })(),
        },
        GithubRequest::Runs => GithubEvent::Runs(
            c.get(&format!("{r}/actions/runs?per_page=30"))
                .map(|j| array(j["workflow_runs"].clone()).iter().map(run).collect()),
        ),
        GithubRequest::Jobs(id) => GithubEvent::Jobs {
            run: id,
            result: c
                .get(&format!("{r}/actions/runs/{id}/jobs?per_page=50"))
                .map(|j| array(j["jobs"].clone()).iter().map(job).collect()),
        },
        GithubRequest::Log(id) => GithubEvent::Log {
            job: id,
            result: (|| {
                let log = c.log(&format!("{r}/actions/jobs/{id}/logs"))?;
                let notes = c
                    .get(&format!("{r}/check-runs/{id}/annotations?per_page=50"))
                    .map(|j| array(j).iter().map(annotation).collect())
                    .unwrap_or_default();
                Ok((log_text(&log), notes))
            })(),
        },
        GithubRequest::Comment { number, body } => done(
            "comment",
            c.send(
                "POST",
                &format!("{r}/issues/{number}/comments"),
                Some(&json!({ "body": body })),
            )
            .map(|j| text(&j["html_url"])),
        ),
        GithubRequest::Reply { number, to, body } => done(
            "reply",
            c.send(
                "POST",
                &format!("{r}/pulls/{number}/comments/{to}/replies"),
                Some(&json!({ "body": body })),
            )
            .map(|j| text(&j["html_url"])),
        ),
        GithubRequest::LineComment {
            number,
            commit,
            path,
            line,
            side,
            body,
        } => done(
            "line comment",
            c.send(
                "POST",
                &format!("{r}/pulls/{number}/comments"),
                Some(&json!({ "body": body, "commit_id": commit, "path": path, "line": line, "side": side })),
            )
            .map(|j| text(&j["html_url"])),
        ),
        GithubRequest::Review { number, event, body } => done(
            "review",
            c.send(
                "POST",
                &format!("{r}/pulls/{number}/reviews"),
                Some(&json!({ "body": body, "event": event.api() })),
            )
            .map(|j| text(&j["html_url"])),
        ),
        GithubRequest::Merge { number, method } => done(
            "merge",
            c.send(
                "PUT",
                &format!("{r}/pulls/{number}/merge"),
                Some(&json!({ "merge_method": method.api() })),
            )
            .map(|j| text(&j["sha"])),
        ),
        GithubRequest::Create {
            title,
            body,
            head,
            base,
            draft,
        } => done(
            "create",
            c.send(
                "POST",
                &format!("{r}/pulls"),
                Some(&json!({ "title": title, "body": body, "head": head, "base": base, "draft": draft })),
            )
            .map(|j| j["number"].as_u64().map(|n| n.to_string()).unwrap_or_default()),
        ),
        GithubRequest::Rerun { run, failed_only } => done(
            "re-run",
            c.send(
                "POST",
                &format!(
                    "{r}/actions/runs/{run}/{}",
                    if failed_only { "rerun-failed-jobs" } else { "rerun" }
                ),
                None,
            )
            .map(|_| String::new()),
        ),
        GithubRequest::Cancel(run) => done(
            "cancel",
            c.send("POST", &format!("{r}/actions/runs/{run}/cancel"), None)
                .map(|_| String::new()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_times_are_unix_seconds() {
        assert_eq!(time(&json!("1970-01-01T00:00:00Z")), Some(0));
        assert_eq!(time(&json!("2026-10-30T10:15:30Z")), Some(1_793_355_330));
        assert_eq!(time(&json!("2000-02-29T00:00:00.123Z")), Some(951_782_400));
        assert_eq!(time(&Json::Null), None);
    }

    #[test]
    fn pull_requests_map_from_github_s_json() {
        let j = json!({
            "number": 57, "title": "Threat meter: raid pulls", "user": { "login": "DomenikIrrgang" },
            "head": { "ref": "feat/threat-raid", "sha": "3f2a91c" }, "base": { "ref": "main" },
            "draft": false, "updated_at": "2026-10-30T10:15:30Z", "created_at": "2026-10-30T08:00:00Z",
            "body": "Raid pulls list every tank.", "state": "open", "merged": false,
            "mergeable": true, "mergeable_state": "clean", "commits": 3, "additions": 118,
            "deletions": 21, "changed_files": 4, "html_url": "https://github.com/o/r/pull/57"
        });
        let p = pull(&j);
        assert_eq!(p.summary.number, 57);
        assert_eq!(p.summary.head, "feat/threat-raid");
        assert_eq!(p.summary.author, "DomenikIrrgang");
        assert_eq!((p.additions, p.deletions, p.changed_files), (118, 21, 4));
        assert_eq!(p.mergeable, Some(true));
        let c = comment(
            &json!({ "id": 9, "user": { "login": "rv" }, "body": "typo", "created_at": "2026-10-30T09:00:00Z",
                "path": "src/UI/ThreatMeter.wowlua", "line": 65, "side": "RIGHT", "in_reply_to_id": 8,
                "diff_hunk": "@@ -60,6 +60,8 @@" }),
            true,
        );
        let on = c.on.expect("on a line");
        assert_eq!(
            (on.path.as_str(), on.line, on.reply_to),
            ("src/UI/ThreatMeter.wowlua", Some(65), Some(8))
        );
    }

    #[test]
    fn runs_jobs_and_annotations_map_from_github_s_json() {
        let r = run(&json!({
            "id": 77, "name": "ci", "display_title": "Threat meter", "head_branch": "feat/x",
            "event": "pull_request", "status": "completed", "conclusion": "failure",
            "head_sha": "abc", "actor": { "login": "me" }, "created_at": "2026-10-30T10:00:00Z",
            "updated_at": "2026-10-30T10:02:00Z", "html_url": "u", "pull_requests": [{ "number": 57 }]
        }));
        assert_eq!(
            (r.id, r.conclusion.as_deref(), r.pulls.as_slice()),
            (77, Some("failure"), &[57][..])
        );
        assert!(!r.running());
        let j = job(&json!({
            "id": 5, "name": "build", "status": "completed", "conclusion": "failure",
            "started_at": "2026-10-30T10:00:00Z", "completed_at": "2026-10-30T10:01:00Z", "html_url": "u",
            "steps": [{ "number": 1, "name": "Run wowlua build", "status": "completed", "conclusion": "failure",
                "started_at": "2026-10-30T10:00:10Z", "completed_at": "2026-10-30T10:00:50Z" }]
        }));
        assert_eq!(j.steps[0].name, "Run wowlua build");
        assert_eq!(j.completed.map(|c| c - j.started), Some(60));
        let a = annotation(
            &json!({ "path": "src/UI/ThreatMeter.wowlua", "start_line": 68, "start_column": 20,
            "annotation_level": "failure", "title": "E2003", "message": "no field `roel`" }),
        );
        assert_eq!(
            (a.line, a.column, a.level.as_str()),
            (68, Some(20), "failure")
        );
        assert_eq!(
            log_text(
                "2026-10-30T10:00:10.1234567Z Run wowlua build\n2026-10-30T10:00:11.0000000Z ::error file=a,line=1::x"
            ),
            "Run wowlua build\n::error file=a,line=1::x"
        );
    }

    #[test]
    fn failures_say_what_github_means() {
        assert_eq!(error_of(401, "{}"), GithubError::SignedOut);
        assert_eq!(error_of(404, "{}"), GithubError::NotFound);
        assert_eq!(
            error_of(405, r#"{"message":"Pull Request is not mergeable"}"#),
            GithubError::Refused("Pull Request is not mergeable".into())
        );
        assert_eq!(
            error_of(
                422,
                r#"{"message":"Validation Failed","errors":[{"message":"A pull request already exists"}]}"#
            ),
            GithubError::Refused("Validation Failed: A pull request already exists".into())
        );
    }
}
