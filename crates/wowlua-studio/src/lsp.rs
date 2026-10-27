//! wowlua's language server in Studio's process (`wowlua_lsp::embedded`),
//! on a thread of its own: Studio's messages go in through a channel, the
//! server's come back as events. Checks run on that thread when the server
//! says they are due, so the window never waits for one.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Instant;

use eframe::egui;
use serde_json::Value as Json;
use wowlua_lsp::embedded::{Change, Embedded};
use wowlua_studio_app::system::{Event, FileChange};

/// What Studio sends the server's thread.
pub enum Input {
    Message(Json),
    Files(Vec<(PathBuf, FileChange)>),
}

/// Starts the server's thread; its messages go to `events`.
pub fn start(events: Sender<Event>, ctx: egui::Context) -> Sender<Input> {
    let (send, receive) = channel();
    std::thread::Builder::new()
        .name("wowlua language server".into())
        .spawn(move || run(&receive, &events, &ctx))
        .expect("a thread for the language server");
    send
}

fn run(input: &Receiver<Input>, events: &Sender<Event>, ctx: &egui::Context) {
    let start = Instant::now();
    let mut server = Embedded::new();
    loop {
        server.set_time(start.elapsed());
        let next = match server.wait() {
            Some(due) => input.recv_timeout(due),
            None => input.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        server.set_time(start.elapsed());
        let out = match next {
            Ok(Input::Message(message)) => server.message(&message),
            Ok(Input::Files(files)) => {
                let changes: Vec<(&std::path::Path, Change)> = files
                    .iter()
                    .map(|(path, change)| {
                        let change = match change {
                            FileChange::Created => Change::Created,
                            FileChange::Changed => Change::Changed,
                            FileChange::Deleted => Change::Deleted,
                        };
                        (path.as_path(), change)
                    })
                    .collect();
                server.files_changed(&changes);
                Vec::new()
            }
            Err(RecvTimeoutError::Timeout) => {
                // Each message as it is made: a check's beginning before
                // the check runs.
                server.run_due_with(|message| {
                    let _ = events.send(Event::Lsp(message));
                    ctx.request_repaint();
                });
                Vec::new()
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if !out.is_empty() {
            for message in out {
                if events.send(Event::Lsp(message)).is_err() {
                    return;
                }
            }
            ctx.request_repaint();
        }
        if server.exited() {
            return;
        }
    }
}
