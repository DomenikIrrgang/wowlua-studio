//! The game page's session (S055): the workspace's addon run by wowlua's
//! debugger on a thread of its own (`run_hosted`, wowlua decision T484),
//! whose frame hook hands each game frame's scene to the UI thread; there
//! wowlua's renderer draws it into a texture on eframe's own GPU device,
//! which egui shows. Requests go to the session as the debugger's, their
//! answers come back as [`GameEvent`]s.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use serde_json::{Value as Json, json};
use wowlua_debug::protocol::{self, Event as DebugEvent, Request, Response};
use wowlua_debug::session::{self, ChannelLink, ClockMode, Options, Outgoing, Target};
use wowlua_driver::Session;
use wowlua_driver::project::{LoadOptions, load_projects};
use wowlua_manifest::model::PackageKind;
use wowlua_render::client::ClientAssets;
use wowlua_studio_app::system::{GameEvent, GameInput, GameRequest, GameScreen};
use wowlua_test::runs::{self, Switches};

use crate::tests::first_error;

/// The scene the session drew last, and the installation's art to draw
/// it with: between the session's thread and the UI's.
#[derive(Default)]
pub struct Frames {
    scene: Option<wowlua_engine::scene::Scene>,
    /// Scenes the session handed over.
    count: u64,
    assets: Option<ClientAssets>,
}

/// The running session, as the UI thread holds it.
pub struct Running {
    tx: Sender<(u64, Request)>,
    interrupt: Arc<AtomicBool>,
    /// Screenshots asked: where each request's image goes.
    shots: Arc<Mutex<Vec<(u64, PathBuf)>>>,
}

impl Running {
    /// Asks the session; its answer comes as a reply with `id`.
    pub fn ask(&self, id: u64, request: GameRequest) {
        if let GameRequest::Screenshot { to, .. } = &request
            && let Ok(mut shots) = self.shots.lock()
        {
            shots.push((id, to.clone()));
        }
        let _ = self.tx.send((id, to_debug(request)));
        self.interrupt.store(true, Ordering::Relaxed);
    }

    /// Ends the session.
    pub fn stop(&self) {
        let _ = self.tx.send((u64::MAX, Request::Terminate));
        self.interrupt.store(true, Ordering::Relaxed);
    }
}

/// The page's request as the debugger's.
fn to_debug(request: GameRequest) -> Request {
    match request {
        GameRequest::FrameTree { hidden } => Request::FrameTree { root: None, hidden },
        GameRequest::Inspect(frame) => Request::Inspect { frame },
        GameRequest::Pick { x, y } => Request::Pick { x, y },
        GameRequest::Units => Request::Units,
        GameRequest::Target(unit) => Request::Target { unit },
        GameRequest::Events => Request::Events,
        GameRequest::SetScreen {
            width,
            height,
            ui_scale,
        } => Request::SetScreen {
            width,
            height,
            ui_scale,
        },
        GameRequest::Screenshot { frame, .. } => Request::Screenshot {
            frame: frame.map(|f| format!("#{f}")),
            scale: None,
        },
        GameRequest::Input(input) => Request::Input {
            input: match input {
                GameInput::Click { x, y, right } => protocol::Input::Click {
                    frame: None,
                    x: Some(x),
                    y: Some(y),
                    button: if right { "RightButton" } else { "LeftButton" }.into(),
                },
                GameInput::Hover { x, y } => protocol::Input::Hover {
                    frame: None,
                    x: Some(x),
                    y: Some(y),
                },
                GameInput::Drag { x, y, dx, dy } => protocol::Input::DragAt { x, y, dx, dy },
                GameInput::Type(text) => protocol::Input::Type { text },
                GameInput::Key(key) => protocol::Input::Key { key },
                GameInput::Slash(text) => protocol::Input::Slash { text },
            },
        },
    }
}

