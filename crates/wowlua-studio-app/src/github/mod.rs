//! GitHub in Studio (M12): the pull requests page and Actions runs of the
//! workspace's repository on GitHub. The state keeps GitHub's last answers
//! and asks again when the workspace opens, with git's fetch (every 5
//! minutes, the same setting) and every 15 seconds while a run is in
//! progress (S072).

pub mod model;
pub mod page;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use model::{Annotation, GithubError, Job, PullDetail, PullSummary, Repo, Run};

use crate::system::{GithubEvent, GithubRequest, System};

/// How often runs in progress are read again (seconds).
const WATCH_EVERY: f64 = 15.0;

/// What the page shows in its main view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selected {
    Pull(u64),
    Run(u64),
    /// The new pull request's form.
    NewPull,
}

/// A run of the user's that finished since it was last read: to notify.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finished {
    pub run: Run,
}

/// GitHub's last answers about the workspace's repository, and what is
/// asked.
#[derive(Default)]
pub struct GithubState {
    root: Option<PathBuf>,
    /// The repository, its default branch and the signed-in login.
    pub repo: Option<Result<(Repo, String, String), GithubError>>,
    pub pulls: Option<Result<Vec<PullSummary>, GithubError>>,
    pub details: HashMap<u64, PullDetail>,
    pub runs: Option<Result<Vec<Run>, GithubError>>,
    pub jobs: HashMap<u64, Vec<Job>>,
    pub logs: HashMap<u64, (String, Vec<Annotation>)>,
    /// The repository's branches on GitHub, and what a head brings over a
    /// base (the new pull request's form).
    pub branches: Vec<String>,
    pub compares: HashMap<(String, String), Result<model::Compare, GithubError>>,
    /// The line comments kept for the review being written, by pull
    /// request.
    pub pending: HashMap<u64, Vec<model::Pending>>,
    pub selected: Option<Selected>,
    /// When pull requests and runs were last asked for (egui's clock).
    pub read_at: Option<f64>,
    asked: HashSet<String>,
    /// Runs of the user's seen in progress: told when they finish.
    watched: HashSet<u64>,
    /// Something running (`merging…`), and what the last change said.
    pub busy: Option<String>,
    pub said: Option<Result<String, GithubError>>,
    /// Runs that finished, for the app to notify.
    pub finished: Vec<Finished>,
    /// What the page's controls hold.
    pub view: page::View,
    /// The status bar's job of each run of the user's in progress.
    pub run_jobs: HashMap<u64, u64>,
    /// What the screenshot harness asked to show once GitHub answered
    /// (`run`, `failed`, `pull`, `new`).
    pub harness: Option<String>,
}

impl GithubState {
    /// A workspace opened (or closed): everything read again.
    pub fn opened(&mut self, root: Option<&Path>) {
        *self = GithubState {
            root: root.map(Path::to_path_buf),
            harness: self.harness.take(),
            ..GithubState::default()
        };
    }

