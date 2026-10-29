//! The frame budget (decision S014): building a frame's UI and turning it
//! into triangles takes at most 4 ms at the 99th percentile. Checked in
//! optimized builds, one test at a time (`cargo test --release -p
//! wowlua-studio-app --test frame_budget -- --test-threads=1`: tests side
//! by side time each other); unoptimized ones only report the numbers.

mod common;

use std::{
    path::Path,
    time::{Duration, Instant},
};

use egui::{Event, Pos2, RawInput, Rect, Vec2};
use wowlua_studio_app::{commands::Command, pages::Page};

const BUDGET: Duration = Duration::from_millis(4);

#[test]
fn frames_stay_within_the_budget() {
    let ctx = egui::Context::default();
    let (mut studio, _) = common::studio(&ctx);
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
    let mut times = Vec::new();
    for frame in 0..600u32 {
        // The start page first, then a workspace: the pointer crosses the
        // window; pages change; the palette opens.
        if frame == 200 {
            studio.open(Path::new("/p/suuui"));
        }
        // A large file in the editor, from frame 300.
        if frame == 300 {
            studio.edit(Path::new("/p/suuui/src/big.wowlua"));
        }
        if frame < 200 && frame % 50 == 25 {
            studio.queue(if frame % 100 == 25 {
                Command::NewAddon
            } else {
                Command::AdoptAddon
            });
        }
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let x = (frame * 7 % 1440) as f32;
        let y = (frame * 3 % 900) as f32;
        input.events.push(Event::PointerMoved(Pos2::new(x, y)));
        if frame > 200 && frame % 40 == 0 {
            let page = Page::ALL[(frame / 40) as usize % Page::ALL.len()];
            studio.queue(Command::ShowPage(page));
        }
        if frame % 100 == 50 {
            studio.queue(Command::OpenPalette);
        }
        let started = Instant::now();
        let mut output = ctx.run_ui(input, |ui| studio.ui(ui));
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        // No renderer takes the font atlas's updates here.
        output.textures_delta.clear();
        // The first frames build the font atlas.
        if frame >= 20 {
            times.push(started.elapsed());
        }
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "frames: median {:.3} ms · p99 {:.3} ms · slowest {:.3} ms (budget {} ms at p99)",
        ms(at(0.5)),
        ms(at(0.99)),
        ms(at(1.0)),
        ms(BUDGET)
    );
    if !cfg!(debug_assertions) {
        assert!(
            at(0.99) <= BUDGET,
            "the 99th percentile frame took {:.3} ms",
            ms(at(0.99))
        );
    }
}

/// A data file on one 2 MB line: opening it and drawing it with the
/// pointer over it.
#[test]
fn a_file_on_one_long_line_stays_within_the_budget() {
    let ctx = egui::Context::default();
    let (mut studio, _) = common::studio(&ctx);
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
    studio.open(Path::new("/p/suuui"));
    let mut times = Vec::new();
    for frame in 0..120u32 {
        if frame == 10 {
            studio.edit(Path::new("/p/suuui/data/long.json"));
        }
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let x = (300 + frame * 7 % 1000) as f32;
        input.events.push(Event::PointerMoved(Pos2::new(x, 140.0)));
        let started = Instant::now();
        let mut output = ctx.run_ui(input, |ui| studio.ui(ui));
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        output.textures_delta.clear();
        if frame >= 20 {
            times.push(started.elapsed());
        }
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "long line: median {:.3} ms · p99 {:.3} ms · slowest {:.3} ms",
        ms(at(0.5)),
        ms(at(0.99)),
        ms(at(1.0))
    );
    if !cfg!(debug_assertions) {
        assert!(at(0.99) <= BUDGET, "p99 {:.3} ms", ms(at(0.99)));
    }
}

