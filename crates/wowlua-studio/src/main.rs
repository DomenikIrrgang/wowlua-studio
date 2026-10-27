//! wowlua studio: the IDE for wowlua (spec/roadmap.md). The native host
//! of the application (`wowlua-studio-app`): the window, the files in the
//! system's folder for application data, macOS's menu bar.

mod frame_stats;
#[cfg(target_os = "macos")]
mod menu_bar;
mod screenshot;
mod store;

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
        Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))),
    )
}

struct App {
    studio: Studio,
    stats: Option<FrameStats>,
    screenshot: Option<Screenshot>,
    #[cfg(target_os = "macos")]
    menu_bar: menu_bar::MenuBar,
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        let host = Host {
            platform: PLATFORM,
            wowlua_version: wowlua_driver::COMPILER_VERSION.to_owned(),
            store: Box::new(store::FileStore::new()),
        };
        let mut studio = Studio::new(ctx, host);
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
        for command in self.menu_bar.poll() {
            self.studio.queue(command);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
