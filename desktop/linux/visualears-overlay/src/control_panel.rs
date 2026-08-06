use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
#[cfg(all(unix, not(target_os = "macos")))]
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use tao::{
    dpi::{LogicalPosition, LogicalSize},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::{Icon, WindowBuilder},
};
#[cfg(target_os = "windows")]
use wry::WebContext;
use wry::{http::Request, WebViewBuilder};

#[derive(Clone, Copy, PartialEq, Eq)]
enum OverlayChildKind {
    /// live warm model: hidden at launch, shown/hidden via stdin, never torn down between stops
    Standby,
    /// live overlay without standby steering (Linux GTK path)
    Live,
    /// fire-and-forget sample overlay (no model load, no readiness tracking)
    Demo,
}

/// Rendered child tracked by the control panel. For `Standby` children the process stays alive
/// (model stays loaded) across stop/start; `show`/`hide`/`quit` are written to its stdin.
struct OverlayChild {
    child: Child,
    stdin: std::process::ChildStdin,
    ready: bool,
    want_visible: bool,
    argv: Vec<String>,
    kind: OverlayChildKind,
}

#[cfg(all(unix, not(target_os = "macos")))]
const LOGO_PNG: &[u8] = include_bytes!("../assets/visualears-logo.png");
const PANEL_LOGO_PNG: &[u8] = include_bytes!("../assets/shenava-panel-logo.png");

#[derive(Clone)]
pub struct ControlArgs {
    pub model_key: String,
    pub model_path: String,
    pub tokens_path: String,
    pub mel_path: String,
    pub hotwords_path: Option<String>,
}

#[derive(Debug)]
enum UserEvent {
    Status(String, bool),
    Running(bool),
    AudioLevel(f64),
    ModelUnavailable(String),
    /// the warm model child finished loading (unlocks the panel action buttons)
    ModelReady,
    ToggleAt(i32, i32),
    Hide,
    Drag,
    Quit,
}

#[cfg(all(unix, not(target_os = "macos")))]
struct ShenavaTray {
    proxy: EventLoopProxy<UserEvent>,
}

#[cfg(all(unix, not(target_os = "macos")))]
impl ksni::Tray for ShenavaTray {
    const MENU_ON_ACTIVATE: bool = false;

    fn id(&self) -> String {
        "shenava".into()
    }

    fn title(&self) -> String {
        "شنوا".into()
    }

    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        static ICON: LazyLock<ksni::Icon> = LazyLock::new(|| {
            let source = image::load_from_memory_with_format(LOGO_PNG, image::ImageFormat::Png)
                .expect("embedded Shenava logo must be a valid PNG")
                .into_rgba8();
            let mut data =
                image::imageops::resize(&source, 32, 32, image::imageops::FilterType::Lanczos3)
                    .into_vec();
            for pixel in data.chunks_exact_mut(4) {
                pixel.rotate_right(1); // RGBA to the ARGB byte order required by SNI.
            }
            ksni::Icon {
                width: 32,
                height: 32,
                data,
            }
        });
        vec![ICON.clone()]
    }

    fn activate(&mut self, x: i32, y: i32) {
        eprintln!("[tray] activate anchor=({x},{y})");
        let _ = self.proxy.send_event(UserEvent::ToggleAt(x, y));
    }

    // Intentionally inherit the empty menu. Left click is a direct activation and there is no
    // Shenava submenu/list between the status icon and the panel.
}