/// The search panel with as many matches as a search keeps, on long
/// lines, scrolled through.
#[test]
fn the_search_panel_stays_within_the_budget() {
    use wowlua_studio_app::search::{self, SearchState, Tab};
    use wowlua_studio_app::system::{Found, Hit, Search};
    let ctx = egui::Context::default();
    // The fonts and the palette.
    let _ = common::studio(&ctx);
    let line = "local x = \"test\" ".repeat(120);
    let files = (0..300)
        .map(|f| {
            let hits = (0..17)
                .map(|i| Hit {
                    line: i,
                    at: (11, 15),
                    text: line.clone(),
                    start: 11,
                    end: 15,
                })
                .collect();
            (Path::new("/p/suuui/src").join(format!("f{f}.wowlua")), hits)
        })
        .collect();
    let query = Search {
        query: "test".to_owned(),
        ..Search::default()
    };
    let mut state = SearchState {
        tab: Tab::Text,
        search: query.clone(),
        found: Some((
            query,
            Found {
                files,
                searched: 300,
                millis: 40,
                error: None,
                cut: true,
            },
        )),
        ..SearchState::default()
    };
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(420.0, 900.0));
    let mut times = Vec::new();
    for frame in 0..300u32 {
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        input.events.push(Event::PointerMoved(Pos2::new(
            200.0,
            (frame * 3 % 900) as f32,
        )));
        input.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        let started = Instant::now();
        let mut output = ctx.run_ui(input, |ui| {
            search::show(ui, &mut state, Path::new("/p/suuui"));
        });
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        output.textures_delta.clear();
        if frame >= 20 {
            times.push(started.elapsed());
        }
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "search panel: median {:.3} ms · p99 {:.3} ms",
        ms(at(0.5)),
        ms(at(0.99))
    );
    if !cfg!(debug_assertions) {
        assert!(at(0.99) <= BUDGET, "p99 {:.3} ms", ms(at(0.99)));
    }
}

/// The tests page with a large suite's results (SuuUI's: 185 files, about
/// 3,700 tests, some failing): the explorer scrolled through beside the
/// results.
#[test]
fn the_tests_page_stays_within_the_budget() {
    use egui::UiBuilder;
    use wowlua_studio_app::lsp::Discovered;
    use wowlua_studio_app::system::{NoSystem, TestCase, TestFile, TestOutcome, Workspace};
    use wowlua_studio_app::tests::{self, Kept, TestsState};
    let ctx = egui::Context::default();
    let _ = common::studio(&ctx);
    let workspace = Workspace {
        root: "/p/suuui".into(),
        name: "SuuUI".into(),
        members: Vec::new(),
        flavors: vec!["retail".into()],
        cannot_check: None,
    };
    let mut state = TestsState::default();
    let mut files = Vec::new();
    let mut results = Vec::new();
    for f in 0..185 {
        let rel = format!("tests/m{}/f{f}.test.wowlua", f % 12);
        let path = workspace.root.join(&rel);
        let names: Vec<String> = (0..20).map(|t| format!("does thing {t}")).collect();
        state.discovered(
            path.clone(),
            names
                .iter()
                .enumerate()
                .map(|(i, n)| Discovered {
                    name: n.clone(),
                    line: i * 6 + 2,
                    start: i * 6 + 1,
                    skip: false,
                    cases: false,
                })
                .collect(),
        );
        results.push(TestFile {
            package: workspace.root.clone(),
            file: rel,
            flavor: "retail".into(),
            tests: names
                .iter()
                .enumerate()
                .map(|(i, n)| TestCase {
                    name: n.clone(),
                    outcome: if (f + i) % 97 == 0 {
                        TestOutcome::Fail {
                            message: "expected 2, got 3".into(),
                            stack: format!(
                                "tests/m0/f{f}.test.wowlua:{}: in function 'fn'",
                                i * 6 + 3
                            ),
                        }
                    } else {
                        TestOutcome::Pass
                    },
                    seconds: 0.002,
                    functions: vec![("src/core.wowlua".into(), 10)],
                })
                .collect(),
            error: None,
            printed: String::new(),
            seconds: 0.3,
        });
        files.push(path);
    }
    state.files = Some(files);
    state.restore(Kept {
        results,
        last: None,
    });
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
    let left = Rect::from_min_max(Pos2::ZERO, Pos2::new(330.0, 900.0));
    let right = Rect::from_min_max(Pos2::new(330.0, 0.0), Pos2::new(1440.0, 900.0));
    let mut times = Vec::new();
    for frame in 0..300u32 {
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        input.events.push(Event::PointerMoved(Pos2::new(
            160.0,
            (frame * 3 % 900) as f32,
        )));
        input.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -300.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        let started = Instant::now();
        let mut output = ctx.run_ui(input, |ui| {
            ui.scope_builder(UiBuilder::new().max_rect(left), |ui| {
                tests::explorer(ui, &mut state, &workspace);
            });
            ui.scope_builder(UiBuilder::new().max_rect(right), |ui| {
                tests::results(ui, &mut state, &workspace, &NoSystem, &[]);
            });
        });
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        output.textures_delta.clear();
        if frame >= 20 {
            times.push(started.elapsed());
        }
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "tests page: median {:.3} ms · p99 {:.3} ms",
        ms(at(0.5)),
        ms(at(0.99))
    );
    if !cfg!(debug_assertions) {
        assert!(at(0.99) <= BUDGET, "p99 {:.3} ms", ms(at(0.99)));
    }
}

