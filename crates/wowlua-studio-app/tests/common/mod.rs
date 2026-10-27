//! What the tests share: a store in memory and a system whose folders open
//! at once.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use wowlua_studio_app::{
    Host, Platform, Store, Studio,
    system::{self, Entry, Member, NoSystem, Opened, System, Workspace},
};

/// Files in memory, shared with the test that reads them.
#[derive(Clone, Default)]
pub struct Memory(pub Rc<RefCell<HashMap<String, String>>>);

impl Store for Memory {
    fn load(&self, name: &str) -> Option<String> {
        self.0.borrow().get(name).cloned()
    }

    fn save(&self, name: &str, text: &str) {
        self.0.borrow_mut().insert(name.to_owned(), text.to_owned());
    }
}

/// Studio on Linux with [`Fake`]'s system; its files.
pub fn studio(ctx: &egui::Context) -> (Studio, Memory) {
    let memory = Memory::default();
    let studio = Studio::new(
        ctx,
        Host {
            platform: Platform::Linux,
            wowlua_version: "0.13.0".to_owned(),
            store: Box::new(memory.clone()),
            system: Box::new(Fake::default()),
        },
    );
    (studio, memory)
}

/// A system whose folders open at once as a workspace of three packages,
/// each folder holding a few files; a folder named `plain` has no
/// `wowlua.json`.
#[derive(Default)]
pub struct Fake {
    events: RefCell<Vec<system::Event>>,
}

impl System for Fake {
    fn now(&self) -> u64 {
        0
    }
    fn home(&self) -> Option<PathBuf> {
        None
    }
    fn exists(&self, _: &Path) -> bool {
        true
    }
    fn pick_folder(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn list(&self, dir: &Path) -> Vec<Entry> {
        let mut entries: Vec<Entry> = ["src", "tests", "locales"]
            .iter()
            .map(|d| Entry {
                is_dir: true,
                name: (*d).into(),
                path: dir.join(d),
            })
            .collect();
        for i in 0..12 {
            let name = format!("File{i}.wowlua");
            entries.push(Entry {
                is_dir: false,
                path: dir.join(&name),
                name,
            });
        }
        entries
    }
    fn open(&self, path: &Path) {
        if path.ends_with("plain") {
            self.events
                .borrow_mut()
                .push(system::Event::Opened(Opened::NoManifest(
                    path.to_path_buf(),
                )));
            return;
        }
        let member = |name: &str, rel: &str| Member {
            name: name.into(),
            kind: "addon".into(),
            version: "1.0.0".into(),
            dir: path.join(rel),
            rel: rel.into(),
        };
        self.events
            .borrow_mut()
            .push(system::Event::Opened(Opened::Workspace(Workspace {
                root: path.to_path_buf(),
                name: "suuui".into(),
                members: vec![
                    member("suuui", ""),
                    member("foreverlfg", "libs/foreverlfg"),
                    member("hello", "Examples/hello"),
                ],
                flavors: vec!["camelot".into()],
                cannot_check: None,
            })));
    }
    fn read_file(&self, path: &Path) -> Result<String, String> {
        Ok(format!("-- {}\nlocal x = 1\n", path.display()))
    }
    fn write_file(&self, _: &Path, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn highlight(&self, _: &str) -> Vec<system::Highlight> {
        Vec::new()
    }
    fn folding(&self, _: &str) -> Vec<(usize, usize)> {
        Vec::new()
    }
    fn schema(&self, _: bool) -> Option<String> {
        None
    }
    fn canonical(&self, path: &Path) -> PathBuf {
        path.to_path_buf()
    }
    fn lsp_send(&self, _: serde_json::Value) {}
    fn lsp_files(&self, _: Vec<(PathBuf, system::FileChange)>) {}
    fn lsp_stop(&self) {}
    fn search(&self, _: &Path, _: system::Search) {}
    fn terminal_open(&self, _: &Path, _: u16, _: u16) -> Result<u64, String> {
        Err("none".into())
    }
    fn terminal_input(&self, _: u64, _: &[u8]) {}
    fn terminal_resize(&self, _: u64, _: u16, _: u16) {}
    fn terminal_scroll(&self, _: u64, _: i32) {}
    fn terminal_screen(&self, _: u64) -> Option<system::TermScreen> {
        None
    }
    fn terminal_close(&self, _: u64) {}
    fn poll(&self) -> Vec<system::Event> {
        std::mem::take(&mut *self.events.borrow_mut())
    }
    // The rest is the empty system's.
    fn watch(&self, root: Option<&Path>) {
        NoSystem.watch(root)
    }
    fn find_installation(&self) {}
    fn set_installation(&self, _: &Path) {}
    fn check_ui(&self, _: &str) {}
    fn download_ui(&self, _: &str) {}
    fn names(&self, name: &str) -> system::PackageNames {
        NoSystem.names(name)
    }
    fn in_git(&self, _: &Path) -> bool {
        false
    }
    fn new_addon(&self, _: system::NewAddon) {}
    fn adopt_preview(&self, dir: &Path) -> system::AdoptPreview {
        NoSystem.adopt_preview(dir)
    }
    fn adopt(&self, _: system::Adopt) {}
    fn open_window(&self, _: Option<&Path>) {}
    fn whats_new(&self) -> Vec<String> {
        vec!["A line of the changelog.".into(); 3]
    }
    fn branch(&self, _: &Path) -> Option<String> {
        Some("main".into())
    }
}
