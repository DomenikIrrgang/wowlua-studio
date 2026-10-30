//! A screenshot of the window, for checking how Studio looks without a
//! person at the screen: `WOWLUA_STUDIO_SCREENSHOT=out.png` draws a few
//! frames, saves the window as a PNG and quits.
//! `WOWLUA_STUDIO_SCREENSHOT_RUN` lists commands to run first, separated
//! by `;`: `open:<folder>`, `file:<file>`, `find:<query>`, `page:debug`, `theme:light`, `palette`,
//! `about`, `new`, `adopt`, `stats`, `terminal`, `runtests`, `runcoverage`,
//! `testtab:<tab>`, `coverage`, `gutter`, `rungame`, `gamepick:x,y`,
//! `gamesize:WxH[@scale]`, `units`, `bp:file:line`, `debug`, `debugtest:file::test`,
//! `profile:<speedscope file>`, `profiletest:file::test`, `record:<seconds>`,
//! `profview:<view>[@frame][#rank]`, `world:<profile>[/<part>][@json]`, `spells:<query>`,
//! `worldlive:<profile>|<path>=<json>|…`, `ship:<tab>[/<profile>]`,
//! `shipdo:<build|install|dry|publish|tag|dev>`, `settings:<section>`,
//! `manifest:<section>[@json]` (the workspace's wowlua.json in its tab),
//! `git:<branches|commit|new|switch:<branch>|discard>` (the git page's
//! parts, once the repository is read), `github:<run|failed|pull|new>`
//! (the pull requests page's, once GitHub answered), `fromgithub[:<owner/
//! name or link>]` (the start page's from-GitHub panel, a repository looked
//! up), `clone:<owner/name>@<branch>`, `close` (the folder, once open).

use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
};

use eframe::egui;
use wowlua_studio_app::{Studio, commands::Command, pages::Page};
use wowlua_studio_ui::Theme;

pub struct Screenshot {
    path: PathBuf,
    frame: u32,
    /// Seconds to wait first (`WOWLUA_STUDIO_SCREENSHOT_AFTER`): the
    /// language server's check takes a moment.
    after: f64,
    image: Option<mpsc::Receiver<Arc<egui::ColorImage>>>,
}