/// The game page's frame tree with 5,000 frames, every one matching the
/// filter, and the events panel full (1,000 events): scrolled through.
#[test]
fn the_game_page_stays_within_the_budget() {
    use egui::UiBuilder;
    use serde_json::json;
    use wowlua_studio_app::game::{self, GameState};
    use wowlua_studio_app::system::{GameEvent, NoSystem};
    let ctx = egui::Context::default();
    let _ = common::studio(&ctx);
    let mut state = GameState::default();
    let session = state.begin();
    state.event(
        session,
        GameEvent::Started {
            addon: "SuuUI".into(),
            flavor: "camelot".into(),
            installation: None,
        },
        0.0,
    );
    let tree: Vec<serde_json::Value> = (0..200)
        .map(|i| {
            let children: Vec<serde_json::Value> = (0..24)
                .map(|c| {
                    json!({ "id": 1000 + i * 24 + c, "type": "Frame",
                            "name": format!("SuuUIFrame{i}_{c}"), "shown": true })
                })
                .collect();
            json!({ "id": i, "type": "Frame", "name": format!("SuuUIRoot{i}"),
                    "shown": true, "children": children })
        })
        .collect();
    let events: Vec<serde_json::Value> = (0..1000)
        .map(|i| {
            json!({ "seq": i, "event": "UNIT_HEALTH", "args": ["\"party1\""],
                    "frames": ["SuuUIPartyFrames", "CompactRaidFrame1"], "ms": 0.04 })
        })
        .collect();
    state.query = "suuui".into();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
    let left = Rect::from_min_max(Pos2::ZERO, Pos2::new(330.0, 900.0));
    let bottom = Rect::from_min_max(Pos2::new(330.0, 600.0), Pos2::new(1440.0, 900.0));
    let mut times = Vec::new();
    for frame in 0..300u32 {
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        input.events.push(Event::PointerMoved(Pos2::new(
            160.0,
            (frame * 3 % 900) as f32,
        )));
        input.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -300.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        let started = Instant::now();
        let mut output = ctx.run_ui(input, |ui| {
            ui.scope_builder(UiBuilder::new().max_rect(left), |ui| {
                game::frames(ui, &mut state, &NoSystem);
            });
            ui.scope_builder(UiBuilder::new().max_rect(bottom), |ui| {
                game::events(ui, &mut state, &NoSystem);
            });
        });
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        output.textures_delta.clear();
        if frame == 0 {
            // The first frame asked for the tree (1) and the events (2).
            let reply = |id, result| GameEvent::Reply {
                id,
                result: Ok(result),
            };
            state.event(session, reply(1, json!(tree)), 0.0);
            state.event(session, reply(2, json!(events)), 0.0);
        }
        if frame >= 20 {
            times.push(started.elapsed());
        }
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "game page: median {:.3} ms · p99 {:.3} ms",
        ms(at(0.5)),
        ms(at(0.99))
    );
    if !cfg!(debug_assertions) {
        assert!(at(0.99) <= BUDGET, "p99 {:.3} ms", ms(at(0.99)));
    }
}

