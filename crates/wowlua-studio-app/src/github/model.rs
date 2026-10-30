//! What Studio knows of the workspace's repository on GitHub (M12): its
//! pull requests with their comments, reviews and files, and its Actions
//! runs with their jobs, steps, logs and annotations. Plain data: the
//! native side reads it from GitHub's REST API with the sign-in's token.

/// The repository on GitHub: `owner/name`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Repo {
    pub owner: String,
    pub name: String,
}

impl Repo {
    pub fn full(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    /// The repository a git remote's URL names, when it is github.com's
    /// (`https://github.com/o/r(.git)`, `git@github.com:o/r.git`,
    /// `ssh://git@github.com/o/r`).
    pub fn of_remote(url: &str) -> Option<Repo> {
        let url = url.trim();
        let rest = url
            .strip_prefix("https://github.com/")
            .or_else(|| url.strip_prefix("http://github.com/"))
            .or_else(|| url.strip_prefix("git@github.com:"))
            .or_else(|| url.strip_prefix("ssh://git@github.com/"))
            .or_else(|| {
                // (`https://user@github.com/o/r`)
                let at = url.strip_prefix("https://")?;
                let (_, host_path) = at.split_once('@')?;
                host_path.strip_prefix("github.com/")
            })?;
        let rest = rest.trim_end_matches('/').trim_end_matches(".git");
        let (owner, name) = rest.split_once('/')?;
        (!owner.is_empty() && !name.is_empty() && !name.contains('/')).then(|| Repo {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }
}

/// A pull request as the list shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullSummary {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub head: String,
    pub base: String,
    pub draft: bool,
    /// Unix seconds.
    pub updated: i64,
}

/// A pull request with what its page shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pull {
    pub summary: PullSummary,
    pub body: String,
    /// `open`, `closed`; merged when `merged`.
    pub state: String,
    pub merged: bool,
    /// Whether GitHub can merge it (none while it works it out).
    pub mergeable: Option<bool>,
    /// `clean`, `blocked`, `behind`, `dirty`, `unstable`, `draft`, …
    pub mergeable_state: String,
    pub head_sha: String,
    pub commits: u64,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
    pub created: i64,
    pub url: String,
}

/// A comment: on the conversation, or on a line of a file (a review's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub id: u64,
    pub author: String,
    pub body: String,
    pub created: i64,
    pub on: Option<OnLine>,
}

/// Where a review comment is: the file, the line on the new side (or the
/// old one), the comment it answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnLine {
    pub path: String,
    pub line: Option<u64>,
    /// `RIGHT` (the new side) or `LEFT`.
    pub side: String,
    pub reply_to: Option<u64>,
    /// The lines around it, as GitHub quotes them.
    pub hunk: String,
}

/// A submitted review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Review {
    pub id: u64,
    pub author: String,
    /// `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, `DISMISSED`.
    pub state: String,
    pub body: String,
    pub submitted: i64,
}

/// A file of a pull request, with its diff's hunks (GitHub's `patch`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullFile {
    pub path: String,
    /// `added`, `modified`, `removed`, `renamed`.
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    /// The unified diff's hunks (none for a binary or too large file).
    pub patch: Option<String>,
}

/// A pull request with everything its page shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullDetail {
    pub pull: Pull,
    /// The conversation's comments and the review comments on lines.
    pub comments: Vec<Comment>,
    pub reviews: Vec<Review>,
    pub files: Vec<PullFile>,
}

/// How a review ends: a comment, an approval, changes asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewEvent {
    Comment,
    Approve,
    RequestChanges,
}

impl ReviewEvent {
    pub fn api(self) -> &'static str {
        match self {
            ReviewEvent::Comment => "COMMENT",
            ReviewEvent::Approve => "APPROVE",
            ReviewEvent::RequestChanges => "REQUEST_CHANGES",
        }
    }
}

/// How a pull request is merged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeMethod {
    Squash,
    Merge,
    Rebase,
}

impl MergeMethod {
    pub fn api(self) -> &'static str {
        match self {
            MergeMethod::Squash => "squash",
            MergeMethod::Merge => "merge",
            MergeMethod::Rebase => "rebase",
        }
    }
}

/// A workflow run of Actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub id: u64,
    /// The workflow's name (`ci`).
    pub workflow: String,
    /// What it ran for: the commit's or the pull request's title.
    pub title: String,
    pub branch: String,
    /// `push`, `pull_request`, `workflow_dispatch`, …
    pub event: String,
    /// `queued`, `in_progress`, `completed`.
    pub status: String,
    /// When completed: `success`, `failure`, `cancelled`, `skipped`, …
    pub conclusion: Option<String>,
    pub head_sha: String,
    pub actor: String,
    pub created: i64,
    pub updated: i64,
    pub url: String,
    /// The pull requests it ran for.
    pub pulls: Vec<u64>,
}

impl Run {
    pub fn running(&self) -> bool {
        self.status != "completed"
    }
}

/// A job of a run, with its steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub started: i64,
    pub completed: Option<i64>,
    pub steps: Vec<Step>,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub number: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub started: Option<i64>,
    pub completed: Option<i64>,
}

/// A job's annotation: an error or warning at a place in a file
/// (`::error file=…,line=…::message`, wowlua's `--message-format github`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Annotation {
    pub path: String,
    pub line: u64,
    pub column: Option<u64>,
    /// `failure`, `warning`, `notice`.
    pub level: String,
    pub title: String,
    pub message: String,
}

/// Why GitHub couldn't answer, as far as Studio can tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GithubError {
    /// Not signed in, or the token no longer works.
    SignedOut,
    /// The repository or the thing asked for isn't visible to the user.
    NotFound,
    /// Refused: the user may not do this (`422`, `403`, `405`), and why.
    Refused(String),
    Other(String),
}

impl std::fmt::Display for GithubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GithubError::SignedOut => f.write_str("not signed in to GitHub"),
            GithubError::NotFound => f.write_str("GitHub has no such thing for this account"),
            GithubError::Refused(why) | GithubError::Other(why) => f.write_str(why),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_names_its_github_repository() {
        let r = |u| Repo::of_remote(u).map(|r| r.full());
        assert_eq!(
            r("https://github.com/DomenikIrrgang/suuui.git").as_deref(),
            Some("DomenikIrrgang/suuui")
        );
        assert_eq!(
            r("git@github.com:DomenikIrrgang/suuui.git").as_deref(),
            Some("DomenikIrrgang/suuui")
        );
        assert_eq!(r("ssh://git@github.com/o/r").as_deref(), Some("o/r"));
        assert_eq!(r("https://me@github.com/o/r/").as_deref(), Some("o/r"));
        assert_eq!(r("https://gitlab.com/o/r.git"), None);
        assert_eq!(r("https://github.com/o"), None);
    }
}
