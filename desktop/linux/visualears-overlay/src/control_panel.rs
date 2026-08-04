use serde_json::Value;
use std::io::{BufRead, BufReader};
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
    ToggleAt(i32, i32),
    Hide,
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

    let child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let proxy = event_loop.create_proxy();
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
    child: &Arc<Mutex<Option<Child>>>,
    proxy: &EventLoopProxy<UserEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    let value: Value = serde_json::from_str(body)?;
    let cmd = value.get("cmd").and_then(Value::as_str).unwrap_or("");
    match cmd {
        "start" => {
            stop_child(child);
            let style = style_args(&value);
            let mut argv = vec![
                "--overlay".to_string(),
                args.model_key.clone(),
                args.model_path.clone(),
                args.tokens_path.clone(),
                args.mel_path.clone(),
            ];
            if let Some(hotwords) = &args.hotwords_path {
                argv.extend(["--hotwords".to_string(), hotwords.clone()]);
            }
            if let Some(device) = value
                .get("device")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                argv.extend([
                    "--device".to_string(),
                    map_device_choice(device).to_string(),
                ]);
            }
            argv.extend(style);
            let waiting = if value.get("device").and_then(Value::as_str) == Some("system") {
                "در انتظار صدای سیستم…"
            } else {
                "در انتظار صدا…"
            };
            let _ = proxy.send_event(UserEvent::Status(waiting.into(), true));
            let spawned = spawn_overlay_child(argv, proxy.clone())?;
            *child.lock().map_err(|_| "overlay child lock poisoned")? = Some(spawned);
        }
        "stop" => {
            stop_child(child);
            let _ = proxy.send_event(UserEvent::Status("زیرنویس متوقف شد".into(), false));
        }
        "demo" => {
            stop_child(child);
            let style = style_args(&value);
            let mut argv = vec![
                "--overlay-demo".to_string(),
                "زیرنویس زنده برای هر ویدیو و هر جلسه.".to_string(),
            ];
            argv.extend(style);
            let spawned = spawn_overlay_child(argv, proxy.clone())?;
            *child.lock().map_err(|_| "overlay child lock poisoned")? = Some(spawned);
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

fn spawn_overlay_child(
    argv: Vec<String>,
    proxy: EventLoopProxy<UserEvent>,
) -> Result<Child, Box<dyn std::error::Error>> {
    kill_stale_overlay_children();
    let mut command = Command::new(std::env::current_exe()?);
    command.args(argv);
    command.stderr(Stdio::piped());
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // GNOME Wayland does not allow app-controlled top-level window placement. Use GTK on
        // Xwayland for the small transparent subtitle window so the vertical-position slider can
        // actually move the overlay.
        command.env("GDK_BACKEND", "x11");
    }
    let mut child = command.spawn()?;
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("{line}");
                if line.contains("[audio] signal detected") {
                    let _ = proxy.send_event(UserEvent::Status("گوش می‌دهم…".into(), true));
                } else if line.to_ascii_lowercase().contains("error")
                    || line.to_ascii_lowercase().contains("failed")
                {
                    let _ = proxy.send_event(UserEvent::Status(format!("خطا: {line}"), false));
                }
            }
        });
    }
    Ok(child)
}

fn kill_stale_overlay_children() {
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Ok(exe) = std::env::current_exe() {
        let exe = exe.to_string_lossy();
        for mode in ["--overlay-demo", "--overlay "] {
            let pattern = format!("{exe} {mode}");
            let _ = Command::new("pkill").args(["-f", &pattern]).status();
        }
    }
}