/// The world profiles page with a large profile (40 units, 120 class
/// spells, 60 auras): its panels, the units' form and the spells' list.
#[test]
fn the_world_page_stays_within_the_budget() {
    use egui::UiBuilder;
    use serde_json::json;
    use std::collections::BTreeMap;
    use wowlua_studio_app::editor::{Editors, Tab, doc::Document};
    use wowlua_studio_app::system::{ClassSpell, NoSystem, WorldProfile};
    use wowlua_studio_app::world::{self, WorldState};
    let ctx = egui::Context::default();
    let _ = common::studio(&ctx);
    let units: Vec<serde_json::Value> = (0..40)
        .map(|i| {
            json!({ "name": format!("Unit {i}"), "level": 60, "class": "ROGUE",
                    "reaction": "hostile", "distance": i, "nameplate": i % 2 == 0,
                    "group": if i < 4 { Some("party") } else { None } })
        })
        .collect();
    let auras: Vec<serde_json::Value> = (0..60)
        .map(|i| json!({ "spellId": 1000 + i, "name": format!("Aura {i}"), "duration": 10 }))
        .collect();
    let profile = json!({
        "character": { "name": "Player", "class": "WARRIOR", "level": 60 },
        "units": units,
        "auras": auras,
        "trained": (0..60).collect::<Vec<u32>>(),
    });
    let text = serde_json::to_string_pretty(&profile).unwrap();
    let path = Path::new("/w/tests/profiles/raid.json").to_path_buf();
    let mut editors = Editors::default();
    let mut tab = Tab::new(Document::new(path.clone(), &text));
    tab.hidden = true;
    editors.tabs.push(tab);
    let workspace = wowlua_studio_app::system::Workspace {
        root: "/w".into(),
        name: "SuuUI".into(),
        members: Vec::new(),
        flavors: vec!["camelot".into()],
        cannot_check: None,
    };
    let mut state = WorldState::default();
    state.listed(
        vec![WorldProfile {
            name: "raid".into(),
            path,
        }],
        1e9,
    );
    let spells: Vec<ClassSpell> = (0..120)
        .map(|i| ClassSpell {
            id: i,
            name: format!("Spell {i}"),
            rank: "Rank 1".into(),
            level: 1 + i % 60,
            line: ["Arms", "Fury", "Protection"][i as usize % 3].into(),
            icon: None,
        })
        .collect();
    state.class_spells_found(String::new(), Vec::new());
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
    let left = Rect::from_min_max(Pos2::new(64.0, 40.0), Pos2::new(284.0, 870.0));
    let middle = Rect::from_min_max(Pos2::new(284.0, 40.0), Pos2::new(1190.0, 870.0));
    let right = Rect::from_min_max(Pos2::new(1190.0, 40.0), Pos2::new(1440.0, 870.0));
    let diagnostics = BTreeMap::new();
    let mut debug = wowlua_studio_app::debug::DebugState::default();
    let mut times = Vec::new();
    for frame in 0..300u32 {
        // The units' form, then the spells' list.
        state.part = if frame < 150 { "units" } else { "spells" }.into();
        // (the spells part asks for them on its first frame)
        if frame == 152 {
            state.class_spells_found("WARRIOR".into(), spells.clone());
        }
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        input.events.push(Event::PointerMoved(Pos2::new(
            600.0,
            (frame * 3 % 800) as f32,
        )));
        input.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        let started = Instant::now();
        let mut output = ctx.run_ui(input, |ui| {
            ui.scope_builder(UiBuilder::new().max_rect(left), |ui| {
                world::profiles(ui, &mut state, &mut editors, &workspace, &NoSystem);
                world::parts_panel(ui, &mut state, &mut editors, &workspace, &NoSystem);
            });
            ui.scope_builder(UiBuilder::new().max_rect(middle), |ui| {
                world::editor(
                    ui,
                    &mut state,
                    &mut editors,
                    &workspace,
                    &NoSystem,
                    &diagnostics,
                    None,
                );
            });
            ui.scope_builder(UiBuilder::new().max_rect(right), |ui| {
                world::live(ui, &mut state, &mut debug, &workspace, &NoSystem);
            });
        });
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        output.textures_delta.clear();
        if frame >= 20 && !(150..=152).contains(&frame) {
            times.push(started.elapsed());
        }
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "world page: median {:.3} ms · p99 {:.3} ms",
        ms(at(0.5)),
        ms(at(0.99))
    );
    if !cfg!(debug_assertions) {
        assert!(at(0.99) <= BUDGET, "p99 {:.3} ms", ms(at(0.99)));
    }
}