pub fn run(args: ControlArgs) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    // GNOME Shell owns the visible top-bar button. The control panel stays alive as an
    // accessory surface and is shown only when that button asks for it. Windows does not yet
    // have a tray implementation, so its control panel must be reachable from launch.
    let starts_hidden = !cfg!(target_os = "windows");
    let window = WindowBuilder::new()
        .with_title("Shenava")
        .with_window_icon(window_icon())
        .with_inner_size(LogicalSize::new(340.0, 424.0))
        .with_decorations(false)
        .with_always_on_top(true)
        .with_visible(!starts_hidden)
        .with_resizable(false)
        .build(&event_loop)?;
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use tao::platform::unix::WindowExtUnix;
        let _ = window.set_skip_taskbar(true);
    }
    if !starts_hidden {
        position_panel_near_top_bar(&window, None);
    }

    let child: Arc<Mutex<Option<OverlayChild>>> = Arc::new(Mutex::new(None));
    let proxy = event_loop.create_proxy();
    // Release packages carry a pinned, hash-checked model. Never replace it at runtime from a
    // mutable network URL (and never try to write beside an installed executable in Program Files).
    let model_path = std::path::Path::new(&args.model_path);
    let missing = !model_path.exists()
        || std::fs::metadata(model_path)
            .map(|m| m.len() == 0)
            .unwrap_or(true);
    if missing {
        let _ = proxy.send_event(UserEvent::ModelUnavailable(format!(
            "فایل مدل پیدا نشد؛ شنوا را دوباره نصب کنید. ({})",
            args.model_path
        )));
    } else {
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        warm_spawn(&args, &child, &proxy);
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let _ = proxy.send_event(UserEvent::ModelReady);
            let _ = proxy.send_event(UserEvent::Status("آماده برای شروع".into(), true));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    let tray_handle = {
        use ksni::blocking::TrayMethods;
        let tray = ShenavaTray {
            proxy: proxy.clone(),
        };
        Some(if std::path::Path::new("/.flatpak-info").exists() {
            // Flatpak blocks arbitrary per-process well-known D-Bus names. Register through the
            // connection's unique bus name, as allowed by the StatusNotifierItem protocol.
            tray.disable_dbus_name(true).spawn()?
        } else {
            tray.spawn()?
        })
    };
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let tray_handle: Option<()> = None;
    let handler_args = args.clone();
    let handler_child = child.clone();
    let handler_proxy = proxy.clone();
    let handler = move |req: Request<String>| {
        let body = req.body().as_str();
        if let Err(err) = handle_ipc(body, &handler_args, &handler_child, &handler_proxy) {
            let _ = handler_proxy.send_event(UserEvent::Running(false));
            let _ = handler_proxy.send_event(UserEvent::Status(format!("خطا: {err}"), false));
        }
    };

    #[cfg(target_os = "windows")]
    let mut web_context = {
        let data_dir = std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Shenava")
            .join("WebView2");
        std::fs::create_dir_all(&data_dir)?;
        WebContext::new(Some(data_dir))
    };

    // WebView2's implicit profile defaults beside the executable. That location is not writable
    // after a normal per-machine install under Program Files, causing environment creation to fail
    // with E_UNEXPECTED (0x8000FFFF). Keep the Windows profile under the current user's AppData.
    #[cfg(target_os = "windows")]
    let builder = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_html(panel_html())
        .with_ipc_handler(handler)
        .with_accept_first_mouse(true);
    #[cfg(not(target_os = "windows"))]
    let builder = WebViewBuilder::new()
        .with_html(panel_html())
        .with_ipc_handler(handler)
        .with_accept_first_mouse(true);

    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    ))]
    let webview = builder.build(&window)?;
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    )))]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window
            .default_vbox()
            .ok_or("GTK vbox unavailable for webview")?;
        builder.build_gtk(vbox)?
    };

    #[cfg(target_os = "windows")]
    {
        // Belt-and-suspenders for Windows: the tao window can be created without ever mapping
        // on screen (the panel "doesn't show up" report). Re-assert visibility + focus once the
        // WebView2 surface is attached so the panel is guaranteed to appear at launch.
        window.set_visible(true);
        window.set_focus();
        eprintln!(
            "[control] panel window visible={} size={:?}",
            window.is_visible(),
            window.outer_size()
        );
    }

    eprintln!("[control] Shenava control panel ready");
    let mut webview = Some(webview);
    event_loop.run(move |event, _, control_flow| {
        // The service handle owns the menu-less StatusNotifier item for this process.
        let _keep_tray_alive = &tray_handle;
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(UserEvent::Quit) => {
                eprintln!("[control] CloseRequested/Quit -> exiting");
                stop_child(&child);
                let _ = webview.take();
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::Status(message, ok)) => {
                if let Some(webview) = webview.as_ref() {
                    let js = format!(
                        "window.__shenavaSetStatus({}, {});",
                        serde_json::to_string(&message).unwrap_or_else(|_| "\"\"".to_string()),
                        if ok { "true" } else { "false" }
                    );
                    let _ = webview.evaluate_script(&js);
                }
            }
            Event::UserEvent(UserEvent::AudioLevel(level)) => {
                if let Some(webview) = webview.as_ref() {
                    let js = format!("window.__shenavaAudioLevel({level:.5});");
                    let _ = webview.evaluate_script(&js);
                }
            }
            Event::UserEvent(UserEvent::Running(running)) => {
                if let Some(webview) = webview.as_ref() {
                    let js = format!("window.__shenavaSetRunning({running});");
                    let _ = webview.evaluate_script(&js);
                }
            }
            Event::UserEvent(UserEvent::ModelUnavailable(message)) => {
                if let Some(webview) = webview.as_ref() {
                    let js = format!(
                        "window.__shenavaModelUnavailable({});",
                        serde_json::to_string(&message).unwrap_or_else(|_| "\"\"".to_string())
                    );
                    let _ = webview.evaluate_script(&js);
                }
            }
            Event::UserEvent(UserEvent::ModelReady) => {
                if let Some(webview) = webview.as_ref() {
                    let _ = webview.evaluate_script("window.__shenavaModelReady();");
                }
            }
            Event::UserEvent(UserEvent::ToggleAt(x, y)) => {
                if window.is_visible() {
                    window.set_visible(false);
                } else {
                    position_panel_at_status_click(&window, x, y);
                    window.set_visible(true);
                    window.set_focus();
                }
            }
            Event::UserEvent(UserEvent::Hide) => collapse_panel(&window),
            Event::UserEvent(UserEvent::Drag) => {
                if let Err(e) = window.drag_window() {
                    eprintln!("[control] drag_window error: {e:?}");
                }
            }
            _ => {}
        }
    });
    #[allow(unreachable_code)]
    Ok(())
}

fn collapse_panel(window: &tao::window::Window) {
    #[cfg(target_os = "windows")]
    {
        // With no Windows tray icon, hiding the window would leave the process running with no
        // way to reopen its control surface. Minimizing keeps it recoverable from the taskbar.
        window.set_minimized(true);
    }
    #[cfg(not(target_os = "windows"))]
    window.set_visible(false);
}

fn position_panel_at_status_click(window: &tao::window::Window, click_x: i32, click_y: i32) {
    eprintln!("[tray] positioning from anchor=({click_x},{click_y})");
    if click_x > 0 && click_y >= 0 {
        // The notifier protocol supplies the click in screen coordinates. Align the panel's
        // upper-right corner below that point, just like the macOS NSPopover anchor.
        let anchor = (click_x as f64 - 8.0, click_y as f64 - 8.0, 16.0, 16.0);
        position_panel_near_top_bar(window, Some(anchor));
    } else {
        position_panel_near_top_bar(window, None);
    }
}