/// Starts the workspace's addon: its news to `tell`, its scenes to
/// `frames` (`wake` asks for the UI's next frame).
pub fn start(
    root: PathBuf,
    profile: Option<String>,
    frames: Arc<Mutex<Frames>>,
    tell: impl Fn(GameEvent) + Send + Sync + Clone + 'static,
    wake: impl Fn() + Send + Sync + 'static,
) -> Running {
    let (tx, rx) = channel();
    let (out_tx, out_rx) = channel::<Outgoing>();
    let interrupt = Arc::new(AtomicBool::new(false));
    let shots: Arc<Mutex<Vec<(u64, PathBuf)>>> = Arc::default();
    let package_root: Arc<Mutex<PathBuf>> = Arc::new(Mutex::new(root.clone()));
    // The answers, as the page reads them.
    {
        let tell = tell.clone();
        let shots = shots.clone();
        let package_root = package_root.clone();
        std::thread::spawn(move || {
            for out in out_rx {
                match out {
                    Outgoing::Response(id, _) if id >= u64::MAX - 16 => {}
                    Outgoing::Response(id, r) => {
                        let to = shots.lock().ok().and_then(|mut s| {
                            let at = s.iter().position(|(i, _)| *i == id)?;
                            Some(s.remove(at).1)
                        });
                        let root = package_root.lock().map(|r| r.clone()).unwrap_or_default();
                        tell(GameEvent::Reply {
                            id,
                            result: answer(r, to.as_deref(), &root),
                        });
                    }
                    Outgoing::Event(DebugEvent::Output { line }) => {
                        tell(GameEvent::Output {
                            category: line.category,
                            text: line.text,
                        });
                    }
                    Outgoing::Event(DebugEvent::Terminated { summary, failed }) => {
                        tell(GameEvent::Ended {
                            error: failed.then(|| summary.unwrap_or_default()),
                        });
                    }
                    Outgoing::Event(_) => {}
                }
            }
        });
    }
    let link_interrupt = interrupt.clone();
    let session_tx = tx.clone();
    std::thread::Builder::new()
        .name("wowlua session".into())
        .stack_size(64 << 20)
        .spawn(move || {
            let error = run(
                &root,
                profile,
                frames,
                &tell,
                Arc::new(wake),
                &package_root,
                ChannelLink::new(rx, out_tx),
                link_interrupt,
                &session_tx,
            );
            if let Some(error) = error {
                tell(GameEvent::Ended { error: Some(error) });
            }
        })
        .expect("a thread for the game session");
    Running {
        tx,
        interrupt,
        shots,
    }
}