/// The ship page with SuuUI's size: four profiles and their forms, 300
/// files a build, a log of 60 diagnostics, 200 lines of the dev loop, the
/// publish checks and targets; each tab, scrolled through.
#[test]
fn the_ship_page_stays_within_the_budget() {
    use egui::UiBuilder;
    use serde_json::json;
    use std::path::PathBuf;
    use wowlua_studio_app::editor::{Editors, Tab, doc::Document};
    use wowlua_studio_app::ship::{self, ShipState};
    use wowlua_studio_app::system::*;
    let ctx = egui::Context::default();
    let _ = common::studio(&ctx);
    let manifest = json!({
        "package": { "name": "suuui", "version": "0.6.0-beta.2" },
        "profiles": {
            "release": { "minify": "max", "optimize": { "inline": true } },
            "readable": { "inherits": "release", "minify": "none" }
        }
    });
    let text = serde_json::to_string_pretty(&manifest).unwrap();
    let path = PathBuf::from("/w/wowlua.json");
    let mut editors = Editors::default();
    let mut tab = Tab::new(Document::new(path.clone(), &text));
    tab.hidden = true;
    editors.tabs.push(tab);
    let workspace = Workspace {
        root: "/w".into(),
        name: "SuuUI".into(),
        members: Vec::new(),
        flavors: vec!["camelot".into()],
        cannot_check: None,
    };
    let effective = json!({ "bundle": true, "minify": "max", "minify-libraries": true,
        "optimize": { "inline": true, "cache-globals": "safe", "dead-code": true },
        "debug": false, "assertions": false, "log-level": "warn" });
    let profile = |name: &str, chain: &[&str]| BuildProfile {
        name: name.into(),
        builtin: name != "readable",
        chain: chain.iter().map(|s| s.to_string()).collect(),
        effective: effective.clone(),
        problem: None,
    };
    let files: Vec<(String, u64)> = (0..300)
        .map(|i| (format!("Media/Textures/file{i}.tga"), 4_000 + i))
        .chain([
            ("SuuUI.lua".to_owned(), 1_840_000),
            ("SuuUI.toc".to_owned(), 400),
        ])
        .collect();
    let facts = ShipFacts {
        addon: "SuuUI".into(),
        version: "0.6.0-beta.2".into(),
        manifest: path,
        out: "/w/dist".into(),
        profiles: vec![
            profile("release", &["release"]),
            profile("readable", &["release", "readable"]),
            profile("test", &["test"]),
            profile("dev", &["dev"]),
        ],
        outputs: ["release", "readable", "dev"]
            .iter()
            .map(|p| BuiltOutput {
                profile: (*p).into(),
                dir: format!("/w/dist/{p}/SuuUI").into(),
                files: files.clone(),
                modified: Some(1_700_000_000_000),
            })
            .collect(),
        installs: vec![InstallFolder {
            client: "_forever_".into(),
            path: "/wow/_forever_/Interface/AddOns/SuuUI".into(),
            state: InstallState::Copied(Some(
                (0..20)
                    .map(|i| InstallChange {
                        path: format!("SuuUI/file{i}.lua"),
                        kind: ChangeKind::Changed { was: 10, now: 12 },
                        modified: Some(1_700_000_000_000),
                    })
                    .collect(),
            )),
        }],
        targets: ["curseforge", "wago", "github"]
            .iter()
            .map(|k| ShipTarget {
                name: (*k).into(),
                kind: (*k).into(),
                project: "1742945".into(),
                profile: "readable".into(),
                token_env: None,
                enabled: true,
            })
            .collect(),
        release: "beta".into(),
        branch: Some("main".into()),
        remote: Some("origin".into()),
        clean: true,
        tag: "v0.6.0-beta.2".into(),
        changelog: Some((
            "/w/CHANGELOG.md".into(),
            "### New\n- Threat meter: raid pulls.\n".repeat(20),
        )),
        ..ShipFacts::default()
    };
    let mut state = ShipState::default();
    state.event(ShipEvent::Read(Box::new(Ok(facts))));
    let diagnostic = |i: u32| ShipDiagnostic {
        severity: "warning".into(),
        code: Some("W1004".into()),
        message: format!("`data` shadows a local of an enclosing scope ({i})"),
        path: Some("/w/src/UI/GroupFinderListing.wowlua".into()),
        line: Some(i),
        column: Some(33),
    };
    state.event(ShipEvent::Build {
        job: 0,
        event: BuildEvent::Done(BuildDone {
            ok: true,
            profile: "release".into(),
            dir: Some("/w/dist/release/SuuUI".into()),
            ms: 2_100,
            diagnostics: (0..60).map(diagnostic).collect(),
            installed: Vec::new(),
        }),
    });
    for i in 0..200 {
        state.event(ShipEvent::Dev(DevEvent::Built(DevBuild {
            at: 0,
            clock: "10:58:12".into(),
            changed: vec![format!("src/UI/File{i}.wowlua")],
            ok: i % 7 != 0,
            ms: 400,
            diagnostics: vec![diagnostic(i)],
            synced: vec!["/wow/_forever_/Interface/AddOns/SuuUI".into()],
        })));
    }
    state.event(ShipEvent::Tokens(vec![
        ("curseforge".into(), TokenState::Saved),
        ("wago".into(), TokenState::Missing),
        ("github".into(), TokenState::FromEnv("GITHUB_TOKEN".into())),
    ]));
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
    let left = Rect::from_min_max(Pos2::new(64.0, 40.0), Pos2::new(344.0, 870.0));
    let main = Rect::from_min_max(Pos2::new(344.0, 40.0), Pos2::new(1440.0, 870.0));
    let installation = Installation {
        path: Some("/wow".into()),
        clients: Vec::new(),
        problem: None,
    };
    let mut times = Vec::new();
    for frame in 0..300u32 {
        state.tab =
            [ship::Tab::Build, ship::Tab::Install, ship::Tab::Publish][frame as usize / 100];
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        input.events.push(egui::Event::PointerMoved(Pos2::new(
            700.0,
            (frame * 3 % 800) as f32,
        )));
        input.events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        let started = Instant::now();
        let mut output = ctx.run_ui(input, |ui| {
            ui.scope_builder(UiBuilder::new().max_rect(left), |ui| {
                ship::profiles(ui, &mut state, &workspace, &NoSystem);
            });
            ui.scope_builder(UiBuilder::new().max_rect(main), |ui| {
                ship::ship(ui, &mut state, &workspace, &NoSystem, Some(&installation));
            });
        });
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        output.textures_delta.clear();
        if frame % 100 >= 20 {
            times.push(started.elapsed());
        }
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "ship page: median {:.3} ms · p99 {:.3} ms",
        ms(at(0.5)),
        ms(at(0.99))
    );
    if !cfg!(debug_assertions) {
        assert!(at(0.99) <= BUDGET, "p99 {:.3} ms", ms(at(0.99)));
    }
}

