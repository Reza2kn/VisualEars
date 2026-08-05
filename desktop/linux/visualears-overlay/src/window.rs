//! Transparent, always-on-top caption overlay: winit window + softbuffer presentation. A worker
//! thread drains mic audio, ticks the live captioner, and updates the shared transcript; the window
//! redraws it at ~30fps via the cosmic-text renderer (correct shaped Persian).

use crate::caption_render::CaptionRenderer;
use crate::captioner::{CaptionEvent, LiveCaptioner, TranscriptState};
use crate::engine::StreamingRecognizer;
use crate::rescore::StaticRescorer;
use crate::style::{CaptionAnimation, OverlayStyle};
#[cfg(not(all(unix, not(target_os = "macos"))))]
use std::num::NonZeroU32;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
#[cfg(not(all(unix, not(target_os = "macos"))))]
use winit::application::ApplicationHandler;
#[cfg(not(all(unix, not(target_os = "macos"))))]
use winit::event::WindowEvent;
#[cfg(not(all(unix, not(target_os = "macos"))))]
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
#[cfg(not(all(unix, not(target_os = "macos"))))]
use winit::window::{Icon, Window, WindowLevel};
#[cfg(not(all(unix, not(target_os = "macos"))))]
use std::io::BufRead;
#[cfg(not(all(unix, not(target_os = "macos"))))]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(all(unix, not(target_os = "macos"))))]
const SHENAVA_ICON_PNG: &[u8] = include_bytes!("../assets/shenava-panel-logo.png");

type Shared = Arc<Mutex<TranscriptState>>;

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn shenava_window_icon() -> Option<Icon> {
    let image = image::load_from_memory_with_format(SHENAVA_ICON_PNG, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height).ok()
}

fn overlay_size(style: &OverlayStyle) -> (i32, i32) {
    let line_h = style.font_size * style.line_height;
    let shadow_room = style.shadow_blur * 0.55 + style.shadow_lift.abs();
    let height = (line_h * style.visible_lines as f32 + shadow_room * 2.0 + 42.0)
        .round()
        .clamp(104.0, 390.0) as i32;
    (1280, height)
}

struct AnimationFrame {
    text: String,
    font_scale: f32,
    y_shift: i32,
}

struct AnimationState {
    last_text: String,
    started: Instant,
    typewriter_base_chars: usize,
}

impl AnimationState {
    fn new() -> Self {
        Self {
            last_text: String::new(),
            started: Instant::now(),
            typewriter_base_chars: 0,
        }
    }

    fn frame(&mut self, text: String, style: &OverlayStyle) -> AnimationFrame {
        if text != self.last_text {
            self.typewriter_base_chars = typewriter_base_chars(&self.last_text, &text);
            self.last_text = text.clone();
            self.started = Instant::now();
        }
        let elapsed = self.started.elapsed().as_secs_f32();
        match style.animation {
            CaptionAnimation::Typewriter => {
                let chars: Vec<char> = text.chars().collect();
                let take = (self.typewriter_base_chars + (elapsed * 34.0).ceil() as usize)
                    .min(chars.len())
                    .max(1);
                AnimationFrame {
                    text: chars.into_iter().take(take).collect(),
                    font_scale: 1.0,
                    y_shift: 0,
                }
            }
            CaptionAnimation::Karaoke => {
                let words: Vec<&str> = text.split_whitespace().collect();
                let take = ((elapsed * 4.6).ceil() as usize).min(words.len()).max(1);
                AnimationFrame {
                    text: words.into_iter().take(take).collect::<Vec<_>>().join(" "),
                    font_scale: 1.0,
                    y_shift: 0,
                }
            }
            CaptionAnimation::Pop => {
                let t = (elapsed / 0.18).clamp(0.0, 1.0);
                AnimationFrame {
                    text,
                    font_scale: 0.88 + 0.12 * t,
                    y_shift: 0,
                }
            }
            CaptionAnimation::Slide => {
                let t = (elapsed / 0.22).clamp(0.0, 1.0);
                AnimationFrame {
                    text,
                    font_scale: 1.0,
                    y_shift: ((1.0 - t) * 20.0).round() as i32,
                }
            }
        }
    }
}