impl Screenshot {
    pub fn from_env(studio: &mut Studio) -> Option<Self> {
        let path = PathBuf::from(std::env::var_os("WOWLUA_STUDIO_SCREENSHOT")?);
        let run = std::env::var("WOWLUA_STUDIO_SCREENSHOT_RUN").unwrap_or_default();
        for step in run.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(folder) = step.strip_prefix("open:") {
                studio.open(std::path::Path::new(folder));
                continue;
            }
            if let Some(file) = step.strip_prefix("file:") {
                studio.edit(std::path::Path::new(file));
                continue;
            }
            if let Some(tab) = step.strip_prefix("testtab:") {
                studio.tests_tab(tab);
                continue;
            }
            if let Some(rest) = step.strip_prefix("debugtest:") {
                let (file, test) = match rest.split_once("::") {
                    Some((f, t)) => (f, Some(t)),
                    None => (rest, None),
                };
                studio.debug_test_later(std::path::Path::new(file), test);
                continue;
            }
            if let Some(file) = step.strip_prefix("profile:") {
                studio.profile_later(std::path::Path::new(file));
                continue;
            }
            if let Some(rest) = step.strip_prefix("profiletest:") {
                let (file, test) = match rest.split_once("::") {
                    Some((f, t)) => (f, Some(t)),
                    None => (rest, None),
                };
                studio.profile_test_later(std::path::Path::new(file), test);
                continue;
            }
            if let Some(seconds) = step.strip_prefix("record:").and_then(|s| s.parse().ok()) {
                studio.record_later(seconds);
                continue;
            }
            if let Some(rest) = step.strip_prefix("profview:") {
                let (rest, rank) = match rest.split_once('#') {
                    Some((r, n)) => (r, n.parse().ok()),
                    None => (rest, None),
                };
                let (view, frame) = match rest.split_once('@') {
                    Some((v, f)) => (v, f.parse().ok()),
                    None => (rest, None),
                };
                use wowlua_studio_app::profiler::View;
                let view = match view {
                    "tree" => View::Tree,
                    "flame" => View::Flame,
                    "dispatches" => View::Dispatches,
                    "slow" => View::Slow,
                    "hints" => View::Hints,
                    _ => View::Functions,
                };
                studio.profile_view(view, frame, rank);
                continue;
            }
            if step == "debug" {
                studio.start_debugging();
                continue;
            }
            if let Some(rest) = step.strip_prefix("bp:")
                && let Some((file, line)) = rest.rsplit_once(':')
                && let Ok(line) = line.parse()
            {
                studio.set_breakpoint(std::path::Path::new(file), line);
                continue;
            }
            if step == "rungame" {
                studio.run_game();
                continue;
            }
            if let Some(rest) = step.strip_prefix("gamesize:") {
                let (size, scale) = match rest.split_once('@') {
                    Some((s, k)) => (s, k.parse().ok()),
                    None => (rest, None),
                };
                if let Some((w, h)) = size
                    .split_once('x')
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                {
                    studio.game_size((w, h), scale);
                }
                continue;
            }
            if let Some((x, y)) = step
                .strip_prefix("gamepick:")
                .and_then(|p| p.split_once(','))
                .and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)))
            {
                studio.game_pick(x, y);
                continue;
            }
            if let Some(rest) = step.strip_prefix("worldlive:") {
                let mut parts = rest.split('|');
                let profile = parts.next().unwrap_or_default();
                let edits = parts
                    .filter_map(|e| e.split_once('='))
                    .map(|(p, v)| (p.to_owned(), v.to_owned()))
                    .collect();
                studio.world_live_later(profile, edits);
                continue;
            }
            if let Some(rest) = step.strip_prefix("fromgithub") {
                studio.harness_from_github(rest.trim_start_matches(':'));
                continue;
            }
            if let Some(rest) = step.strip_prefix("clone:") {
                let (repo, branch) = rest.split_once('@').unwrap_or((rest, "main"));
                studio.start_clone(repo, branch);
                continue;
            }
            if step == "close" {
                studio.close_later();
                continue;
            }
            if let Some(line) = step.strip_prefix("chatsend:") {
                studio.game_chat_later(line);
                continue;
            }
            if step == "fullscreen" {
                studio.full_screen_later();
                continue;
            }
            if step == "world" {
                studio.world_on();
                continue;
            }
            if let Some(rest) = step.strip_prefix("hold:") {
                let (key, secs) = rest.split_once(':').unwrap_or((rest, "1"));
                studio.hold_later(key, secs.parse().unwrap_or(1.0));
                continue;
            }
            if let Some(rest) = step.strip_prefix("github:") {
                studio.github_show(rest);
                continue;
            }
            if let Some(rest) = step.strip_prefix("git:") {
                studio.git_show(rest);
                continue;
            }
            if let Some(rest) = step.strip_prefix("ship:") {
                let (tab, profile) = match rest.split_once('/') {
                    Some((t, p)) => (t, Some(p)),
                    None => (rest, None),
                };
                studio.ship_show(tab, profile);
                continue;
            }
            if let Some(what) = step.strip_prefix("settings:") {
                studio.settings_show(what);
                continue;
            }
            if let Some(what) = step.strip_prefix("manifest:") {
                let (section, json) = match what.strip_suffix("@json") {
                    Some(s) => (s, true),
                    None => (what, false),
                };
                studio.manifest_show(section, json);
                continue;
            }
            if let Some(what) = step.strip_prefix("shipdo:") {
                studio.ship_later(what);
                continue;
            }
            if let Some(query) = step.strip_prefix("spells:") {
                studio.world_search(query);
                continue;
            }
            if let Some(rest) = step.strip_prefix("world:") {
                let (rest, json) = match rest.strip_suffix("@json") {
                    Some(r) => (r, true),
                    None => (rest, false),
                };
                let (profile, part) = match rest.split_once('/') {
                    Some((p, part)) => (p, Some(part)),
                    None => (rest, None),
                };
                studio.world_show(profile, part, json);
                continue;
            }
            if step == "runtests" || step == "runcoverage" {
                studio.run_tests(step == "runcoverage");
                continue;
            }
            if step == "gutter" {
                studio.coverage_gutter();
                continue;
            }
            if let Some(query) = step.strip_prefix("find:") {
                studio.find(query);
                continue;
            }
            let command = match step.split_once(':') {
                Some(("page", name)) => Page::ALL
                    .into_iter()
                    .find(|p| p.label() == name)
                    .map(Command::ShowPage),
                Some(("theme", name)) => Theme::from_name(name).map(Command::SetTheme),
                None if step == "palette" => Some(Command::OpenPalette),
                None if step == "about" => Some(Command::About),
                None if step == "new" => Some(Command::NewAddon),
                None if step == "adopt" => Some(Command::AdoptAddon),
                None if step == "stats" => Some(Command::FrameStats(true)),
                None if step == "units" => Some(Command::ShowPanel(
                    wowlua_studio_app::panels::Panel::Units,
                    wowlua_studio_app::dock::Dock::Left,
                )),
                None if step == "coverage" => Some(Command::ShowPanel(
                    wowlua_studio_app::panels::Panel::Coverage,
                    wowlua_studio_app::dock::Dock::Main,
                )),
                None if step == "gameinput" => Some(Command::ShowPanel(
                    wowlua_studio_app::panels::Panel::Input,
                    wowlua_studio_app::dock::Dock::Bottom,
                )),
                None if step == "gamechat" => Some(Command::ShowPanel(
                    wowlua_studio_app::panels::Panel::GameChat,
                    wowlua_studio_app::dock::Dock::Bottom,
                )),
                None if step == "terminal" => Some(Command::ShowPanel(
                    wowlua_studio_app::panels::Panel::Terminal,
                    wowlua_studio_app::dock::Dock::Bottom,
                )),
                _ => None,
            };
            match command {
                Some(c) => studio.queue(c),
                None => eprintln!("wowlua studio: unknown screenshot step `{step}`"),
            }
        }
        Some(Screenshot {
            path,
            frame: 0,
            after: std::env::var("WOWLUA_STUDIO_SCREENSHOT_AFTER")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0),
            image: None,
        })
    }

    pub fn step(&mut self, ctx: &egui::Context, studio: &mut Studio) {
        self.frame += 1;
        ctx.request_repaint();
        // Opening a folder takes a moment: the shot waits for it.
        if self.frame >= 30 && ctx.input(|i| i.time) >= self.after && self.image.is_none() {
            let (send, receive) = mpsc::channel();
            self.image = Some(receive);
            let callback = egui::ScreenshotCallback::new(move |image| {
                let _ = send.send(image);
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(callback));
        }
        let image = self.image.as_ref().and_then(|r| r.try_recv().ok());
        if let Some(image) = image {
            if let Err(e) = save_png(&self.path, &image) {
                eprintln!("wowlua studio: could not save {}: {e}", self.path.display());
            }
            studio.quit_as_is();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn save_png(path: &PathBuf, image: &egui::ColorImage) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let [w, h] = image.size;
    let mut encoder = png::Encoder::new(file, w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    writer.write_image_data(&bytes)?;
    Ok(())
}
