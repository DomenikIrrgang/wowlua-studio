//! A button's verb icon (Option D, the design system's VerbIcons): every
//! action leads with an icon of what it does, chosen from its label's
//! first word; removing ones are drawn as danger.

use crate::icons::Icon;

/// The icon of the action a label names, by its first word (`copy the
/// code` → copy); a link to the web is `external`.
pub fn icon_for(label: &str) -> Option<Icon> {
    let label = label.trim().to_lowercase();
    let first = label
        .split(|c: char| c.is_whitespace() || c == ',' || c == '\u{2026}')
        .map(|w| w.trim_start_matches('+'))
        .find(|w| !w.is_empty())
        .unwrap_or("");
    let web = [
        "github.com",
        "github",
        "browser",
        "registry.wowlua.dev",
        "the web",
    ]
    .iter()
    .any(|w| label.contains(w));
    Some(match first {
        "open" | "go" | "show" if web => Icon::External,
        "add" | "new" | "create" => Icon::Add,
        "unstage" => Icon::Subtract,
        "edit" | "change" | "set" | "rename" | "require" => Icon::Edit,
        "remove" | "delete" | "discard" | "clear" | "yank" | "forget" => Icon::Remove,
        "close" | "cancel" | "deny" | "dismiss" => Icon::Close,
        "reset" | "undo" | "restore" | "revert" => Icon::Reset,
        "save" | "saved" | "apply" | "accept" | "resolve" | "take" | "keep" | "done" | "ok" => {
            Icon::Confirm
        }
        "allow" | "approve" | "trust" => Icon::Approve,
        "copy" | "copied" => Icon::Copy,
        "open" | "go" | "jump" | "reveal" => Icon::Open,
        "choose" | "browse" | "pick" => Icon::FolderOpen,
        "search" | "find" | "locate" => Icon::Search,
        "replace" => Icon::Replace,
        "show" | "watch" | "expand" | "view" => Icon::Watch,
        "run" | "start" | "continue" | "dry" | "play" | "test" => Icon::Run,
        "check" | "retry" | "re-run" | "rerun" | "refresh" | "reload" | "again" => Icon::Refresh,
        "stop" | "abort" | "end" => Icon::Stop,
        "pause" | "break" => Icon::Pause,
        "debug" => Icon::Debug,
        "profile" => Icon::Perf,
        "record" => Icon::Record,
        "build" => Icon::Build,
        "install" | "download" | "import" | "pull" | "adopt" | "fetch" => Icon::Install,
        "publish" | "push" | "upload" | "export" | "tag" => Icon::Publish,
        "update" | "upgrade" | "train" => Icon::Update,
        "commit" => Icon::Commit,
        "branch" | "fork" => Icon::Git,
        "merge" => Icon::PullRequest,
        "compare" | "diff" => Icon::Compare,
        "explain" | "why" => Icon::Explain,
        "sign" | "log" if label.contains(" out") => Icon::SignOut,
        "sign" | "login" | "log" => Icon::SignIn,
        "send" => Icon::Send,
        "reply" => Icon::Reply,
        "inspect" | "target" => Icon::Inspect,
        "screenshot" | "snapshot" => Icon::Snapshot,
        "fire" => Icon::Event,
        "advance" | "wait" => Icon::Clock,
        "float" => Icon::Float,
        "move" | "drag" => Icon::Move,
        "previous" | "prev" | "back" => Icon::Previous,
        "next" => Icon::Next,
        "configure" | "settings" => Icon::Config,
        "stage" => Icon::Add,
        _ => return None,
    })
}

/// Whether the action removes something: drawn as danger (`remove`,
/// `delete`, `discard`, `yank`; `clear` is not).
pub fn is_danger(label: &str) -> bool {
    let first = label.trim().to_lowercase();
    let first = first.split_whitespace().next().unwrap_or("");
    matches!(first, "remove" | "delete" | "discard" | "yank")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_lead_with_their_verb_s_icon() {
        assert_eq!(icon_for("copy the code"), Some(Icon::Copy));
        assert_eq!(icon_for("open github.com"), Some(Icon::External));
        assert_eq!(icon_for("open"), Some(Icon::Open));
        assert_eq!(icon_for("change requirement"), Some(Icon::Edit));
        assert_eq!(icon_for("+ new profile"), Some(Icon::Add));
        assert_eq!(icon_for("check again"), Some(Icon::Refresh));
        assert_eq!(icon_for("sign out"), Some(Icon::SignOut));
        assert_eq!(icon_for("sign in with GitHub"), Some(Icon::SignIn));
        assert_eq!(icon_for("choose…"), Some(Icon::FolderOpen));
        assert_eq!(icon_for("wowlua.json"), None);
        assert!(is_danger("remove") && is_danger("Delete the branch"));
        assert!(!is_danger("clear") && !is_danger("save"));
    }
}
