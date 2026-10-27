//! wowlua studio: the IDE for wowlua (spec/roadmap.md). A native window
//! drawn with egui on wgpu, over wowlua's libraries in the same process.

mod frame_stats;

use eframe::egui;
use frame_stats::FrameStats;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("wowlua studio")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "wowlua studio",
        options,
        Box::new(|_cc| Ok(Box::new(Studio::new()))),
    )
}

struct Studio {
    stats: Option<FrameStats>,
}

impl Studio {
    fn new() -> Self {
        Self {
            stats: FrameStats::from_env(),
        }
    }
}

impl eframe::App for Studio {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let started = std::time::Instant::now();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("wowlua studio");
            ui.label(format!(
                "wowlua {} built in",
                wowlua_driver::COMPILER_VERSION
            ));
        });
        if let Some(stats) = &mut self.stats {
            stats.record(started.elapsed());
        }
    }
}