    /// The screenshot harness's step, once what it needs is read.
    fn harness_step(&mut self) {
        let Some(what) = self.harness.clone() else {
            return;
        };
        let runs = match &self.runs {
            Some(Ok(r)) => r.clone(),
            _ => return,
        };
        self.selected = match what.as_str() {
            "run" => runs.first().map(|r| Selected::Run(r.id)),
            "failed" => runs
                .iter()
                .find(|r| r.conclusion.as_deref() == Some("failure"))
                .or(runs.first())
                .map(|r| Selected::Run(r.id)),
            "pull" => match &self.pulls {
                Some(Ok(p)) => p.first().map(|q| Selected::Pull(q.number)),
                _ => return,
            },
            _ => Some(Selected::NewPull),
        };
        self.harness = None;
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// The signed-in login, once known.
    pub fn login(&self) -> Option<&str> {
        match &self.repo {
            Some(Ok((_, _, login))) => Some(login),
            _ => None,
        }
    }

    fn ask(&mut self, system: &dyn System, key: &str, request: GithubRequest) {
        let Some(root) = &self.root else {
            return;
        };
        if self.asked.insert(key.to_owned()) {
            system.github(root, request);
        }
    }

    /// Something that changes GitHub, with the page busy until it answers.
    pub fn change(&mut self, system: &dyn System, busy: &str, request: GithubRequest) {
        if let Some(root) = &self.root {
            self.busy = Some(busy.to_owned());
            self.said = None;
            system.github(root, request);
        }
    }

    /// Pull requests and runs read again now (the refresh button, a sign
    /// in).
    pub fn refresh(&mut self, system: &dyn System, now: f64) {
        self.read_at = Some(now);
        self.asked
            .retain(|k| k.starts_with("pull ") || k.starts_with("log "));
        if matches!(self.repo, Some(Err(_))) {
            self.repo = None;
        }
        self.branches.clear();
        self.compares.clear();
        self.ask(system, "repo", GithubRequest::Repo);
        self.ask(system, "pulls", GithubRequest::Pulls);
        self.ask(system, "runs", GithubRequest::Runs);
    }

    /// Each frame: GitHub read when the workspace opens, then every
    /// `fetch_minutes` (0: only when asked), every 15 seconds while a run
    /// is in progress.
    pub fn tick(&mut self, system: &dyn System, fetch_minutes: u32, now: f64) {
        if self.root.is_none() {
            return;
        }
        let since = self.read_at.map(|at| now - at);
        let running = matches!(&self.runs, Some(Ok(runs)) if runs.iter().any(Run::running));
        let due = match since {
            None => true,
            Some(s) => {
                (fetch_minutes > 0 && s >= f64::from(fetch_minutes) * 60.0)
                    || (running && s >= WATCH_EVERY)
            }
        };
        if due {
            self.refresh(system, now);
        }
    }

    /// A pull request's page, its details asked for when not known.
    pub fn want_pull(&mut self, system: &dyn System, number: u64) {
        if !self.details.contains_key(&number) {
            self.ask(
                system,
                &format!("pull {number}"),
                GithubRequest::Pull(number),
            );
        }
    }

    /// A run's jobs, asked for while it runs and once when done.
    pub fn want_jobs(&mut self, system: &dyn System, run: u64) {
        if !self.jobs.contains_key(&run) {
            self.ask(system, &format!("jobs {run}"), GithubRequest::Jobs(run));
        }
    }

    /// The repository's branches on GitHub, asked for once a read.
    pub fn want_branches(&mut self, system: &dyn System) {
        self.ask(system, "branches", GithubRequest::Branches);
    }

    /// What `head` brings over `base`, asked for once.
    pub fn want_compare(&mut self, system: &dyn System, base: &str, head: &str) {
        if !self
            .compares
            .contains_key(&(base.to_owned(), head.to_owned()))
        {
            self.ask(
                system,
                &format!("compare {base} {head}"),
                GithubRequest::Compare {
                    base: base.to_owned(),
                    head: head.to_owned(),
                },
            );
        }
    }

    /// A job's log and annotations, asked for once.
    pub fn want_log(&mut self, system: &dyn System, job: u64) {
        if !self.logs.contains_key(&job) {
            self.ask(system, &format!("log {job}"), GithubRequest::Log(job));
        }
    }

    /// An answer from GitHub.
    pub fn event(&mut self, root: &Path, event: GithubEvent) {
        if self.root.as_deref() != Some(root) {
            return;
        }
        match event {
            GithubEvent::Repo(result) => {
                self.asked.remove("repo");
                self.repo = Some(result);
            }
            GithubEvent::Pulls(result) => {
                self.asked.remove("pulls");
                // (a pull request read again shows what changed)
                self.details.clear();
                self.asked.retain(|k| !k.starts_with("pull "));
                self.pulls = Some(result);
            }
            GithubEvent::Pull { number, result } => {
                self.asked.remove(&format!("pull {number}"));
                match result {
                    Ok(d) => {
                        self.details.insert(number, *d);
                    }
                    Err(e) => self.said = Some(Err(e)),
                }
            }
            GithubEvent::Runs(result) => {
                self.asked.remove("runs");
                if let Ok(runs) = &result {
                    let me = self.login().map(str::to_owned);
                    for r in runs {
                        let mine = me.as_deref() == Some(r.actor.as_str());
                        if r.running() {
                            // (its jobs are read again while it runs)
                            self.jobs.remove(&r.id);
                            self.asked.remove(&format!("jobs {}", r.id));
                            if mine {
                                self.watched.insert(r.id);
                            }
                        } else if self.watched.remove(&r.id) {
                            self.jobs.remove(&r.id);
                            self.asked.remove(&format!("jobs {}", r.id));
                            self.finished.push(Finished { run: r.clone() });
                        }
                    }
                }
                self.runs = Some(result);
                self.harness_step();
            }
            GithubEvent::Branches(result) => {
                self.asked.remove("branches");
                if let Ok(b) = result {
                    self.branches = b;
                }
            }
            GithubEvent::Compare { base, head, result } => {
                self.asked.remove(&format!("compare {base} {head}"));
                self.compares.insert((base, head), result);
            }
            GithubEvent::Jobs { run, result } => {
                self.asked.remove(&format!("jobs {run}"));
                if let Ok(jobs) = result {
                    self.jobs.insert(run, jobs);
                }
            }
            GithubEvent::Log { job, result } => {
                self.asked.remove(&format!("log {job}"));
                match result {
                    Ok(l) => {
                        self.logs.insert(job, l);
                    }
                    Err(e) => self.said = Some(Err(e)),
                }
            }
            GithubEvent::Done { what, result } => {
                self.busy = None;
                // (what it changed is read again)
                self.details.clear();
                self.asked.retain(|k| !k.starts_with("pull "));
                self.read_at = None;
                if what == "create"
                    && let Ok(n) = &result
                    && let Ok(number) = n.parse()
                {
                    self.selected = Some(Selected::Pull(number));
                }
                self.said = Some(result.map(|s| format!("{what}: {s}")));
            }
        }
    }

    /// The open pull request of a branch, if any.
    /// The base a new pull request of `branch` would have, when GitHub is
    /// read and the branch has none yet (not the default branch itself).
    pub fn pull_wanted(&self, branch: &str) -> Option<&str> {
        let Some(Ok((_, default, _))) = &self.repo else {
            return None;
        };
        let read = matches!(self.pulls, Some(Ok(_)));
        (read && branch != default && self.pull_of(branch).is_none()).then_some(default.as_str())
    }

    pub fn pull_of(&self, branch: &str) -> Option<&PullSummary> {
        match &self.pulls {
            Some(Ok(p)) => p.iter().find(|p| p.head == branch),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: u64, status: &str, actor: &str) -> Run {
        Run {
            id,
            workflow: "ci".into(),
            title: "t".into(),
            branch: "b".into(),
            event: "push".into(),
            status: status.into(),
            conclusion: (status == "completed").then(|| "success".into()),
            head_sha: "s".into(),
            actor: actor.into(),
            created: 0,
            updated: 0,
            url: String::new(),
            pulls: Vec::new(),
        }
    }

    #[test]
    fn a_run_of_mine_seen_running_is_told_when_it_finishes() {
        let root = PathBuf::from("/w");
        let mut s = GithubState::default();
        s.opened(Some(&root));
        s.event(
            &root,
            GithubEvent::Repo(Ok((
                Repo {
                    owner: "o".into(),
                    name: "r".into(),
                },
                "main".into(),
                "me".into(),
            ))),
        );
        s.event(
            &root,
            GithubEvent::Runs(Ok(vec![
                run(1, "in_progress", "me"),
                run(2, "in_progress", "other"),
            ])),
        );
        assert!(s.finished.is_empty());
        s.event(
            &root,
            GithubEvent::Runs(Ok(vec![
                run(1, "completed", "me"),
                run(2, "completed", "other"),
            ])),
        );
        assert_eq!(s.finished.len(), 1);
        assert_eq!(s.finished[0].run.id, 1);
    }

    #[test]
    fn a_pushed_branch_wants_a_pull_request_until_it_has_one() {
        let mut s = GithubState::default();
        assert_eq!(s.pull_wanted("feat"), None, "GitHub not read yet");
        s.repo = Some(Ok((
            Repo {
                owner: "o".into(),
                name: "r".into(),
            },
            "main".into(),
            "me".into(),
        )));
        assert_eq!(
            s.pull_wanted("feat"),
            None,
            "the pull requests not read yet"
        );
        s.pulls = Some(Ok(Vec::new()));
        assert_eq!(s.pull_wanted("feat"), Some("main"));
        assert_eq!(s.pull_wanted("main"), None, "the default branch");
        s.pulls = Some(Ok(vec![PullSummary {
            number: 7,
            title: "t".into(),
            author: "me".into(),
            head: "feat".into(),
            base: "main".into(),
            draft: false,
            updated: 0,
        }]));
        assert_eq!(s.pull_wanted("feat"), None, "#7 is open");
    }
}