fn position_panel_near_top_bar(window: &tao::window::Window, anchor: Option<(f64, f64, f64, f64)>) {
    if let Some((x, y, w, h)) = anchor {
        let panel_w = 340.0;
        let px = x + w - panel_w;
        let py = y + h + 6.0;
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use gtk::prelude::MonitorExt;
            let mut px = px;
            let mut py = py;
            let panel_h = 424.0;
            if let Some(display) = gtk::gdk::Display::default() {
                if let Some(monitor) = display.primary_monitor().or_else(|| display.monitor(0)) {
                    let geometry = monitor.geometry();
                    let min_x = geometry.x() as f64 + 6.0;
                    let max_x = geometry.x() as f64 + geometry.width() as f64 - panel_w - 6.0;
                    let min_y = geometry.y() as f64 + 6.0;
                    let max_y = geometry.y() as f64 + geometry.height() as f64 - panel_h - 6.0;
                    px = px.clamp(min_x, max_x.max(min_x));
                    py = py.clamp(min_y, max_y.max(min_y));
                }
            }
            window.set_outer_position(LogicalPosition::new(px, py));
            return;
        }
        window.set_outer_position(LogicalPosition::new(px, py));
        return;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use gtk::prelude::MonitorExt;
        if let Some(display) = gtk::gdk::Display::default() {
            if let Some(monitor) = display.primary_monitor().or_else(|| display.monitor(0)) {
                let geometry = monitor.geometry();
                let x = geometry.x() + geometry.width() - 340 - 18;
                let y = geometry.y() + 32;
                window.set_outer_position(LogicalPosition::new(x.max(0) as f64, y.max(0) as f64));
                return;
            }
        }
    }
    #[allow(unreachable_code)]
    window.set_outer_position(LogicalPosition::new(18.0, 32.0));
}

fn handle_ipc(
    body: &str,
    args: &ControlArgs,
    child: &Arc<Mutex<Option<OverlayChild>>>,
    proxy: &EventLoopProxy<UserEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    let value: Value = serde_json::from_str(body)?;
    let cmd = value.get("cmd").and_then(Value::as_str).unwrap_or("");
    match cmd {
        "start" => {
            let _ = proxy.send_event(UserEvent::Running(true));
            let style = style_args(&value);
            let device = value
                .get("device")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("system");
            let overlay_kind = if cfg!(all(unix, not(target_os = "macos"))) {
                OverlayChildKind::Live
            } else {
                OverlayChildKind::Standby
            };
            let overlay_mode = if overlay_kind == OverlayChildKind::Live {
                "--overlay"
            } else {
                "--overlay-standby"
            };
            let mut argv = vec![
                overlay_mode.to_string(),
                args.model_key.clone(),
                args.model_path.clone(),
                args.tokens_path.clone(),
                args.mel_path.clone(),
            ];
            if let Some(hotwords) = &args.hotwords_path {
                argv.extend(["--hotwords".to_string(), hotwords.clone()]);
            }
            argv.extend([
                "--device".to_string(),
                map_device_choice(device).to_string(),
            ]);
            argv.extend(style);
            let mut guard = child.lock().map_err(|_| "overlay child lock poisoned")?;
            let reuse = match guard.as_mut() {
                Some(oc) => {
                    oc.kind == OverlayChildKind::Standby
                        && oc.argv == argv
                        && oc.child.try_wait().map_err(|_| "child wait")?.is_none()
                }
                None => false,
            };
            if reuse {
                let oc = guard.as_mut().expect("checked above");
                oc.want_visible = true;
                if oc.ready {
                    let _ = oc.stdin.write_all(b"show\n");
                    let _ = oc.stdin.flush();
                    let waiting = if device == "system" {
                        "در انتظار صدای سیستم…"
                    } else {
                        "در انتظار صدا…"
                    };
                    let _ = proxy.send_event(UserEvent::Status(waiting.into(), true));
                } else {
                    let _ = proxy.send_event(UserEvent::Status("در حال پردازش مدل…".into(), true));
                }
            } else {
                if let Some(mut old) = guard.take() {
                    let _ = old.child.kill();
                    let _ = old.child.wait();
                }
                drop(guard);
                let (spawned, stdin) =
                    spawn_overlay_child(argv.clone(), proxy.clone(), child.clone())?;
                *child.lock().map_err(|_| "overlay child lock poisoned")? = Some(OverlayChild {
                    child: spawned,
                    stdin,
                    ready: false,
                    want_visible: true,
                    argv,
                    kind: overlay_kind,
                });
                let _ = proxy.send_event(UserEvent::Status("در حال پردازش مدل…".into(), true));
            }
        }
        "stop" => {
            let _ = proxy.send_event(UserEvent::Running(false));
            if let Ok(mut guard) = child.lock() {
                match guard.as_mut() {
                    Some(oc) if oc.kind == OverlayChildKind::Standby => {
                        oc.want_visible = false;
                        if oc.ready {
                            // keep the warm model child alive; just hide it
                            let _ = oc.stdin.write_all(b"hide\n");
                            let _ = oc.stdin.flush();
                        } else if let Some(mut old) = guard.take() {
                            // still loading; cancel the warm load
                            let _ = old.child.kill();
                            let _ = old.child.wait();
                        }
                    }
                    _ => {
                        if let Some(mut old) = guard.take() {
                            let _ = old.child.kill();
                            let _ = old.child.wait();
                        }
                    }
                }
            }
            let _ = proxy.send_event(UserEvent::Status("زیرنویس متوقف شد".into(), false));
        }
        "demo" => {
            let _ = proxy.send_event(UserEvent::Running(true));
            if let Ok(mut guard) = child.lock() {
                if let Some(mut old) = guard.take() {
                    let _ = old.child.kill();
                    let _ = old.child.wait();
                }
            }
            let style = style_args(&value);
            let mut argv = vec![
                "--overlay-demo".to_string(),
                "زیرنویس زنده برای هر ویدیو و هر جلسه.".to_string(),
            ];
            argv.extend(style);
            let (spawned, stdin) = spawn_overlay_child(argv.clone(), proxy.clone(), child.clone())?;
            *child.lock().map_err(|_| "overlay child lock poisoned")? = Some(OverlayChild {
                child: spawned,
                stdin,
                ready: false,
                want_visible: false,
                argv,
                kind: OverlayChildKind::Demo,
            });
            let _ = proxy.send_event(UserEvent::Status(
                "زیرنویس نمونه نمایش داده شد".into(),
                true,
            ));
        }
        "website" => {
            open_external_url("https://shenava.app");
        }
        "quit" => {
            let _ = proxy.send_event(UserEvent::Quit);
        }
        "hide" => {
            let _ = proxy.send_event(UserEvent::Hide);
        }
        "drag" => {
            let _ = proxy.send_event(UserEvent::Drag);
        }
        _ => {}
    }
    Ok(())
}

