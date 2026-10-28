//! The settings page's work on this computer (M10, S062): what it shows
//! (the check cache, the art kept on disk, the log, the GitHub account,
//! whether a Claude key is saved), clearing the caches, Studio's releases,
//! GitHub's device sign-in and the Claude API key. Tokens and keys are kept
//! in the OS credential store through wowlua's credentials; the account's
//! name in Studio's data, so showing it reads no store.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value as Json;
use wowlua_driver::publish::http::Http;
use wowlua_studio_app::system::{GithubSignIn, SettingsInfo, UpdateCheck};

/// The credential store's names of Studio's own secrets (not `wowlua
/// login`'s site tokens).
const GITHUB: &str = "studio-github";
const CLAUDE: &str = "anthropic";

/// The OAuth App Studio signs in to GitHub with: its client ID, built in
/// or in `WOWLUA_STUDIO_GITHUB_CLIENT_ID` (S062: registered by the user).
fn github_client_id() -> Option<String> {
    std::env::var("WOWLUA_STUDIO_GITHUB_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("WOWLUA_STUDIO_GITHUB_CLIENT_ID").map(str::to_owned))
        .filter(|id| !id.trim().is_empty())
}

/// The installation's art kept on disk (S062).
pub fn art_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("wowlua-studio").join("art"))
}

/// A folder's bytes and files.
fn size_of(dir: &Path) -> (u64, usize) {
    let mut bytes = 0;
    let mut files = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in read.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                bytes += meta.len();
                files += 1;
            }
        }
    }
    (bytes, files)
}

fn account_file() -> Option<PathBuf> {
    crate::store::data_dir().map(|d| d.join("accounts.json"))
}

fn accounts() -> Json {
    account_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn set_account(key: &str, value: Json) {
    let Some(path) = account_file() else { return };
    let mut all = accounts();
    if value.is_null() {
        if let Some(o) = all.as_object_mut() {
            o.remove(key);
        }
    } else {
        all[key] = value;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(&all).unwrap_or_default(),
    );
}

pub fn info() -> SettingsInfo {
    let check = wowlua_pkg::Cache::new()
        .map(|c| c.part("check").0)
        .filter(|b| *b > 0);
    let art = art_dir().map(|d| size_of(&d)).filter(|(b, _)| *b > 0);
    let accounts = accounts();
    SettingsInfo {
        check_cache: check,
        art_cache: art,
        log: crate::log::path().filter(|p| p.exists()),
        github: accounts["github"].as_str().map(str::to_owned),
        claude_key: accounts["claude"].as_bool() == Some(true),
    }
}

pub fn clear_check_cache() {
    if let Some(cache) = wowlua_pkg::Cache::new()
        && let Err(e) = cache.clear_part("check")
    {
        crate::log::line(&format!("cannot clear the check cache: {e}"));
    }
}

pub fn clear_art_cache() {
    if let Some(dir) = art_dir()
        && dir.exists()
        && let Err(e) = std::fs::remove_dir_all(&dir)
    {
        crate::log::line(&format!("cannot clear the art cache: {e}"));
    }
}

/// Studio's newest release on GitHub against this one.
pub fn check_updates() -> UpdateCheck {
    let http = Http::default();
    let url = "https://api.github.com/repos/DomenikIrrgang/wowlua-studio/releases/latest";
    let headers = [
        ("accept", "application/vnd.github+json".to_owned()),
        ("user-agent", "wowlua-studio".to_owned()),
    ];
    let response = match http.request("GET", url, &headers, None) {
        Ok(r) => r,
        Err(e) => return UpdateCheck::Failed(format!("cannot reach GitHub: {e}")),
    };
    // (no release yet)
    if response.status == 404 {
        return UpdateCheck::Newest;
    }
    if !response.ok() {
        return UpdateCheck::Failed(format!("GitHub answered HTTP {}", response.status));
    }
    let json = response.json().unwrap_or_default();
    newer(
        json["tag_name"].as_str().unwrap_or_default(),
        json["html_url"].as_str().unwrap_or_default(),
        env!("CARGO_PKG_VERSION"),
    )
}

/// Whether the release `tag` is newer than `current`.
fn newer(tag: &str, url: &str, current: &str) -> UpdateCheck {
    let parse = |v: &str| semver::Version::parse(v.trim_start_matches('v')).ok();
    match (parse(tag), parse(current)) {
        (Some(t), Some(c)) if t > c => UpdateCheck::Newer {
            version: t.to_string(),
            url: url.to_owned(),
        },
        (Some(_), Some(_)) => UpdateCheck::Newest,
        _ => UpdateCheck::Failed(format!("cannot read the release's version `{tag}`")),
    }
}

/// Saves the Claude API key in the OS credential store, or removes it.
pub fn claude_key(key: Option<&str>) {
    let result = match key {
        Some(k) => wowlua_driver::credentials::set(CLAUDE, k).map(|_| ()),
        None => wowlua_driver::credentials::remove(CLAUDE).map(|_| ()),
    };
    match result {
        Ok(()) => set_account(
            "claude",
            if key.is_some() {
                Json::Bool(true)
            } else {
                Json::Null
            },
        ),
        Err(e) => crate::log::line(&format!("cannot keep the Claude key: {e}")),
    }
}

/// Signs out of GitHub: the token and the account's name are forgotten.
pub fn github_sign_out() {
    let _ = wowlua_driver::credentials::remove(GITHUB);
    set_account("github", Json::Null);
}

/// A form body.
fn form(pairs: &[(&str, &str)]) -> Vec<u8> {
    let encode = |s: &str| {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                b' ' => "+".into(),
                _ => format!("%{b:02X}"),
            })
            .collect::<String>()
    };
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect::<Vec<_>>()
        .join("&")
        .into_bytes()
}