/// Match macOS `setRenderedText`: only a pure prefix extension gets typewriter animation. Existing
/// words remain fixed; a roll-up/replacement appears immediately instead of retyping the viewport.
fn typewriter_base_chars(previous: &str, current: &str) -> usize {
    if previous.is_empty() {
        0
    } else if current.starts_with(previous) {
        previous.chars().count()
    } else {
        current.chars().count()
    }
}

/// Worker: accumulate mic samples, tick the captioner ~every 120 ms, publish to the transcript.
fn caption_worker(rx: Receiver<Vec<f32>>, mut cap: LiveCaptioner, transcript: Shared) {
    let mut ring: Vec<f32> = Vec::new();
    loop {
        // Drain everything available (blocking on the first, non-blocking after).
        match rx.recv_timeout(Duration::from_millis(120)) {
            Ok(chunk) => ring.extend(chunk),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
        while let Ok(chunk) = rx.try_recv() {
            ring.extend(chunk);
        }
        let batch = std::mem::take(&mut ring);
        for ev in cap.feed_batch(&batch) {
            if let Ok(mut ts) = transcript.lock() {
                ts.apply(&ev);
            }
        }
    }
}

/// standby caption worker: identical to `caption_worker` but only runs the heavy ASR while `active`
/// is true (window visible). When hidden it cheaply drains audio so the warm-loaded model idles at
/// ~0 ASR CPU instead of being torn down.
#[cfg(not(all(unix, not(target_os = "macos"))))]
fn caption_worker_standby(
    rx: Receiver<Vec<f32>>,
    mut cap: LiveCaptioner,
    transcript: Shared,
    active: Arc<AtomicBool>,
) {
    let mut ring: Vec<f32> = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(120)) {
            Ok(chunk) => ring.extend(chunk),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
        while let Ok(chunk) = rx.try_recv() {
            ring.extend(chunk);
        }
        let batch = std::mem::take(&mut ring);
        if active.load(Ordering::Relaxed) {
            for ev in cap.feed_batch(&batch) {
                if let Ok(mut ts) = transcript.lock() {
                    ts.apply(&ev);
                }
            }
        }
    }
}

/// Shared steering state for a standby (hidden-at-launch) overlay child.
#[cfg(not(all(unix, not(target_os = "macos"))))]
struct StandbyControl {
    window: Option<Arc<Window>>,
    visible: bool,
    quit: bool,
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
struct Overlay {
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    renderer: CaptionRenderer,
    animation: AnimationState,
    transcript: Shared,
    style: OverlayStyle,
    /// standby-only: worker ASR gate (true => visible + processing)
    active: Arc<AtomicBool>,
    /// standby-only: shared show/hide/quit steering
    standby: Option<Arc<Mutex<StandbyControl>>>,
    /// standby-only: last-applied visibility (to avoid re-calling set_visible every frame)
    last_visible: bool,
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
impl ApplicationHandler for Overlay {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let (win_w, win_h) = overlay_size(&self.style);
        let attrs = Window::default_attributes()
            .with_title("Shenava")
            .with_window_icon(shenava_window_icon())
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(true)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_inner_size(winit::dpi::LogicalSize::new(win_w as f64, win_h as f64));
        let window = Arc::new(el.create_window(attrs).expect("create overlay window"));
        // Park the caption strip at the bottom-center of the primary monitor (subtitle position),
        // instead of winit's default top-left placement.
        if let Some(mon) = window.current_monitor() {
            let msize = mon.size();
            let mpos = mon.position();
            let wsize = window.outer_size();
            let x = mpos.x + ((msize.width as i32 - wsize.width as i32) / 2).max(0);
            let margin = (msize.height as f32 * 0.03) as i32;
            let min_y = mpos.y + margin;
            let max_y = mpos.y + (msize.height as i32 - wsize.height as i32 - margin).max(0);
            let y = min_y
                + ((max_y - min_y) as f32 * (1.0 - self.style.vertical_position)).round() as i32;
            window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
        }
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        let surface =
            softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
        self.window = Some(window);
        self.surface = Some(surface);
        if let Some(ctrl) = &self.standby {
            if let Ok(mut c) = ctrl.lock() {
                c.window = self.window.clone();
            }
            if let Some(w) = &self.window {
                w.set_visible(false);
            }
        }
    }

