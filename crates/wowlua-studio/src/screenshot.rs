//! A screenshot of the window, for checking how Studio looks without a
//! person at the screen: `WOWLUA_STUDIO_SCREENSHOT=out.png` draws a few
//! frames, saves the window as a PNG and quits.
//! `WOWLUA_STUDIO_SCREENSHOT_RUN` lists commands to run first, separated
//! by `;`: `page:debug`, `theme:light`, `palette`, `drag:watch`.

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
    image: Option<mpsc::Receiver<Arc<egui::ColorImage>>>,
}

impl Screenshot {
    pub fn from_env(studio: &mut Studio) -> Option<Self> {
        let path = PathBuf::from(std::env::var_os("WOWLUA_STUDIO_SCREENSHOT")?);
        let run = std::env::var("WOWLUA_STUDIO_SCREENSHOT_RUN").unwrap_or_default();
        for step in run.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let command = match step.split_once(':') {
                Some(("page", name)) => Page::ALL
                    .into_iter()
                    .find(|p| p.label() == name)
                    .map(Command::ShowPage),
                Some(("theme", name)) => Theme::from_name(name).map(Command::SetTheme),
                None if step == "palette" => Some(Command::OpenPalette),
                None if step == "about" => Some(Command::About),
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
            image: None,
        })
    }

    pub fn step(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        ctx.request_repaint();
        if self.frame == 5 && self.image.is_none() {
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
