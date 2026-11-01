//! Claude Code (S090, S095): where it is, its version, whom it is signed in
//! as, and signing out. Studio never signs in to Claude itself: the sign-in
//! runs in the terminal, as Claude Code's own `claude auth login`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use wowlua_studio_app::system::{ClaudeAccount, ClaudeCode};

/// The program: `WOWLUA_STUDIO_CLAUDE`, else on the PATH, else where Claude
/// Code's installers put it (an app started from the Dock or the Start
/// menu does not have the shell's PATH).
pub fn find() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("WOWLUA_STUDIO_CLAUDE") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    if let Some(home) = home {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".claude").join("local"));
    }
    if cfg!(windows) {
        if let Some(a) = std::env::var_os("APPDATA") {
            dirs.push(PathBuf::from(a).join("npm"));
        }
    } else {
        dirs.push("/opt/homebrew/bin".into());
        dirs.push("/usr/local/bin".into());
    }
    dirs.iter()
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| p.is_file())
}

/// Claude Code as it is now: found, its version, signed in or not.
pub fn status() -> ClaudeCode {
    let Some(path) = find() else {
        return ClaudeCode::default();
    };
    let version = run(&path, &["--version"])
        .ok()
        .and_then(|out| version_of(&out));
    let (account, problem) = match run(&path, &["auth", "status"]) {
        Ok(out) => match account_of(&out) {
            Ok(a) => (a, None),
            Err(e) => (None, Some(e)),
        },
        Err(e) => (None, Some(e)),
    };
    ClaudeCode {
        sign_in: sign_in_line(&path),
        path: Some(path),
        version,
        account,
        problem,
    }
}

/// Signs Claude Code out (`claude auth logout`).
pub fn sign_out() {
    if let Some(path) = find()
        && let Err(e) = run(&path, &["auth", "logout"])
    {
        crate::log::line(&format!("claude auth logout: {e}"));
    }
}

/// What the program printed (its exit code aside: `auth status` exits 1
/// when signed out and still tells it).
fn run(path: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(path)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `2.1.314` from `2.1.314 (Claude Code)`.
pub fn version_of(out: &str) -> Option<String> {
    out.split_whitespace()
        .next()
        .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_owned)
}

/// The account `claude auth status` tells (JSON): none when signed out.
pub fn account_of(out: &str) -> Result<Option<ClaudeAccount>, String> {
    let j: serde_json::Value =
        serde_json::from_str(out.trim()).map_err(|e| format!("claude auth status: {e}"))?;
    if j["loggedIn"].as_bool() != Some(true) {
        return Ok(None);
    }
    let text = |k: &str| j[k].as_str().unwrap_or("").to_owned();
    let plan = Some(text("subscriptionType"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| text("authMethod"));
    Ok(Some(ClaudeAccount {
        email: text("email"),
        plan,
    }))
}

/// The line the terminal runs to sign in, with the program's path as the
/// shell needs it (the terminal's shell: `cmd.exe` on Windows, else a
/// POSIX one).
fn sign_in_line(path: &Path) -> String {
    let p = path.to_string_lossy();
    if cfg!(windows) {
        format!("\"{p}\" auth login")
    } else {
        format!("'{}' auth login", p.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_and_the_account_are_read() {
        assert_eq!(
            version_of("2.1.314 (Claude Code)\n").as_deref(),
            Some("2.1.314")
        );
        assert_eq!(version_of("error: no"), None);
        let signed_in = r#"{"loggedIn":true,"authMethod":"claude.ai","email":"a@b.c","subscriptionType":"max"}"#;
        assert_eq!(
            account_of(signed_in).unwrap(),
            Some(ClaudeAccount {
                email: "a@b.c".into(),
                plan: "max".into()
            })
        );
        let key = r#"{"loggedIn":true,"authMethod":"api_key","email":""}"#;
        assert_eq!(account_of(key).unwrap().unwrap().plan, "api_key");
        assert_eq!(account_of(r#"{"loggedIn":false}"#).unwrap(), None);
        assert!(account_of("not json").is_err());
    }

    /// This computer's Claude Code, by hand (`--ignored`): found, its
    /// version, signed in (the account itself is not printed).
    #[test]
    #[ignore]
    fn this_computer_s_claude_code() {
        let c = status();
        println!(
            "found: {}, version: {:?}, signed in: {}, problem: {:?}",
            c.path.is_some(),
            c.version,
            c.account.is_some(),
            c.problem
        );
        assert!(c.path.is_some() && c.version.is_some());
    }

    #[test]
    fn the_sign_in_line_quotes_the_path() {
        let line = sign_in_line(Path::new("/Users/o'neil/.local/bin/claude"));
        if cfg!(windows) {
            assert!(line.ends_with(" auth login"));
        } else {
            assert_eq!(line, "'/Users/o'\\''neil/.local/bin/claude' auth login");
        }
    }
}
