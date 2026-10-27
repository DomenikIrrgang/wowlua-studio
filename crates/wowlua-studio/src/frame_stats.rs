//! How long building the UI takes, frame by frame (decision S005): with
//! `WOWLUA_STUDIO_FRAME_STATS` set, Studio prints the median, the 99th
//! percentile and the slowest frame of each interval to stderr.

use std::time::{Duration, Instant};

const INTERVAL: Duration = Duration::from_secs(2);

pub struct FrameStats {
    since: Instant,
    frames: Vec<Duration>,
}

impl FrameStats {
    /// Stats when the environment asks for them.
    pub fn from_env() -> Option<Self> {
        std::env::var_os("WOWLUA_STUDIO_FRAME_STATS").map(|_| Self {
            since: Instant::now(),
            frames: Vec::new(),
        })
    }

    pub fn record(&mut self, frame: Duration) {
        self.frames.push(frame);
        if self.since.elapsed() < INTERVAL {
            return;
        }
        self.frames.sort_unstable();
        let at = |q: f64| self.frames[((self.frames.len() - 1) as f64 * q) as usize];
        eprintln!(
            "[frames] {} frames · median {:.2} ms · p99 {:.2} ms · slowest {:.2} ms",
            self.frames.len(),
            ms(at(0.5)),
            ms(at(0.99)),
            ms(at(1.0)),
        );
        self.frames.clear();
        self.since = Instant::now();
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
