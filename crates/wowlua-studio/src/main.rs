//! wowlua studio: the IDE for wowlua (spec/roadmap.md). The native host
//! of the application (`wowlua-studio-app`): the window, the files in the
//! system's folder for application data, the system (`native`), macOS's
//! menu bar. `wowlua-studio <folder>` opens the folder (S031: a new window
//! is Studio started again with it).

mod art;
mod bindings;
mod deps;
mod frame_stats;
mod game;
mod gamedata;
mod log;
mod lsp;
#[cfg(target_os = "macos")]
mod menu_bar;
mod native;
mod profiles;
mod screenshot;
mod settings;
mod ship;
mod store;
mod terminal;
mod tests;
#[cfg(target_os = "macos")]
mod traffic_lights;
mod world;

use eframe::egui;
use frame_stats::FrameStats;
use screenshot::Screenshot;
use wowlua_studio_app::{Host, Platform, Studio};

const PLATFORM: Platform = if cfg!(target_os = "macos") {
    Platform::MacOs
} else if cfg!(target_os = "windows") {
    Platform::Windows
} else {
    Platform::Linux
};

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("wowlua studio")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([800.0, 500.0]);
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }
    viewport = if PLATFORM == Platform::MacOs {
        // The traffic lights stay, over Studio's own title bar (S013).
        viewport
            .with_fullsize_content_view(true)
            .with_title_shown(false)
            .with_titlebar_shown(false)
    } else {
        // Studio draws its title bar and window buttons (the design's).
        viewport.with_decorations(false)
    };
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "wowlua studio",
        options,
        Box::new(|cc| {
            Ok(Box::new(App::new(
                &cc.egui_ctx,
                cc.wgpu_render_state.clone(),
            )))
        }),
    )
}

/// The app's icon (decision S020): the mark tile, in Apple's icon shape on
/// macOS (`scripts/app-icon.py` makes both).
fn app_icon() -> Option<egui::IconData> {
    let bytes: &[u8] = if PLATFORM == Platform::MacOs {
        include_bytes!("../assets/icon-macos.png")
    } else {
        include_bytes!("../assets/icon.png")
    };
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut rgba).ok()?;
    rgba.truncate(info.buffer_size());
    Some(egui::IconData {
        rgba,
        width: info.width,
        height: info.height,
    })
}

struct App {
    studio: Studio,
    stats: Option<FrameStats>,
    screenshot: Option<Screenshot>,
    #[cfg(target_os = "macos")]
    menu_bar: menu_bar::MenuBar,
}

impl App {
    fn new(ctx: &egui::Context, gpu: Option<eframe::egui_wgpu::RenderState>) -> Self {
        let mut system = native::NativeSystem::new(ctx);
        // The game's screen is drawn on eframe's device (S055).
        if let Some(gpu) = gpu {
            system.set_gpu(gpu);
        }
        let host = Host {
            platform: PLATFORM,
            wowlua_version: wowlua_driver::COMPILER_VERSION.to_owned(),
            store: Box::new(store::FileStore::new()),
            system: Box::new(system),
        };
        let mut studio = Studio::new(ctx, host);
        if let Some(folder) = std::env::args_os().nth(1) {
            let folder = std::path::PathBuf::from(folder);
            studio.open(&std::path::absolute(&folder).unwrap_or(folder));
        }
        let screenshot = Screenshot::from_env(&mut studio);
        App {
            #[cfg(target_os = "macos")]
            menu_bar: menu_bar::MenuBar::new(ctx, &studio.entries()),
            studio,
            stats: FrameStats::from_env(),
            screenshot,
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            for command in self.menu_bar.poll() {
                self.studio.queue(command);
            }
            if self.studio.take_menus_changed() {
                self.menu_bar.rebuild(&self.studio.entries());
            }
            traffic_lights::place(_frame);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if let Some(cpu) = frame.info().cpu_usage {
            self.studio.set_frame_time(cpu);
        }
        let started = std::time::Instant::now();
        self.studio.ui(ui);
        if let Some(stats) = &mut self.stats {
            stats.record(started.elapsed());
        }
        if let Some(shot) = &mut self.screenshot {
            shot.step(ui.ctx());
        }
    }

    fn on_exit(&mut self) {
        self.studio.save();
    }
}