    fn window_event(
            &mut self,
            el: &ActiveEventLoop,
            _id: winit::window::WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::CloseRequested => el.exit(),
                WindowEvent::RedrawRequested => self.draw(),
                _ => {}
            }
        }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        el.set_control_flow(ControlFlow::wait_duration(Duration::from_millis(33)));
        if let Some(ctrl) = &self.standby {
            let c = ctrl.lock().unwrap();
            if c.quit {
                el.exit();
                return;
            }
            if c.visible != self.last_visible {
                self.last_visible = c.visible;
                self.active.store(c.visible, Ordering::Relaxed);
                if let Some(w) = c.window.clone() {
                    w.set_visible(c.visible);
                    w.request_redraw();
                }
            }
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
impl Overlay {
    fn draw(&mut self) {
        let (Some(window), Some(surface)) = (&self.window, &mut self.surface) else {
            return;
        };
        let size = window.inner_size();
        let (w, h) = (size.width as usize, size.height as usize);
        let (Some(nw), Some(nh)) = (NonZeroU32::new(w as u32), NonZeroU32::new(h as u32)) else {
            return;
        };
        surface.resize(nw, nh).ok();

        let blocks = self
            .transcript
            .lock()
            .ok()
            .map(|ts| ts.blocks(24))
            .unwrap_or_default();
        let text = self
            .renderer
            .rollup_text(&blocks, w, self.style.font_size, &self.style);
        let frame = self.animation.frame(text, &self.style);
        let font_px = self.style.font_size * frame.font_scale;
        // Keep the subtitle surface itself fully transparent. Some Linux compositors/backends
        // ignore fractional alpha on softbuffer surfaces and turn even a subtle tinted backing into
        // a fully opaque burgundy rectangle.
        let bg = (0, 0, 0, 0);
        let rgba = self.renderer.render_with_style_offset(
            &frame.text,
            w,
            h,
            font_px,
            bg,
            &self.style,
            frame.y_shift,
        );

        if let Ok(mut buffer) = surface.buffer_mut() {
            for (i, px) in buffer.iter_mut().enumerate() {
                // Write ARGB (alpha in the top byte) so a transparent window shows through where the
                // renderer left the pixel empty (α≈0) — the floating-caption look on backends that
                // honor per-pixel alpha (macOS CoreGraphics). Opaque backends (X11) ignore the top byte.
                let a = rgba[i * 4 + 3] as u32;
                let (r, g, b) = (
                    rgba[i * 4] as u32,
                    rgba[i * 4 + 1] as u32,
                    rgba[i * 4 + 2] as u32,
                );
                *px = (a << 24) | (r << 16) | (g << 8) | b;
            }
            buffer.present().ok();
        }
    }
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn run(
    rec: StreamingRecognizer,
    rx: Receiver<Vec<f32>>,
    rescorer: Option<StaticRescorer>,
    style: OverlayStyle,
) -> Result<(), Box<dyn std::error::Error>> {
    let transcript: Shared = Arc::new(Mutex::new(TranscriptState::default()));
    if let Ok(mut ts) = transcript.lock() {
        ts.apply(&CaptionEvent {
            text: "گوش می‌دهم…".to_string(),
            is_final: false,
        });
    }
    let cap = match rescorer {
        Some(r) => LiveCaptioner::with_static_guide(rec, r),
        None => LiveCaptioner::new(rec),
    }
    .with_max_active_seconds(style.rolling_window_seconds);
    {
        let transcript = transcript.clone();
        std::thread::spawn(move || caption_worker(rx, cap, transcript));
    }
    let event_loop = EventLoop::new()?;
    let mut app = Overlay {
        window: None,
        surface: None,
        renderer: CaptionRenderer::new(),
        animation: AnimationState::new(),
        transcript,
        style: style.clamp(),
        active: Arc::new(AtomicBool::new(true)),
        standby: None,
        last_visible: true,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn run_standby(
    rec: StreamingRecognizer,
    rescorer: Option<StaticRescorer>,
    style: OverlayStyle,
    device: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let transcript: Shared = Arc::new(Mutex::new(TranscriptState::default()));
    let cap = match rescorer {
        Some(r) => LiveCaptioner::with_static_guide(rec, r),
        None => LiveCaptioner::new(rec),
    }
    .with_max_active_seconds(style.rolling_window_seconds);
    let active = Arc::new(AtomicBool::new(false));
    let ctrl = Arc::new(Mutex::new(StandbyControl {
        window: None,
        visible: false,
        quit: false,
    }));
    let (tx, rx) = std::sync::mpsc::channel();
    let _capture = crate::audio::start(tx, device)?; // streams while standby; worker drains when idle
    {
        let transcript = transcript.clone();
        let active = active.clone();
        std::thread::spawn(move || caption_worker_standby(rx, cap, transcript, active));
    }
    // stdin command channel: show | hide | quit  (closed / EOF => commands stop, child stays alive)
    {
        let ctrl = ctrl.clone();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines().map_while(Result::ok) {
                let mut c = ctrl.lock().unwrap();
                match line.trim() {
                    "show" | "start" | "go" => c.visible = true,
                    "hide" | "stop" | "pause" => c.visible = false,
                    "quit" | "exit" => {
                        c.quit = true;
                        break;
                    }
                    _ => {}
                }
            }
        });
    }
    eprintln!("[overlay] standby ready: model loaded, window hidden; awaiting show/hide/quit on stdin");
    let event_loop = EventLoop::new()?;
    let mut app = Overlay {
        window: None,
        surface: None,
        renderer: CaptionRenderer::new(),
        animation: AnimationState::new(),
        transcript,
        style: style.clamp(),
        active,
        standby: Some(ctrl),
        last_visible: false,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn run_static_caption(
    text: &str,
    style: OverlayStyle,
) -> Result<(), Box<dyn std::error::Error>> {
    let transcript: Shared = Arc::new(Mutex::new(TranscriptState::default()));
    if let Ok(mut ts) = transcript.lock() {
        ts.apply(&CaptionEvent {
            text: text.to_string(),
            is_final: true,
        });
    }
    let event_loop = EventLoop::new()?;
    let mut app = Overlay {
        window: None,
        surface: None,
        renderer: CaptionRenderer::new(),
        animation: AnimationState::new(),
        transcript,
        style: style.clamp(),
        active: Arc::new(AtomicBool::new(true)),
        standby: None,
        last_visible: true,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn run(
    rec: StreamingRecognizer,
    rx: Receiver<Vec<f32>>,
    rescorer: Option<StaticRescorer>,
    style: OverlayStyle,
) -> Result<(), Box<dyn std::error::Error>> {
    let transcript: Shared = Arc::new(Mutex::new(TranscriptState::default()));
    let cap = match rescorer {
        Some(r) => LiveCaptioner::with_static_guide(rec, r),
        None => LiveCaptioner::new(rec),
    }
    .with_max_active_seconds(style.rolling_window_seconds);
    {
        let transcript = transcript.clone();
        std::thread::spawn(move || caption_worker(rx, cap, transcript));
    }
    run_gtk_overlay(transcript, style)
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn run_static_caption(
    text: &str,
    style: OverlayStyle,
) -> Result<(), Box<dyn std::error::Error>> {
    let transcript: Shared = Arc::new(Mutex::new(TranscriptState::default()));
    if let Ok(mut ts) = transcript.lock() {
        ts.apply(&CaptionEvent {
            text: text.to_string(),
            is_final: true,
        });
    }
    run_gtk_overlay(transcript, style)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run_gtk_overlay(
    transcript: Shared,
    style: OverlayStyle,
) -> Result<(), Box<dyn std::error::Error>> {
    use gtk::cairo::Operator;
    use gtk::glib;
    use gtk::prelude::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    gtk::init()?;

    let style = style.clamp();
    let (win_w, win_h) = overlay_size(&style);
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Shenava");
    window.set_decorated(false);
    window.set_resizable(true);
    window.set_app_paintable(true);
    window.set_keep_above(true);
    window.set_default_size(win_w, win_h);
    window.set_accept_focus(false);
    window.set_focus_on_map(false);
    window.set_skip_taskbar_hint(true);
    window.set_skip_pager_hint(true);

    if let Some(screen) = gtk::prelude::WidgetExt::screen(&window) {
        if let Some(visual) = screen.rgba_visual() {
            window.set_visual(Some(&visual));
        }
    }

    if let Some(display) = gtk::gdk::Display::default() {
        if let Some(monitor) = display.primary_monitor().or_else(|| display.monitor(0)) {
            let geometry = monitor.geometry();
            let x = geometry.x() + ((geometry.width() - win_w) / 2).max(0);
            let margin = (geometry.height() as f32 * 0.03) as i32;
            let min_y = geometry.y() + margin;
            let max_y = geometry.y() + (geometry.height() - win_h - margin).max(0);
            let y = min_y + ((max_y - min_y) as f32 * (1.0 - style.vertical_position)) as i32;
            window.move_(x, y);
        }
    }

    let area = gtk::DrawingArea::new();
    area.set_size_request(win_w, win_h);
    window.add(&area);

    let renderer = Rc::new(RefCell::new(CaptionRenderer::new()));
    let animation = Rc::new(RefCell::new(AnimationState::new()));
    let draw_style = style.clone();
    let draw_transcript = transcript.clone();
    let draw_animation = animation.clone();
    area.connect_draw(move |area, cr| {
        let alloc = area.allocation();
        let w = alloc.width().max(1) as usize;
        let h = alloc.height().max(1) as usize;

        cr.set_operator(Operator::Clear);
        let _ = cr.paint();
        cr.set_operator(Operator::Over);

        let blocks = draw_transcript
            .lock()
            .ok()
            .map(|ts| ts.blocks(24))
            .unwrap_or_default();
        let text = renderer
            .borrow_mut()
            .rollup_text(&blocks, w, draw_style.font_size, &draw_style);
        let frame = draw_animation.borrow_mut().frame(text, &draw_style);

        if frame.text.trim().is_empty() {
            return glib::Propagation::Proceed;
        }

        let rgba = renderer.borrow_mut().render_with_style_offset(
            &frame.text,
            w,
            h,
            draw_style.font_size * frame.font_scale,
            (0, 0, 0, 0),
            &draw_style,
            frame.y_shift,
        );
        if let Ok(surface) = rgba_to_cairo_surface(&rgba, w, h) {
            if cr.set_source_surface(&surface, 0.0, 0.0).is_ok() {
                let _ = cr.paint();
            }
        }
        glib::Propagation::Proceed
    });

    let tick_area = area.clone();
    glib::timeout_add_local(Duration::from_millis(33), move || {
        tick_area.queue_draw();
        glib::ControlFlow::Continue
    });

    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        glib::Propagation::Proceed
    });
    window.show_all();
    gtk::main();
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn rgba_to_cairo_surface(
    rgba: &[u8],
    w: usize,
    h: usize,
) -> Result<gtk::cairo::ImageSurface, Box<dyn std::error::Error>> {
    use gtk::cairo::{Format, ImageSurface};

    let mut surface = ImageSurface::create(Format::ARgb32, w as i32, h as i32)?;
    let stride = surface.stride() as usize;
    {
        let mut data = surface.data()?;
        for y in 0..h {
            for x in 0..w {
                let src = (y * w + x) * 4;
                let dst = y * stride + x * 4;
                let r = rgba[src] as u16;
                let g = rgba[src + 1] as u16;
                let b = rgba[src + 2] as u16;
                let a = rgba[src + 3] as u16;
                data[dst] = ((b * a + 127) / 255) as u8;
                data[dst + 1] = ((g * a + 127) / 255) as u8;
                data[dst + 2] = ((r * a + 127) / 255) as u8;
                data[dst + 3] = a as u8;
            }
        }
    }
    surface.mark_dirty();
    Ok(surface)
}

#[cfg(test)]
mod tests {
    use super::typewriter_base_chars;

    #[test]
    fn typewriter_keeps_existing_prefix_still() {
        let previous = "سلام دنیا";
        assert_eq!(
            typewriter_base_chars(previous, "سلام دنیا امروز"),
            previous.chars().count()
        );
    }

    #[test]
    fn typewriter_does_not_retype_after_rollup_replaces_prefix() {
        let current = "خط تازه در پنجره";
        assert_eq!(
            typewriter_base_chars("خط قدیمی", current),
            current.chars().count()
        );
    }
}