/// The settings page's Studio sections, then a wowlua.json tab's form: a
/// section of rows, and the dependencies of a package with 40 of them;
/// each scrolled through.
#[test]
fn the_settings_page_stays_within_the_budget() {
    use egui::UiBuilder;
    use wowlua_studio_app::editor::{Document, Editors, Tab};
    use wowlua_studio_app::manifest::{self, Manifests};
    use wowlua_studio_app::settings::{self, Section, SettingsState};
    use wowlua_studio_app::state::Settings;
    use wowlua_studio_app::system::*;
    let ctx = egui::Context::default();
    let _ = common::studio(&ctx);
    let workspace = Workspace {
        root: "/w".into(),
        name: "SuuUI".into(),
        members: Vec::new(),
        flavors: vec!["camelot".into()],
        cannot_check: None,
    };
    let mut state = SettingsState::default();
    let mut settings = Settings::default();
    let mut manifests = Manifests::default();
    let mut editors = Editors::default();
    let path = std::path::PathBuf::from("/w/wowlua.json");
    editors.tabs.push(Tab::new(Document::new(
        path.clone(),
        r#"{ "package": { "name": "suuui", "version": "0.6.0-beta.1", "wowlua": "^0.13" },
  "test": { "blizzard-ui": true, "platform": "mac", "coverage": { "lines": 98 } } }"#,
    )));
    let rows: Vec<DepRow> = (0..40)
        .map(|i| DepRow {
            name: format!("lib-{i}"),
            dev: i % 5 == 0,
            source: "registry".into(),
            registry: true,
            kind: "library".into(),
            requirement: "^1.2".into(),
            locked: "1.2.1".into(),
            yanked: i == 3,
            description: "A library.".into(),
        })
        .collect();
    manifests.deps.event(DepsEvent::Read(Box::new(Ok(DepsFacts {
        package: "suuui".into(),
        manifest: "/w/wowlua.json".into(),
        rows,
        problems: Vec::new(),
    }))));
    manifests.deps.event(DepsEvent::Outdated(Ok((0..20)
        .map(|i| DepNewer {
            name: format!("lib-{i}"),
            latest: "1.2.3".into(),
            action: "`wowlua update`".into(),
        })
        .collect())));
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
    let left = Rect::from_min_max(Pos2::new(64.0, 40.0), Pos2::new(304.0, 870.0));
    let main = Rect::from_min_max(Pos2::new(304.0, 40.0), Pos2::new(1440.0, 870.0));
    let mut times = Vec::new();
    for frame in 0..300u32 {
        state.section = Section::Debugger;
        match frame {
            100 => manifests.show_section(&path, "test"),
            200 => manifests.show_section(&path, "dependencies"),
            _ => {}
        }
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        input.events.push(egui::Event::PointerMoved(Pos2::new(
            700.0,
            (frame * 3 % 800) as f32,
        )));
        input.events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        let started = Instant::now();
        let mut output = ctx.run_ui(input, |ui| {
            if frame < 100 {
                ui.scope_builder(UiBuilder::new().max_rect(left), |ui| {
                    settings::sections(ui, &mut state);
                });
                ui.scope_builder(UiBuilder::new().max_rect(main), |ui| {
                    let _ = settings::studio(
                        ui,
                        &mut state,
                        &mut settings,
                        &NoSystem,
                        Some(&workspace),
                        None,
                        wowlua_studio_app::Platform::MacOs,
                    );
                });
            } else {
                ui.scope_builder(UiBuilder::new().max_rect(main), |ui| {
                    let _ = manifest::form(
                        ui,
                        &mut manifests,
                        &mut editors,
                        0,
                        &workspace,
                        &NoSystem,
                        "0.13.6",
                        &[],
                    );
                });
            }
        });
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        output.textures_delta.clear();
        if frame % 100 >= 20 {
            times.push(started.elapsed());
        }
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "settings page: median {:.3} ms · p99 {:.3} ms",
        ms(at(0.5)),
        ms(at(0.99))
    );
    if !cfg!(debug_assertions) {
        assert!(at(0.99) <= BUDGET, "p99 {:.3} ms", ms(at(0.99)));
    }
}