fn open_external_url(url: &str) {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    let _ = cmd.spawn();
}

fn map_device_choice(choice: &str) -> &str {
    match choice {
        // Product-level choices from the panel. Keep the UI simple; platform-specific capture
        // aliases are handled here and in audio.rs.
        "system" => "system",
        "mic" => "mic",
        other => other,
    }
}

fn parse_audio_level_line(line: &str) -> Option<f64> {
    let value: f64 = line
        .strip_prefix("[audio] level rms=")?
        .trim()
        .parse()
        .ok()?;
    value.is_finite().then_some(value.clamp(0.0, 1.0))
}

/// Spawn the resident hidden standby child (default config) that makes the first Start instant.
#[cfg(not(all(unix, not(target_os = "macos"))))]
fn warm_spawn(
    args: &ControlArgs,
    child: &Arc<Mutex<Option<OverlayChild>>>,
    proxy: &EventLoopProxy<UserEvent>,
) {
    let mut argv = vec![
        "--overlay-standby".to_string(),
        args.model_key.clone(),
        args.model_path.clone(),
        args.tokens_path.clone(),
        args.mel_path.clone(),
    ];
    if let Some(hotwords) = &args.hotwords_path {
        argv.extend(["--hotwords".to_string(), hotwords.clone()]);
    }
    argv.extend(["--device".to_string(), "system".to_string()]);
    // match the default-config argv the panel's front-end sends for a fresh Start, so a default
    // Start reuses this child instead of paying another full model load
    let defaults = serde_json::json!({
        "animation": "typewriter",
        "visibleLines": 2,
        "verticalPosition": 0.09,
        "fontSize": 64,
        "maxWidth": 0.82,
        "shadowBlur": 20,
        "shadowOpacity": 0.95,
        "shadowLift": 5,
        "rollingWindow": 12,
    });
    argv.extend(style_args(&defaults));
    if let Ok((spawned, stdin)) = spawn_overlay_child(argv.clone(), proxy.clone(), child.clone()) {
        if let Ok(mut guard) = child.lock() {
            if guard.is_none() {
                *guard = Some(OverlayChild {
                    child: spawned,
                    stdin,
                    ready: false,
                    want_visible: false,
                    argv,
                    kind: OverlayChildKind::Standby,
                });
            }
        }
    }
}

fn spawn_overlay_child(
    argv: Vec<String>,
    proxy: EventLoopProxy<UserEvent>,
    state: Arc<Mutex<Option<OverlayChild>>>,
) -> Result<(Child, std::process::ChildStdin), Box<dyn std::error::Error>> {
    kill_stale_overlay_children();
    let mut command = Command::new(std::env::current_exe()?);
    command.args(&argv);
    command.stderr(Stdio::piped());
    command.stdin(Stdio::piped());
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        command.env("GDK_BACKEND", "x11");
    }
    let mut child = command.spawn()?;
    let stdin = child.stdin.take().ok_or("child stdin unavailable")?;
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("{line}");
                if let Some(level) = parse_audio_level_line(&line) {
                    let _ = proxy.send_event(UserEvent::AudioLevel(level));
                }
                if line.contains("[audio] signal detected") {
                    let _ = proxy.send_event(UserEvent::Status("گوش می‌دهم…".into(), true));
                } else if line.contains("[model]") {
                    let msg = if line.contains("[model] ready") {
                        "مدل آماده است…"
                    } else if line.contains("pre-optimized NNEF") {
                        "در حال بارگذاری مدل بهینه…"
                    } else if line.contains("parsing ONNX") || line.contains("ONNX typed") {
                        "در حال پردازش مدل…"
                    } else if line.contains("converting to f16") {
                        "تبدیل دقت مدل…"
                    } else if line.contains("building optimized runnable") {
                        "ساخت موتور شناسایی…"
                    } else {
                        "در حال بارگذاری مدل…"
                    };
                    let _ = proxy.send_event(UserEvent::Status(msg.into(), true));
                } else if line.to_ascii_lowercase().contains("error")
                    || line.to_ascii_lowercase().contains("failed")
                {
                    let _ = proxy.send_event(UserEvent::Running(false));
                    let _ = proxy.send_event(UserEvent::Status(format!("خطا: {line}"), false));
                }
                // warm-start readiness: the live standby child is ready once the model is loaded;
                // if the user already clicked Start, reveal it immediately.
                let model_ready = line.contains("[model] ready");
                if model_ready || line.to_ascii_lowercase().contains("signal detected") {
                    if let Ok(mut guard) = state.lock() {
                        if let Some(oc) = guard.as_mut() {
                            oc.ready = true;
                            if oc.kind == OverlayChildKind::Standby && oc.want_visible {
                                let _ = oc.stdin.write_all(b"show\n");
                                let _ = oc.stdin.flush();
                            }
                        }
                    }
                    // A freshly loaded warm model unlocks the panel action buttons. Signal
                    // detection is a runtime audio event and must not reset Start/Stop state.
                    if model_ready {
                        let _ = proxy.send_event(UserEvent::ModelReady);
                    }
                }
            }
        });
    }
    Ok((child, stdin))
}

