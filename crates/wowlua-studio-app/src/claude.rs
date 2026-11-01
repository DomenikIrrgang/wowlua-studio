//! The claude page (M14, S090, S095): Claude Code as Studio found it, and
//! signing in through it. The conversations come with M14's next steps.

use egui::Ui;
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, forms, palette};

use crate::settings::SettingsState;
use crate::system::{ClaudeCode, System};

/// What the page asks the app to do.
pub enum Action {
    /// Runs Claude Code's sign-in in the terminal: this line.
    SignIn(String),
}

/// The conversation's panel: whether Studio can run Claude Code, and
/// signing in (`waiting`: a sign-in runs in the terminal).
pub fn chat(
    ui: &mut Ui,
    settings: &mut SettingsState,
    system: &dyn System,
    waiting: bool,
) -> Option<Action> {
    let p = palette(ui.ctx());
    settings.read(system);
    let claude = settings.claude().cloned();
    let mut action = None;
    forms::padded(ui, 24, 20, |ui| {
        ui.set_max_width(580.0);
        let heading = |ui: &mut Ui, text: &str| {
            ui.label(
                egui::RichText::new(text)
                    .font(Face::PlexMedium.at(15.0))
                    .color(p.ink),
            );
            ui.add_space(6.0);
        };
        let text = |ui: &mut Ui, text: &str| {
            forms::paragraph(ui, text, 12.5, 19.0, p.ink_muted);
        };
        match claude.as_ref() {
            None => text(ui, "Looking for Claude Code…"),
            Some(c) if c.path.is_none() => {
                heading(ui, "Claude Code is not installed");
                text(
                    ui,
                    "Studio runs the Claude Code you install, on your Claude plan. Install it from claude.com/claude-code, then check again.",
                );
                ui.add_space(12.0);
                if widgets::button(ui, ButtonKind::Secondary, "check again", true, None).clicked() {
                    settings.read_again();
                }
            }
            Some(c) if c.account.is_none() => {
                heading(ui, "Claude Code is not signed in");
                text(
                    ui,
                    &format!(
                        "Studio runs the Claude Code you installed ({}), on your Claude plan. Claude sees the workspace you open and only acts through Studio's tools; you approve what it changes.",
                        found(c)
                    ),
                );
                if let Some(problem) = &c.problem {
                    ui.add_space(4.0);
                    forms::paragraph(ui, problem, 11.5, 17.0, p.ink_faint);
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if widgets::button(
                        ui,
                        ButtonKind::Primary,
                        "sign in through Claude Code",
                        !waiting,
                        None,
                    )
                    .clicked()
                    {
                        action = Some(Action::SignIn(c.sign_in.clone()));
                    }
                    if widgets::button(ui, ButtonKind::Secondary, "check again", true, None)
                        .clicked()
                    {
                        settings.read_again();
                    }
                });
                ui.add_space(8.0);
                forms::paragraph(
                    ui,
                    if waiting {
                        "Finish signing in in the terminal below; Studio notices when you are done."
                    } else {
                        "Runs Claude Code's own sign-in in the terminal, which opens your browser. Studio never sees or keeps your sign-in."
                    },
                    11.5,
                    17.0,
                    p.ink_faint,
                );
            }
            Some(c) => {
                heading(ui, "Claude Code is ready");
                text(ui, &format!("{} · {}", found(c), signed_in(c)));
                ui.add_space(8.0);
                text(
                    ui,
                    "Conversations come with the next steps of M14 (Studio's roadmap): Studio's tools for Claude, the conversation, its changes and checkpoints.",
                );
            }
        }
    });
    action
}

/// `Claude Code 2.1.314 at ~/.local/bin/claude`.
pub fn found(c: &ClaudeCode) -> String {
    let version = c
        .version
        .as_ref()
        .map(|v| format!("Claude Code {v}"))
        .unwrap_or_else(|| "Claude Code".into());
    match &c.path {
        Some(path) => format!("{version} at {}", path.display()),
        None => version,
    }
}

/// `signed in as a@b.c (max)`.
pub fn signed_in(c: &ClaudeCode) -> String {
    match &c.account {
        Some(a) if a.plan.is_empty() => format!("signed in as {}", a.email),
        Some(a) => format!("signed in as {} ({})", a.email, a.plan),
        None => "not signed in".into(),
    }
}