/// The profiler page with a large profile (3,000 functions, a tree of
/// 20,000 nodes, 600 frames with their trees): each view, a frame picked,
/// scrolled through.
#[test]
fn the_profiler_page_stays_within_the_budget() {
    use egui::UiBuilder;
    use wowlua_studio_app::profiler::{self, ProfilerState, View};
    use wowlua_studio_app::system::{
        NoSystem, Profile, ProfileFrame, ProfileKey, ProfileNode, Workspace,
    };
    let ctx = egui::Context::default();
    let _ = common::studio(&ctx);
    let w = Workspace {
        root: "/p/suuui".into(),
        name: "SuuUI".into(),
        members: Vec::new(),
        flavors: vec!["camelot".into()],
        cannot_check: None,
    };
    let mut keys = vec![ProfileKey {
        name: "(root)".into(),
        ..ProfileKey::default()
    }];
    for i in 0..3000u32 {
        keys.push(ProfileKey {
            name: format!("function src/Module{}.wowlua:{}", i % 60, i),
            file: Some(format!("src/Module{}.wowlua", i % 60)),
            line: i,
            kind: None,
        });
    }
    // A tree: each node under one a few before it, costs varied.
    let mut nodes = vec![ProfileNode::default()];
    for i in 1..20_000u32 {
        let parent = if i < 40 { 0 } else { i / 3 };
        nodes.push(ProfileNode {
            key: 1 + (i * 7919) % 3000,
            parent,
            calls: u64::from(i % 50 + 1),
            cost: [
                u64::from(i * 31 % 9000),
                u64::from(i % 700),
                u64::from(i * 13),
            ],
        });
    }
    let frames = (0..600u32)
        .map(|f| ProfileFrame {
            name: format!("frame {f}"),
            cost: [u64::from(f % 12 * 1000 + 500), 100, 2000],
            nodes: (0..200u32)
                .map(|j| {
                    let n = (f * 31 + j * 97) % 20_000;
                    (n, (1u64, [u64::from(j * 10), 10, 30]))
                })
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .map(|(n, (c, cost))| (n, c, cost))
                .collect(),
        })
        .collect();
    let profile = Profile {
        name: "SuuUI · addon".into(),
        keys,
        nodes,
        frames,
    };
    let mut state = ProfilerState::default();
    state.open_profile(&profile, None);
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
    let main = Rect::from_min_max(Pos2::new(64.0, 40.0), Pos2::new(1110.0, 874.0));
    let side = Rect::from_min_max(Pos2::new(1110.0, 40.0), Pos2::new(1440.0, 874.0));
    let views = [View::Functions, View::Tree, View::Flame, View::Dispatches];
    let mut times = Vec::new();
    for frame in 0..400u32 {
        if frame % 50 == 0 {
            let picked = (frame % 100 == 50).then_some((frame as usize * 7) % 600);
            state.set_view(views[(frame / 50) as usize % views.len()], picked);
        }
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        input.events.push(Event::PointerMoved(Pos2::new(
            300.0 + (frame * 5 % 700) as f32,
            200.0 + (frame * 3 % 600) as f32,
        )));
        input.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -200.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        let started = Instant::now();
        let mut output = ctx.run_ui(input, |ui| {
            ui.scope_builder(UiBuilder::new().max_rect(main), |ui| {
                profiler::show(ui, &mut state, &w, &NoSystem, true);
            });
            ui.scope_builder(UiBuilder::new().max_rect(side), |ui| {
                let p = wowlua_studio_ui::palette(ui.ctx());
                profiler::function(ui, &mut state, p);
            });
        });
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        output.textures_delta.clear();
        if frame >= 20 {
            times.push(started.elapsed());
        }
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "profiler page: median {:.3} ms · p99 {:.3} ms",
        ms(at(0.5)),
        ms(at(0.99))
    );
    if !cfg!(debug_assertions) {
        assert!(at(0.99) <= BUDGET, "p99 {:.3} ms", ms(at(0.99)));
    }
}

