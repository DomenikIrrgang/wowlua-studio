//! The link between Claude Code and Studio's tools (M14, S096): Claude
//! Code starts `wowlua-studio mcp-bridge <socket> <token>`, which passes its
//! standard input and output to the running Studio over a local socket, the
//! token first. No port is opened; the socket is in the user's own
//! temporary folder and only the user may open it.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(windows)]
use uds_windows::{UnixListener, UnixStream};

use serde_json::Value as Json;

/// One connection's MCP server: a JSON-RPC message in, its answer out
/// (none for a notification).
pub type Serve = Box<dyn FnMut(&Json) -> Option<Json> + Send>;

/// `wowlua-studio mcp-bridge`: passes lines both ways until either side
/// ends.
pub fn bridge(socket: &Path, token: &str) -> std::io::Result<()> {
    let stream = UnixStream::connect(socket)?;
    pipe(stream, token, std::io::stdin().lock(), std::io::stdout())
}

/// The token, then `input`'s lines to the socket while the socket's go to
/// `output`.
fn pipe(
    stream: UnixStream,
    token: &str,
    input: impl BufRead,
    mut output: impl Write + Send + 'static,
) -> std::io::Result<()> {
    let back = stream.try_clone()?;
    let answers = std::thread::spawn(move || {
        for line in BufReader::new(back).lines() {
            let Ok(line) = line else { break };
            if writeln!(output, "{line}")
                .and_then(|_| output.flush())
                .is_err()
            {
                break;
            }
        }
    });
    let mut to = stream;
    writeln!(to, "{token}")?;
    for line in input.lines() {
        let line = line?;
        writeln!(to, "{line}")?;
        to.flush()?;
    }
    // (no more requests: the answers still come, then the socket closes)
    let _ = to.shutdown(std::net::Shutdown::Write);
    let _ = answers.join();
    Ok(())
}

/// Where Claude Code reaches Studio while a conversation runs: the
/// socket, its one-time token, and the thread that accepts.
pub struct Link {
    pub socket: PathBuf,
    pub token: String,
    stop: Arc<AtomicBool>,
}

impl Link {
    /// Listens; each connection that sends the token first gets a server
    /// of its own from `serve`, on a thread of its own.
    pub fn open(serve: impl Fn() -> Serve + Send + 'static) -> std::io::Result<Link> {
        static N: AtomicU64 = AtomicU64::new(0);
        let socket = std::env::temp_dir().join(format!(
            "wowlua-studio-{}-{}.sock",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
        }
        let token = token()?;
        let stop = Arc::new(AtomicBool::new(false));
        let (want, quit) = (token.clone(), stop.clone());
        std::thread::Builder::new()
            .name("mcp link".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    if quit.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(conn) = conn else { continue };
                    let (want, server) = (want.clone(), serve());
                    let _ = std::thread::Builder::new()
                        .name("mcp".into())
                        .spawn(move || answer(conn, &want, server));
                }
            })?;
        Ok(Link {
            socket,
            token,
            stop,
        })
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // (a connection wakes the accepting thread, which then ends)
        let _ = UnixStream::connect(&self.socket);
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// One connection: the token, then each request answered in turn.
fn answer(conn: UnixStream, token: &str, mut serve: Serve) {
    let Ok(mut out) = conn.try_clone() else {
        return;
    };
    let mut lines = BufReader::new(conn).lines();
    match lines.next() {
        Some(Ok(t)) if t == token => {}
        _ => return,
    }
    for line in lines {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Json>(&line) {
            Ok(msg) => serve(&msg),
            Err(e) => Some(serde_json::json!({
                "jsonrpc": "2.0", "id": null,
                "error": { "code": -32700, "message": e.to_string() },
            })),
        };
        if let Some(r) = reply
            && writeln!(out, "{r}").and_then(|_| out.flush()).is_err()
        {
            break;
        }
    }
}

/// 16 random bytes, as hex.
fn token() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// What the bridge writes, kept for the test.
    #[derive(Clone, Default)]
    struct Kept(Arc<Mutex<Vec<u8>>>);

    impl Write for Kept {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn echo() -> Serve {
        Box::new(|msg: &Json| {
            let id = msg.get("id")?.clone();
            Some(
                serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": { "method": msg["method"] } }),
            )
        })
    }

    #[test]
    fn the_bridge_passes_requests_and_answers() {
        let link = Link::open(echo).unwrap();
        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n\
                     {\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\
                     {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";
        let kept = Kept::default();
        let stream = UnixStream::connect(&link.socket).unwrap();
        pipe(stream, &link.token, input.as_bytes(), kept.clone()).unwrap();
        let out = String::from_utf8(kept.0.lock().unwrap().clone()).unwrap();
        let answers: Vec<Json> = out
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(answers.len(), 2, "{out}");
        assert_eq!(answers[0]["result"]["method"], "initialize");
        assert_eq!(answers[1]["id"], 2);
    }

    #[test]
    fn a_wrong_token_gets_nothing() {
        let link = Link::open(echo).unwrap();
        let kept = Kept::default();
        let stream = UnixStream::connect(&link.socket).unwrap();
        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n";
        pipe(stream, "not-the-token", input.as_bytes(), kept.clone()).unwrap();
        assert!(kept.0.lock().unwrap().is_empty());
        let socket = link.socket.clone();
        drop(link);
        assert!(!socket.exists(), "the socket goes with the link");
    }
}
