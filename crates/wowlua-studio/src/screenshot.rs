//! A screenshot of the window, for checking how Studio looks without a
//! person at the screen: `WOWLUA_STUDIO_SCREENSHOT=out.png` draws a few
//! frames, saves the window as a PNG and quits.
//! `WOWLUA_STUDIO_SCREENSHOT_RUN` lists commands to run first, separated
//! by `;`: `open:<folder>`, `file:<file>`, `find:<query>`, `page:debug`, `theme:light`, `palette`,
//! `about`, `new`, `adopt`, `stats`, `terminal`, `runtests`, `runcoverage`,
//! `testtab:<tab>`, `coverage`, `gutter`, `rungame`, `gamepick:x,y`,
//! `gamesize:WxH[@scale]`, `units`, `bp:file:line`, `debug`, `debugtest:file::test`.

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

    pub fn step(&mut self, ctx: &egui::Context) {
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