/// A debugger's answer as the page's: data, nothing, or a screenshot
/// written to `to` (from the package's root).
fn answer(r: Result<Response, String>, to: Option<&Path>, root: &Path) -> Result<Json, String> {
    match r? {
        Response::Data { data } => Ok(data),
        Response::Image { png_base64, .. } => {
            let Some(to) = to else {
                return Ok(Json::Null);
            };
            let path = root.join(to);
            let bytes = base64_decode(&png_base64).ok_or("the screenshot is not base64")?;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(json!({ "path": path }))
        }
        _ => Ok(Json::Null),
    }
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let bytes: Vec<u8> = text
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= value(c)? << (18 - 6 * i);
        }
        let take = chunk.len().saturating_sub(1);
        for i in 0..take {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

/// Loads, builds and runs the addon (on the session's thread); the error
/// that kept it from running.
#[allow(clippy::too_many_arguments)]
fn run(
    root: &Path,
    profile: Option<String>,
    frames: Arc<Mutex<Frames>>,
    tell: &(dyn Fn(GameEvent) + Send + Sync),
    wake: Arc<dyn Fn() + Send + Sync>,
    package_root: &Mutex<PathBuf>,
    link: ChannelLink,
    interrupt: Arc<AtomicBool>,
    tx: &Sender<(u64, Request)>,
) -> Option<String> {
    tell(GameEvent::Status("loading the workspace".into()));
    let mut driver = Session::new();
    let options = LoadOptions {
        cwd: root.to_path_buf(),
        lock_in_memory: true,
        offline: true,
        all_members: true,
        ..LoadOptions::default()
    };
    let Some(projects) = load_projects(&mut driver, &options) else {
        return Some(first_error(&driver).unwrap_or_else(|| "the workspace does not load".into()));
    };
    // The addon: the workspace's root package's, else its first addon.
    let Some(project) = projects
        .iter()
        .find(|p| matches!(p.kind(), PackageKind::Addon))
        .or_else(|| projects.first())
    else {
        return Some("the workspace has no addon".into());
    };
    if let Ok(mut r) = package_root.lock() {
        *r = project.root().to_path_buf();
    }
    let Some(flavor) = project.manifest.flavors.first().cloned() else {
        return Some(format!("{} has no flavor", project.addon_name()));
    };
    tell(GameEvent::Status(format!(
        "building {} for {flavor}",
        project.addon_name()
    )));
    let built = runs::build(
        &mut driver,
        project,
        vec![flavor.clone()],
        &flavor,
        "test",
        wowlua_driver::parallel::Jobs::resolve(None),
        &mut |_| {},
    );
    if let Some(error) = first_error(&driver) {
        return Some(error);
    }
    let Some((build, _)) = built else {
        return Some(format!("{} does not build", project.addon_name()));
    };
    let test_settings = &project.manifest.manifest.test;
    let blizzard_ui = if test_settings.blizzard_ui {
        let fetching = |what: &str| tell(GameEvent::Status(format!("fetching {what}")));
        match wowlua_test::ui_source(&flavor, &fetching) {
            Ok(ui) => Some(ui),
            Err(e) => return Some(format!("test.blizzard-ui: {e}")),
        }
    } else {
        None
    };
    // The installation's art, read once a Studio start (decision S055: in
    // memory, as wowlua's camera keeps it).
    tell(GameEvent::Status("reading the installation's art".into()));
    let folders: Vec<String> = wowlua_flavors::table()
        .clients_of(&flavor)
        .flat_map(|c| c.folders.iter().cloned())
        .collect();
    let install = wowlua_driver::install::find_wow(None).ok();
    let casc = install
        .as_ref()
        .and_then(|i| wowlua_render::client::open_client(i, &folders).ok());
    let installation = casc
        .as_ref()
        .and(install.as_ref())
        .map(|i| i.display().to_string());
    let mut addons = vec![(build.addon_name.clone(), build.dir.clone())];
    for a in &test_settings.addons {
        let dir = project.root().join(a);
        if let Some(name) = dir.file_name() {
            addons.push((name.to_string_lossy().into_owned(), dir.clone()));
        }
    }
    if let Ok(mut f) = frames.lock() {
        f.assets = Some(ClientAssets::new(casc, &addons));
    }
    let camera = wowlua_test::camera::camera(project, &build, &flavor);
    // Each client made (the first, a `/reload`'s) under the debugger, its
    // events logged from the start.
    let instrument: &(dyn Fn(&mut wowlua_vm::Vm) + Sync) = &|vm: &mut wowlua_vm::Vm| {
        session::instrument(vm);
        let started = std::time::Instant::now();
        let clock: std::rc::Rc<dyn Fn() -> u64> =
            std::rc::Rc::new(move || started.elapsed().as_nanos() as u64);
        wowlua_engine::eventlog::start(vm, Some(clock));
    };
    let mut suite = runs::suite(
        project,
        &build,
        &flavor,
        blizzard_ui,
        Switches::default(),
        Some(instrument),
        camera.clone(),
    );
    if let Some(p) = profile {
        suite.profile = Some(p);
    }
    let screenshot: Option<wowlua_debug::controller::Screenshotter> = camera.map(|camera| {
        let shoot: wowlua_debug::controller::Screenshotter = Box::new(
            move |vm: &mut wowlua_vm::Vm, frame: Option<&str>, scale: Option<f64>| {
                let id = match frame {
                    Some(f) => match f.strip_prefix('#').and_then(|n| n.parse::<usize>().ok()) {
                        Some(n) => Some(wowlua_engine::widgets::WidgetId(n)),
                        None => Some(
                            wowlua_debug::ui::widget_named(vm, f)
                                .ok_or_else(|| format!("no frame {f}"))?,
                        ),
                    },
                    None => None,
                };
                let png = camera.shoot(vm, id, scale.unwrap_or(1.0))?;
                let img = wowlua_render::decode_png(&png)?;
                Ok((png, img.width, img.height))
            },
        );
        shoot
    });
    let opts = Options {
        target: Target::Addon,
        clock: ClockMode::Realtime,
        stop_on_entry: false,
        debug_blizzard_ui: false,
        flavor: Some(flavor.clone()),
        profile: None,
    };
    let hook: session::FrameHook = Box::new(move |vm| {
        let scene = wowlua_engine::scene::build(vm);
        if let Ok(mut f) = frames.lock() {
            f.scene = Some(scene);
            f.count += 1;
        }
        wake();
    });
    // The session starts at once.
    let _ = tx.send((u64::MAX - 1, Request::ConfigurationDone));
    tell(GameEvent::Started {
        addon: project.addon_name(),
        flavor,
        installation,
    });
    session::run_hosted(
        &suite,
        &opts,
        Box::new(link),
        interrupt,
        screenshot,
        Some(hook),
    );
    None
}

// ------------------------------------------------------------------ screen

/// The game screen on the UI thread: wowlua's renderer on eframe's device,
/// drawing into a texture egui shows.
pub struct Screen {
    gpu: egui_wgpu::RenderState,
    renderer: Option<wowlua_render::Renderer>,
    target: Option<(wgpu::Texture, egui::TextureId, [u32; 2])>,
    drawn: u64,
}

impl Screen {
    pub fn new(gpu: egui_wgpu::RenderState) -> Screen {
        Screen {
            gpu,
            renderer: None,
            target: None,
            drawn: 0,
        }
    }

    /// Draws the session's newest scene, if it has one not drawn yet; the
    /// screen as it is.
    pub fn show(&mut self, frames: &Mutex<Frames>) -> Option<GameScreen> {
        let (scene, count, mut assets) = {
            let mut f = frames.lock().ok()?;
            if f.count == self.drawn {
                let (_, id, size) = self.target.as_ref()?;
                return Some(GameScreen {
                    texture: *id,
                    size: *size,
                    drawn: self.drawn,
                });
            }
            (f.scene.take()?, f.count, f.assets.clone()?)
        };
        let size = [scene.width.max(1), scene.height.max(1)];
        if self.target.as_ref().is_none_or(|(_, _, s)| *s != size) {
            let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("game screen"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wowlua_render::FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut egui_renderer = self.gpu.renderer.write();
            let id = match &self.target {
                Some((_, id, _)) => {
                    egui_renderer.update_egui_texture_from_wgpu_texture(
                        &self.gpu.device,
                        &view,
                        wgpu::FilterMode::Linear,
                        *id,
                    );
                    *id
                }
                None => egui_renderer.register_native_texture(
                    &self.gpu.device,
                    &view,
                    wgpu::FilterMode::Linear,
                ),
            };
            self.target = Some((texture, id, size));
        }
        let renderer = self.renderer.get_or_insert_with(|| {
            wowlua_render::Renderer::with_device(
                self.gpu.device.clone(),
                self.gpu.queue.clone(),
                self.gpu.adapter.get_info().name,
            )
        });
        let (texture, id, _) = self.target.as_ref()?;
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let encoder = renderer.draw(&scene, &view, size[0], size[1], 1.0, &mut assets);
        self.gpu.queue.submit([encoder.finish()]);
        self.drawn = count;
        Some(GameScreen {
            texture: *id,
            size,
            drawn: count,
        })
    }
}