fn stop_child(child: &Arc<Mutex<Option<Child>>>) {
    if let Ok(mut guard) = child.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
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
  --cream: #fff7e0;
  --tan: #e9cfb8;
  --tan-text: #5a302d;
  --wash: #f8eee7;
  --muted: #8b6a66;
  --green: #32c95a;
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
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "Vazirmatn", "Tahoma", sans-serif;
  user-select: none;
}
.header {
  height: 62px;
  background: var(--burgundy);
  color: var(--cream);
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 0 16px;
}
.logo {
  width: 34px; height: 34px; border-radius: 8px;
  object-fit: cover; display: block;
  box-shadow: 0 0 0 1px rgba(255,247,224,.22), 0 8px 18px rgba(0,0,0,.24);
}
.brand { flex: 1; }
.brand b { display: block; font-size: 20px; line-height: 1.1; }
.brand span { display: block; font-size: 10.5px; opacity: .72; margin-top: 2px; }
.dot { width: 10px; height: 10px; border-radius: 50%; background: var(--green); }
.tabs { height: 46px; padding: 7px 12px; display: grid; grid-template-columns: repeat(4, 1fr); gap: 6px; direction: ltr; }
.tab {
  border: 0; border-radius: 999px; background: #f0e4dc; color: #8b625d;
  font-size: 19px; font-weight: 800; height: 32px;
}
.tab.active { background: var(--burgundy); color: var(--cream); }
.panel { display: none; padding: 14px 16px; height: 275px; }
.panel.active { display: block; }
.status { min-height: 32px; color: var(--green); font-size: 13px; font-weight: 700; display:flex; align-items:center; }
label.cap { display:block; color:#4d4548; font-size: 13px; font-weight: 800; margin: 8px 0; }
select, button, input[type=range] { font: inherit; }
select {
  width:100%; height: 34px; border: 0; border-radius: 8px; background:#e9e9e9;
  padding: 0 12px; font-size: 17px; font-weight: 800; color:#2d292a;
}
.primary, .ghost {
  width:100%; height:34px; border:0; border-radius:9px; font-weight:900; font-size:17px;
}
.primary { background:var(--burgundy); color:var(--cream); margin-top: 10px; }
.ghost { background:var(--tan); color:var(--tan-text); }
.row2 { display:grid; grid-template-columns:1fr 1fr; gap:8px; margin-top:10px; }
.wide { margin-top:10px; }
.seg { height: 26px; display:grid; gap:0; background:#e9e9e9; border-radius:8px; overflow:hidden; }
.seg.four { grid-template-columns: repeat(4, 1fr); }
.seg button { border:0; background:transparent; font-size:15px; font-weight:800; color:#211c1d; }
.seg button.active { background:#ccc; }
.slider-row { display:grid; grid-template-columns:74px 1fr 44px; align-items:center; gap:8px; height:34px; direction:rtl; }
.slider-row b { text-align:right; font-size:13px; }
.slider-row output { direction:ltr; color:#8c8c8c; font-size:12px; font-variant-numeric: tabular-nums; }
input[type=range] { accent-color:#cfcfcf; direction:ltr; }
.about { text-align:center; padding-top: 10px; }
.about .biglogo { margin: 0 auto 8px; width:48px; height:48px; border-radius:12px; object-fit:cover; display:block; box-shadow:0 8px 18px rgba(70,18,26,.20); }
.about h1 { margin: 0; font-size: 24px; }
.about p { color:#555; font-weight:700; font-size:13px; margin:4px 0 2px; }
.about small { color:#999; direction:ltr; display:block; margin-bottom:6px; }
.about .primary { margin-top: 6px; }
.about .wide { margin-top: 6px; }
.footer { height:40px; border-top:1px solid rgba(233,207,184,.8); display:flex; align-items:center; justify-content:flex-start; padding:0 14px; }
.roll { width:108px; height:26px; border:0; border-radius:8px; background:var(--tan); color:var(--tan-text); font-weight:800; display:flex; align-items:center; justify-content:center; padding:0 10px 1px; line-height:1; }
</style>
</head>
<body>
<div class="header"><img class="logo" alt="Shenava" src="__LOGO_URI__"><div class="brand"><b>شنوا</b><span>زیرنویس زندهٔ روی دستگاه</span></div><div id="dot" class="dot"></div></div>
<div class="tabs">
  <button class="tab active" data-tab="live">▥</button>
  <button class="tab" data-tab="display">Aᴀ</button>
  <button class="tab" data-tab="appearance">☷</button>
  <button class="tab" data-tab="about">ⓘ</button>
</div>
<section id="live" class="panel active">
  <div id="status" class="status">مدل کوچیک + بیم ۳٬۶۶۹ کلمه‌ای آماده است</div>
  <label class="cap">ورودی صدا</label>
  <select id="device"><option value="system">صدای سیستم</option><option value="mic">میکروفون</option></select>
  <button class="primary" id="start">شروع زیرنویس</button>
  <div class="row2"><button class="ghost" id="demo">نمایش زیرنویس</button><button class="ghost" id="stop">پنهان کردن</button></div>
  <button class="ghost wide" id="sample">نمایش یک زیرنویس نمونه</button>
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
<div class="footer"><button class="roll" id="roll">⌃ جمع کردن</button></div>
<script>
const state={animation:'typewriter',visibleLines:2,running:false,mode:null};
const statusEl=document.getElementById('status');
const dotEl=document.getElementById('dot');
const startEl=document.getElementById('start');
const post=(cmd)=>{if(cmd==='start')state.mode='start'; if(cmd==='demo')state.mode='demo'; if(cmd==='stop')state.mode=null; window.ipc.postMessage(JSON.stringify({...state,cmd,device:document.getElementById('device').value,
  verticalPosition:+verticalPosition.value,fontSize:+fontSize.value,maxWidth:+maxWidth.value,
  shadowBlur:+shadowBlur.value,shadowOpacity:+shadowOpacity.value,shadowLift:+shadowLift.value,rollingWindow:+rolling.value}))};
let styleTimer=null;
const restartIfRunning=()=>{if(!state.running||!state.mode)return;clearTimeout(styleTimer);styleTimer=setTimeout(()=>post(state.mode),350)};
document.querySelectorAll('.tab').forEach(b=>b.onclick=()=>{document.querySelectorAll('.tab,.panel').forEach(x=>x.classList.remove('active'));b.classList.add('active');document.getElementById(b.dataset.tab).classList.add('active')});
document.querySelectorAll('#anim button').forEach(b=>b.onclick=()=>{document.querySelectorAll('#anim button').forEach(x=>x.classList.remove('active'));b.classList.add('active');state.animation=b.dataset.v;restartIfRunning()});
document.querySelectorAll('#lines button').forEach(b=>b.onclick=()=>{document.querySelectorAll('#lines button').forEach(x=>x.classList.remove('active'));b.classList.add('active');state.visibleLines=+b.dataset.v;restartIfRunning()});
document.querySelectorAll('input[type=range]').forEach(i=>i.oninput=()=>{let o=i.nextElementSibling;if(i.id==='verticalPosition')o.value=i.value<.34?'پایین':(i.value>.66?'بالا':'میانه');else if(i.id==='maxWidth')o.value=Math.round(i.value*100)+'%';else if(i.id==='rolling')o.value=(+i.value).toFixed(1)+'s';else o.value=i.value;restartIfRunning()});
start.onclick=()=>post(state.running?'stop':'start'); stop.onclick=()=>post('stop'); sample.onclick=()=>post('demo'); demo.onclick=()=>post('demo'); quit.onclick=()=>post('quit'); website.onclick=()=>post('website');
roll.onclick=()=>post('hide');
window.__shenavaSetStatus=(m,ok)=>{state.running=ok;if(!ok)state.mode=null;statusEl.textContent=m;statusEl.style.color=ok?'#32c95a':'#d9892b';dotEl.style.background=ok?'#32c95a':'#d9892b';startEl.textContent=ok?'توقف':'شروع زیرنویس'};
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