fn kill_stale_overlay_children() {
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Ok(exe) = std::env::current_exe() {
        let exe = exe.to_string_lossy();
        for mode in ["--overlay-demo", "--overlay-standby ", "--overlay "] {
            let pattern = format!("{exe} {mode}");
            let _ = Command::new("pkill").args(["-f", &pattern]).status();
        }
    }
}

fn stop_child(child: &Arc<Mutex<Option<OverlayChild>>>) {
    if let Ok(mut guard) = child.lock() {
        if let Some(mut oc) = guard.take() {
            let _ = oc.child.kill();
            let _ = oc.child.wait();
        }
    }
}

fn style_args(value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(animation) = value.get("animation").and_then(Value::as_str) {
        out.extend(["--animation".into(), animation.into()]);
    }
    for (json_key, flag) in [
        ("verticalPosition", "--vertical-position"),
        ("visibleLines", "--visible-lines"),
        ("fontSize", "--font-size"),
        ("maxWidth", "--max-width"),
        ("shadowBlur", "--shadow-blur"),
        ("shadowOpacity", "--shadow-opacity"),
        ("shadowLift", "--shadow-lift"),
        ("rollingWindow", "--rolling-window"),
    ] {
        if let Some(v) = value.get(json_key) {
            if let Some(s) = v
                .as_str()
                .map(ToString::to_string)
                .or_else(|| v.as_f64().map(|n| n.to_string()))
            {
                out.extend([flag.to_string(), s]);
            }
        }
    }
    out
}

