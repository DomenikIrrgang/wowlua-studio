//! Studio's files in the system's folder for application data (S012):
//! `~/Library/Application Support/wowlua-studio` on macOS,
//! `%APPDATA%\wowlua-studio` on Windows, `~/.config/wowlua-studio` on
//! Linux. `WOWLUA_STUDIO_DATA` names another folder.

use std::path::PathBuf;

use wowlua_studio_app::Store;

pub struct FileStore {
    dir: Option<PathBuf>,
}

impl FileStore {
    pub fn new() -> Self {
        let dir = std::env::var_os("WOWLUA_STUDIO_DATA")
            .map(PathBuf::from)
            .or_else(|| dirs::config_dir().map(|d| d.join("wowlua-studio")));
        FileStore { dir }
    }
}

impl Store for FileStore {
    fn load(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.dir.as_ref()?.join(name)).ok()
    }

    fn save(&self, name: &str, text: &str) {
        let Some(dir) = &self.dir else { return };
        // Written beside, then renamed: a crash never leaves half a file.
        let result = std::fs::create_dir_all(dir).and_then(|()| {
            let tmp = dir.join(format!("{name}.tmp"));
            std::fs::write(&tmp, text)?;
            std::fs::rename(&tmp, dir.join(name))
        });
        if let Err(e) = result {
            eprintln!(
                "wowlua studio: could not save {name} in {}: {e}",
                dir.display()
            );
        }
    }
}