/// GitHub's device flow (S062): the code to enter, then waiting for the
/// user to enter it, the token kept in the OS credential store.
pub fn github_sign_in(stop: Arc<AtomicBool>, tell: impl Fn(GithubSignIn)) {
    let Some(client) = github_client_id() else {
        tell(GithubSignIn::Failed(
            "Studio's GitHub app is not configured yet (WOWLUA_STUDIO_GITHUB_CLIENT_ID)".into(),
        ));
        return;
    };
    let http = Http::default();
    let headers = [
        ("accept", "application/json".to_owned()),
        (
            "content-type",
            "application/x-www-form-urlencoded".to_owned(),
        ),
        ("user-agent", "wowlua-studio".to_owned()),
    ];
    let body = form(&[("client_id", &client), ("scope", "repo read:org workflow")]);
    let start = match http.request(
        "POST",
        "https://github.com/login/device/code",
        &headers,
        Some(&body),
    ) {
        Ok(r) if r.ok() => r.json().unwrap_or_default(),
        Ok(r) => {
            return tell(GithubSignIn::Failed(format!(
                "GitHub answered HTTP {}",
                r.status
            )));
        }
        Err(e) => return tell(GithubSignIn::Failed(format!("cannot reach GitHub: {e}"))),
    };
    let (Some(device), Some(code)) = (start["device_code"].as_str(), start["user_code"].as_str())
    else {
        return tell(GithubSignIn::Failed("GitHub's answer has no code".into()));
    };
    let url = start["verification_uri"]
        .as_str()
        .unwrap_or("https://github.com/login/device")
        .to_owned();
    tell(GithubSignIn::Code {
        code: code.to_owned(),
        url,
    });
    let mut interval = start["interval"].as_u64().unwrap_or(5).max(1);
    let token = loop {
        // (waits the interval GitHub asks, stopping when asked)
        for _ in 0..interval * 5 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let body = form(&[
            ("client_id", &client),
            ("device_code", device),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ]);
        let answer = match http.request(
            "POST",
            "https://github.com/login/oauth/access_token",
            &headers,
            Some(&body),
        ) {
            Ok(r) => r.json().unwrap_or_default(),
            Err(e) => return tell(GithubSignIn::Failed(format!("cannot reach GitHub: {e}"))),
        };
        if let Some(token) = answer["access_token"].as_str() {
            break token.to_owned();
        }
        match answer["error"].as_str() {
            Some("authorization_pending") => {}
            Some("slow_down") => interval += 5,
            Some("expired_token") => {
                return tell(GithubSignIn::Failed(
                    "the code expired: sign in again".into(),
                ));
            }
            Some("access_denied") => {
                return tell(GithubSignIn::Failed(
                    "the sign-in was declined on GitHub".into(),
                ));
            }
            Some(other) => return tell(GithubSignIn::Failed(format!("GitHub said {other}"))),
            None => return tell(GithubSignIn::Failed("GitHub's answer has no token".into())),
        }
    };
    let user = http
        .request(
            "GET",
            "https://api.github.com/user",
            &[
                ("authorization", format!("Bearer {token}")),
                ("accept", "application/vnd.github+json".to_owned()),
                ("user-agent", "wowlua-studio".to_owned()),
            ],
            None,
        )
        .ok()
        .and_then(|r| r.json())
        .unwrap_or_default();
    let login = user["login"].as_str().unwrap_or("GitHub").to_owned();
    if let Err(e) = wowlua_driver::credentials::set(GITHUB, &token) {
        return tell(GithubSignIn::Failed(format!("cannot keep the token: {e}")));
    }
    set_account("github", Json::String(login.clone()));
    tell(GithubSignIn::SignedIn(login));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_compare_by_version() {
        assert_eq!(newer("v0.1.0", "u", "0.1.0"), UpdateCheck::Newest);
        assert_eq!(
            newer("v0.2.0", "https://x", "0.1.0"),
            UpdateCheck::Newer {
                version: "0.2.0".into(),
                url: "https://x".into()
            }
        );
        assert!(matches!(
            newer("nightly", "u", "0.1.0"),
            UpdateCheck::Failed(_)
        ));
    }

    #[test]
    fn forms_are_encoded() {
        assert_eq!(
            String::from_utf8(form(&[("scope", "repo read:org"), ("id", "a&b")])).unwrap(),
            "scope=repo+read%3Aorg&id=a%26b"
        );
    }
}