fn panel_html() -> String {
    r#"<!doctype html>
<html lang="fa" dir="rtl">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<style>
:root {
  --burgundy: #78172b;
  --burgundy-hi: #97243a;
  --burgundy-deep: #5f1122;
  --cream: #fff7e0;
  --tan: #e9cfb8;
  --tan-text: #5a302d;
  --wash: #f8eee7;
  --muted: #8b6a66;
  --green: #15803d;   /* status text: >= 4.5:1 on white (WCAG AA) */
  --green-live: #1faa41; /* pulse/indicator: >= 3:1 UI */
  --ring: #b35a2c;    /* in-palette caramel focus ring */
}
* { box-sizing: border-box; }
html, body {
  overflow: hidden !important;
}
*::-webkit-scrollbar {
  width: 0 !important;
  height: 0 !important;
  display: none !important;
  background: transparent !important;
}
body {
  margin: 0;
  width: 340px;
  height: 424px;
  overflow: hidden;
  background: #fff;
  color: #251d1f;
  font-family: "Vazirmatn", "Segoe UI", Tahoma, sans-serif;
  user-select: none;
}
/* Shared keyboard/mouse feedback — idle look unchanged. */
button:focus-visible, select:focus-visible, input:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 2px;
}
button:active { transform: scale(.97); }
.header {
  height: 62px;
  background: var(--burgundy);
  color: var(--cream);
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 0 16px;
  border-bottom: 1px solid rgba(0,0,0,.14);
  box-shadow: 0 2px 6px rgba(70,18,26,.14);
}
.logo {
  width: 34px; height: 34px; border-radius: 8px;
  object-fit: cover; display: block;
  box-shadow: 0 0 0 1px rgba(255,247,224,.22), 0 8px 18px rgba(0,0,0,.24);
}
.brand { flex: 1; }
.brand b { display: block; font-size: 20px; line-height: 1.1; }
.brand span { display: block; font-size: 10.5px; opacity: .75; margin-top: 2px; }
.dot { width: 11px; height: 11px; border-radius: 50%; background: var(--green-live); flex: 0 0 auto; }
.dot.live { animation: pulse 2s ease-out infinite; }
@keyframes pulse {
  0%   { box-shadow: 0 0 0 0 rgba(31,170,65,.45); }
  70%  { box-shadow: 0 0 0 9px rgba(31,170,65,0); }
  100% { box-shadow: 0 0 0 0 rgba(31,170,65,0); }
}
@media (prefers-reduced-motion: reduce) {
  .dot.live { animation: none; }
}
.tabs { height: 46px; padding: 7px 12px; display: grid; grid-template-columns: repeat(4, 1fr); gap: 6px; direction: ltr; }
.tab {
  border: 0; border-radius: 999px; background: #f0e4dc; color: #8b625d;
  font-size: 19px; font-weight: 800; height: 32px; opacity: .82;
  transition: opacity .15s ease, background .15s ease;
}
.tab:hover { opacity: 1; }
.tab.active { background: var(--burgundy); color: var(--cream); opacity: 1; }
.panel { display: none; padding: 14px 16px 12px; height: 275px; }
.panel.active { display: block; }
.status {
  min-height: 34px; color: var(--green); font-size: 13px; font-weight: 800;
  display: flex; align-items: center; background: #f2f6f0; border-radius: 8px;
  padding: 6px 10px; line-height: 1.35;
}
.status-text { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.voice-meter { display: flex; align-items: center; gap: 5px; width: 92px; flex: 0 0 92px; direction: ltr; }
.voice-track { height: 8px; flex: 1; background: #ded8d2; border-radius: 5px; overflow: hidden; }
.voice-fill { width: 0%; height: 100%; background: #d9892b; border-radius: 5px; transition: width .12s ease, background .12s ease; }
.voice-label { min-width: 33px; color: #8b6a66; font-size: 10px; font-weight: 800; direction: rtl; text-align: right; }
.model-wait { display: inline-flex; align-items: center; gap: 4px; flex: 0 0 auto; color: #b35a2c; font-size: 10px; font-weight: 800; direction: rtl; }
.model-spinner { width: 11px; height: 11px; border: 2px solid #e9cfb8; border-top-color: #78172b; border-radius: 50%; animation: model-spin .85s linear infinite; }
@keyframes model-spin { to { transform: rotate(360deg); } }
button:disabled { opacity: .45; cursor: not-allowed; }
label.cap { display: block; color: #4d4548; font-size: 13px; font-weight: 800; margin: 8px 0 4px; }
select, button, input[type=range] { font: inherit; }
select {
  width: 100%; height: 36px; border: 1px solid #ddd6cf; border-radius: 8px; background: #f5f1ec;
  padding: 0 12px; font-size: 17px; font-weight: 800; color: #2d292a;
}
/* ---- Button hierarchy: solid primary / outlined secondary / quiet tertiary ---- */
.primary {
  width: 100%; height: 42px; border: 0; border-radius: 9px; font-weight: 900; font-size: 17px;
  background: var(--burgundy); color: var(--cream); margin-top: 12px; padding: 0 14px;
  box-shadow: inset 0 -2px 0 rgba(0,0,0,.16); transition: background .15s ease;
}
.primary:hover { background: var(--burgundy-hi); }
.primary:active { background: var(--burgundy-deep); }
.ghost {
  width: 100%; height: 38px; border: 1.5px solid var(--burgundy); border-radius: 9px;
  font-weight: 800; font-size: 15px; background: var(--wash); color: var(--burgundy); padding: 0 8px;
  transition: background .15s ease;
}
.ghost:hover { background: #f0dfd2; }
.ghost:active { background: var(--tan); }
.quiet {
  width: 100%; height: 32px; border: 1px solid rgba(138,106,102,.45); border-radius: 8px;
  font-weight: 700; font-size: 13.5px; background: #fff; color: var(--muted); padding: 0 8px;
  transition: background .15s ease;
}
.quiet:hover { background: var(--wash); }
.row2 { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-top: 10px; }
.wide { margin-top: 10px; }
.seg { height: 28px; display: grid; gap: 0; background: #e9e9e9; border-radius: 8px; overflow: hidden; }
.seg.four { grid-template-columns: repeat(4, 1fr); }
.seg button { border: 0; background: transparent; font-size: 15px; font-weight: 800; color: #211c1d; }
.seg button.active { background: #ccc; }
.slider-row { display: grid; grid-template-columns: 74px 1fr 44px; align-items: center; gap: 8px; height: 34px; direction: rtl; }
.slider-row b { text-align: right; font-size: 13px; }
.slider-row output { direction: ltr; color: #8c8c8c; font-size: 12px; font-variant-numeric: tabular-nums; }
input[type=range] { accent-color: #b35a2c; direction: ltr; }
.about { text-align: center; padding-top: 10px; }
.about .biglogo { margin: 0 auto 8px; width: 48px; height: 48px; border-radius: 12px; object-fit: cover; display: block; box-shadow: 0 8px 18px rgba(70,18,26,.20); }
.about h1 { margin: 0; font-size: 24px; }
.about p { color: #555; font-weight: 700; font-size: 13px; margin: 4px 0 2px; }
.about small { color: var(--muted); direction: ltr; display: block; margin-bottom: 6px; }
.about .primary { margin-top: 6px; }
.about .wide { margin-top: 6px; }
.footer {
  height: 42px; border-top: 1px solid rgba(233,207,184,.8);
  display: flex; align-items: center; justify-content: flex-start; padding: 0 14px; gap: 8px;
}
.roll {
  height: 30px; border: 1px solid rgba(138,106,102,.4); border-radius: 8px;
  background: var(--tan); color: var(--tan-text); font-weight: 800;
  display: flex; align-items: center; justify-content: center; padding: 0 12px 1px; line-height: 1;
}
.exit {
  height: 30px; border: 0; border-radius: 8px; background: var(--burgundy); color: var(--cream);
  font-weight: 800; padding: 0 14px 1px; font-size: 13px; line-height: 1; transition: background .15s ease;
}
.exit:hover { background: var(--burgundy-hi); }
</style>
</head>
<body>
<div class="header"><img class="logo" alt="Shenava" src="__LOGO_URI__"><div class="brand"><b>شنوا</b><span>زیرنویس زندهٔ روی دستگاه</span></div><div id="dot" class="dot"></div></div>
<div class="tabs">
  <button class="tab active" data-tab="live" aria-label="زیرنویس زنده">▥</button>
  <button class="tab" data-tab="display" aria-label="نمایش">Aᴀ</button>
  <button class="tab" data-tab="appearance" aria-label="ظاهر">☷</button>
  <button class="tab" data-tab="about" aria-label="درباره">ⓘ</button>
</div>
<section id="live" class="panel active">
  <div id="status" class="status" role="status" aria-live="polite"><span id="statusText" class="status-text">مدل در حال آماده‌سازی است؛ لطفاً صبر کنید…</span><span id="modelWait" class="model-wait"><span class="model-spinner"></span><span id="waitElapsed">۰ث</span></span><span class="voice-meter" aria-label="سطح ورودی صدا"><span class="voice-track"><span id="voiceFill" class="voice-fill"></span></span><span id="voiceLabel" class="voice-label">بدون داده</span></span></div>
  <label class="cap" for="device">ورودی صدا</label>
  <select id="device" aria-label="ورودی صدا"><option value="system">صدای سیستم</option><option value="mic">میکروفون</option></select>
  <button class="primary" id="start" aria-label="شروع زیرنویس" disabled>شروع زیرنویس</button>
  <div class="row2"><button class="ghost" id="demo" aria-label="نمایش زیرنویس" disabled>نمایش زیرنویس</button><button class="ghost" id="stop" aria-label="پنهان کردن زیرنویس" disabled>پنهان کردن</button></div>
  <button class="quiet wide" id="sample" aria-label="نمایش یک زیرنویس نمونه" disabled>نمایش یک زیرنویس نمونه</button>
</section>
<section id="display" class="panel">
  <label class="cap">حرکت متن</label>
  <div class="seg four" id="anim"><button data-v="pop">ظاهرشدن</button><button data-v="karaoke">کاراوکه</button><button data-v="slide">لغزش</button><button class="active" data-v="typewriter">تایپی</button></div>
  <div class="slider-row"><b>جای عمودی</b><input id="verticalPosition" type="range" min="0" max="1" step="0.01" value="0.09"><output>پایین</output></div>
  <label class="cap">تعداد خط</label>
  <div class="seg four" id="lines"><button data-v="1">۱</button><button class="active" data-v="2">۲</button><button data-v="3">۳</button><button data-v="4">۴</button></div>
  <div class="slider-row"><b>اندازه</b><input id="fontSize" type="range" min="38" max="92" step="1" value="64"><output>64</output></div>
  <div class="slider-row"><b>پهنا</b><input id="maxWidth" type="range" min="0.5" max="0.94" step="0.01" value="0.82"><output>82%</output></div>
</section>
<section id="appearance" class="panel">
  <label class="cap">سایه و تأخیر</label>
  <div class="slider-row"><b>محو سایه</b><input id="shadowBlur" type="range" min="0" max="44" step="1" value="20"><output>20</output></div>
  <div class="slider-row"><b>شفافیت</b><input id="shadowOpacity" type="range" min="0" max="1" step="0.01" value="0.95"><output>0.95</output></div>
  <div class="slider-row"><b>بلندی</b><input id="shadowLift" type="range" min="-12" max="18" step="1" value="5"><output>5</output></div>
  <div class="slider-row"><b>پنجرهٔ صدا</b><input id="rolling" type="range" min="4" max="20" step="0.5" value="12"><output>12.0s</output></div>
</section>
<section id="about" class="panel about">
  <img class="biglogo" alt="Shenava" src="__LOGO_URI__"><h1>شنوا</h1>
  <p>زیرنویس زندهٔ فارسی، روی دستگاه و بدون اینترنت</p>
  <small>مدل کوچیک • ساخت 2026-06-19 01:20</small>
  <button class="primary" id="website">وب‌سایت شنوا</button>
<button class="ghost wide" id="quit">خروج از برنامه</button>
</section>
<div class="footer"><button class="roll" id="roll" aria-label="جمع کردن پنجره">⌃ جمع کردن</button><span style="flex:1"></span><button class="exit" id="quitFooter" aria-label="خروج از برنامه">خروج از برنامه</button></div>
<script>
const state={animation:'typewriter',visibleLines:2,running:false,mode:null};
const statusTextEl=document.getElementById('statusText');
const dotEl=document.getElementById('dot');
const startEl=document.getElementById('start');
const voiceFillEl=document.getElementById('voiceFill');
const voiceLabelEl=document.getElementById('voiceLabel');
const modelWaitEl=document.getElementById('modelWait');
const waitElapsedEl=document.getElementById('waitElapsed');
let lastAudioLevelAt=0;
const post=(cmd)=>{if(cmd==='start')state.mode='start'; if(cmd==='demo')state.mode='demo'; if(cmd==='stop')state.mode=null; window.ipc.postMessage(JSON.stringify({...state,cmd,device:document.getElementById('device').value,
  verticalPosition:+verticalPosition.value,fontSize:+fontSize.value,maxWidth:+maxWidth.value,
  shadowBlur:+shadowBlur.value,shadowOpacity:+shadowOpacity.value,shadowLift:+shadowLift.value,rollingWindow:+rolling.value}))};
let styleTimer=null;
const restartIfRunning=()=>{if(!state.running||!state.mode)return;clearTimeout(styleTimer);styleTimer=setTimeout(()=>post(state.mode),350)};
// Frameless panel has no title bar; dragging the header must be wired through the OS (tao
// drag_window) via IPC, because the embedded WebView2 consumes mouse events on its own HWND.
document.querySelector('.header').addEventListener('mousedown',e=>{if(e.button===0){e.preventDefault();post('drag')}});
document.querySelectorAll('.tab').forEach(b=>b.onclick=()=>{document.querySelectorAll('.tab,.panel').forEach(x=>x.classList.remove('active'));b.classList.add('active');document.getElementById(b.dataset.tab).classList.add('active')});
document.querySelectorAll('#anim button').forEach(b=>b.onclick=()=>{document.querySelectorAll('#anim button').forEach(x=>x.classList.remove('active'));b.classList.add('active');state.animation=b.dataset.v;restartIfRunning()});
document.querySelectorAll('#lines button').forEach(b=>b.onclick=()=>{document.querySelectorAll('#lines button').forEach(x=>x.classList.remove('active'));b.classList.add('active');state.visibleLines=+b.dataset.v;restartIfRunning()});
document.querySelectorAll('input[type=range]').forEach(i=>i.oninput=()=>{let o=i.nextElementSibling;if(i.id==='verticalPosition')o.value=i.value<.34?'پایین':(i.value>.66?'بالا':'میانه');else if(i.id==='maxWidth')o.value=Math.round(i.value*100)+'%';else if(i.id==='rolling')o.value=(+i.value).toFixed(1)+'s';else o.value=i.value;restartIfRunning()});
start.onclick=()=>post(state.running?'stop':'start'); stop.onclick=()=>post('stop'); sample.onclick=()=>post('demo'); demo.onclick=()=>post('demo'); quit.onclick=()=>post('quit'); website.onclick=()=>post('website');
roll.onclick=()=>post('hide');
quitFooter.onclick=()=>post('quit');
// A11y: expose each slider's visible Persian label to assistive tech (no visual change).
document.querySelectorAll('.slider-row').forEach(row=>{const b=row.querySelector('b');const i=row.querySelector('input');if(b&&i)i.setAttribute('aria-label',b.textContent)});
window.__shenavaSetStatus=(m,ok)=>{statusTextEl.textContent=m;statusTextEl.style.color=ok?'#1faa41':'#d9892b';dotEl.style.background=ok?'#1faa41':'#d9892b';dotEl.classList.toggle('live',ok)};
window.__shenavaSetRunning=running=>{state.running=running;if(!running)state.mode=null;modelWaitEl.hidden=true;startEl.textContent=running?'توقف':'شروع زیرنویس'};
const audioMeterFraction=rms=>Math.max(0,Math.min(1,(20*Math.log10(Math.max(rms,0.000001))+60)/60));
window.__shenavaAudioLevel=rms=>{if(!Number.isFinite(rms)||rms<0)return;const active=rms>=0.0005;voiceFillEl.style.width=Math.round(audioMeterFraction(rms)*100)+'%';voiceFillEl.style.background=active?'#1faa41':'#d9892b';voiceLabelEl.textContent=active?'صدا':'ساکت';voiceLabelEl.style.color=active?'#15803d':'#8b6a66';lastAudioLevelAt=Date.now()};
setInterval(()=>{if(lastAudioLevelAt&&Date.now()-lastAudioLevelAt>900){voiceFillEl.style.width='0%';voiceLabelEl.textContent='بدون داده';voiceLabelEl.style.color='#8b6a66';}},300);
const panelButtons=['start','demo','stop','sample'].map(id=>document.getElementById(id));
let locked=true;
function setLocked(v){locked=v;panelButtons.forEach(b=>{b.disabled=v});}
const faDigits=s=>String(s).replace(/[0-9]/g,d=>'۰۱۲۳۴۵۶۷۸۹'[d]);
const modelWaitStartedAt=Date.now();
const refreshModelWait=()=>{const seconds=Math.floor((Date.now()-modelWaitStartedAt)/1000);waitElapsedEl.textContent=faDigits(seconds)+'ث'};
refreshModelWait();
setInterval(refreshModelWait,1000);
window.__shenavaModelUnavailable=m=>{modelWaitEl.hidden=true;setLocked(true);window.__shenavaSetStatus(m,false)};
window.__shenavaModelReady=()=>{modelWaitEl.hidden=true;setLocked(false)};
</script>
</body>
</html>"#
    .replace("__LOGO_URI__", &logo_data_uri())
}

fn logo_data_uri() -> String {
    // WebView2's NavigateToString has a strict inline-document size limit. The 512 px source is
    // retained for native rendering, while this 128 px copy keeps the two embedded panel images
    // comfortably below that limit at their actual 34 px and 48 px display sizes.
    format!("data:image/png;base64,{}", STANDARD.encode(PANEL_LOGO_PNG))
}

fn window_icon() -> Option<Icon> {
    let image = image::load_from_memory_with_format(PANEL_LOGO_PNG, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height).ok()
}

#[cfg(test)]
mod tests {
    use super::parse_audio_level_line;

    #[test]
    fn parses_rms_audio_level_lines_for_the_panel_meter() {
        assert_eq!(
            parse_audio_level_line("[audio] level rms=0.01234"),
            Some(0.01234)
        );
        assert_eq!(
            parse_audio_level_line("[audio] signal detected rms=0.00216"),
            None
        );
        assert_eq!(parse_audio_level_line("[audio] level rms=bad"), None);
    }

    #[test]
    fn panel_starts_with_explicit_model_loading_state() {
        let html = super::panel_html();
        assert!(html.contains("id=\"modelWait\""));
        assert!(html.contains("id=\"start\" aria-label=\"شروع زیرنویس\" disabled"));
    }
}
