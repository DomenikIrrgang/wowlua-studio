//! Studio's own log (M10, S062): what it says on standard error, also in
//! `studio.log` in its data folder, for bug reports. Over a megabyte, the
//! log moves to `studio.old.log` and starts again.

use std::io::Write;
use std::path::PathBuf;

const LIMIT: u64 = 1 << 20;

/// The log file.
pub fn path() -> Option<PathBuf> {
    crate::store::data_dir().map(|d| d.join("studio.log"))
}

/// Writes a line, with the time, to standard error and the log.
pub fn line(text: &str) {
    eprintln!("wowlua studio: {text}");
    let Some(path) = path() else { return };
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LIMIT) {
        let _ = std::fs::rename(&path, path.with_file_name("studio.old.log"));
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    let now = jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S").to_string();
    let _ = writeln!(file, "{now} {text}");
}