/// The debugger page's console full (2,000 lines) and 300 breakpoints,
/// the session running: scrolled through.
#[test]
fn the_debugger_page_stays_within_the_budget() {
    use egui::UiBuilder;
    use wowlua_studio_app::debug::{self, Config, DebugState, Kept};
    use wowlua_studio_app::system::{GameEvent, NoSystem, Workspace};
    let ctx = egui::Context::default();
    let _ = common::studio(&ctx);
    let w = Workspace {
        root: "/p/suuui".into(),
        name: "SuuUI".into(),
        members: Vec::new(),
        flavors: vec!["camelot".into()],
        cannot_check: None,
    };
    let mut state = DebugState::default();
    state.restore(Kept::default());
    for f in 0..30 {
        let file = w.root.join(format!("src/Module{f}.wowlua"));
        for line in 1..=10 {
            state.toggle(&NoSystem, &w, &file, line * 7);
        }
    }
    let _ = state.begin(1, Config::addon("SuuUI"), &w);
    for i in 0..2000 {
        state.event(
            1,
            &GameEvent::Output {
                category: "stdout".into(),
                text: format!("SuuUI: line {i} of the session's output, as the chat shows it\n"),
            },
            &NoSystem,
            &w,
        );
    }
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
    let left = Rect::from_min_max(Pos2::ZERO, Pos2::new(330.0, 900.0));
    let bottom = Rect::from_min_max(Pos2::new(330.0, 600.0), Pos2::new(1440.0, 900.0));
    let mut times = Vec::new();
    for frame in 0..300u32 {
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        input.events.push(Event::PointerMoved(Pos2::new(
            160.0,
            (frame * 3 % 900) as f32,
        )));
        input.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -300.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        let started = Instant::now();
        let mut output = ctx.run_ui(input, |ui| {
            ui.scope_builder(UiBuilder::new().max_rect(left), |ui| {
                debug::breakpoints(ui, &mut state, &w, &NoSystem);
            });
            ui.scope_builder(UiBuilder::new().max_rect(bottom), |ui| {
                debug::console(ui, &mut state, &NoSystem);
            });
        });
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        output.textures_delta.clear();
        if frame >= 20 {
            times.push(started.elapsed());
        }
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "debugger page: median {:.3} ms · p99 {:.3} ms",
        ms(at(0.5)),
        ms(at(0.99))
    );
    if !cfg!(debug_assertions) {
        assert!(at(0.99) <= BUDGET, "p99 {:.3} ms", ms(at(0.99)));
    }
}
