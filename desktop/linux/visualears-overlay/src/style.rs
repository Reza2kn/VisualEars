#[derive(Debug, Clone)]
pub struct OverlayStyle {
    /// Text animation selection from the macOS UI.
    pub animation: CaptionAnimation,
    /// 0 = bottom, 1 = top. Mirrors macOS `CaptionStyle.verticalPosition`.
    pub vertical_position: f32,
    /// Caption font size in CSS/macOS-style points. The window renderer maps this directly to px.
    pub font_size: f32,
    /// Rendered text line height multiple. The cosmic-text renderer uses it for metrics.
    pub line_height: f32,
    /// Number of visible roll-up rows, clamped to 1...4 like macOS.
    pub visible_lines: usize,
    /// Fraction of the overlay window width used for text wrapping.
    pub max_width: f32,
    /// Soft shadow opacity for the text.
    pub shadow_opacity: f32,
    /// Approximate blur radius for the text shadow. Mirrors macOS `shadowRadius`.
    pub shadow_blur: f32,
    /// Vertical shadow offset. Positive values drop the shadow below the glyphs.
    pub shadow_lift: f32,
    /// Maximum live utterance window before forced finalization. Keeps noisy input bounded.
    pub rolling_window_seconds: f32,
}

#[derive(Debug, Clone, Copy)]
pub enum CaptionAnimation {
    Pop,
    Karaoke,
    Slide,
    Typewriter,
}

impl Default for OverlayStyle {
    fn default() -> Self {
        Self {
            animation: CaptionAnimation::Typewriter,
            vertical_position: 0.09,
            font_size: 64.0,
            line_height: 1.16,
            visible_lines: 2,
            max_width: 0.82,
            shadow_opacity: 0.95,
            shadow_blur: 20.0,
            shadow_lift: 5.0,
            rolling_window_seconds: 12.0,
        }
    }
}

impl OverlayStyle {
    pub fn clamp(mut self) -> Self {
        self.vertical_position = self.vertical_position.clamp(0.0, 1.0);
        self.font_size = self.font_size.clamp(38.0, 92.0);
        self.line_height = self.line_height.clamp(0.9, 1.8);
        self.visible_lines = self.visible_lines.clamp(1, 4);
        self.max_width = self.max_width.clamp(0.5, 0.94);
        self.shadow_opacity = self.shadow_opacity.clamp(0.0, 1.0);
        self.shadow_blur = self.shadow_blur.clamp(0.0, 44.0);
        self.shadow_lift = self.shadow_lift.clamp(-12.0, 18.0);
        self.rolling_window_seconds = self.rolling_window_seconds.clamp(4.0, 20.0);
        self
    }
}

pub fn parse_style_args(args: &[String]) -> OverlayStyle {
    let mut style = OverlayStyle::default();
    if let Some(value) = value_after(args, "--animation") {
        style.animation = match value.to_ascii_lowercase().as_str() {
            "pop" => CaptionAnimation::Pop,
            "karaoke" => CaptionAnimation::Karaoke,
            "type" | "typewriter" => CaptionAnimation::Typewriter,
            _ => CaptionAnimation::Slide,
        };
    }
    if let Some(value) = parse_after::<f32>(args, "--vertical-position") {
        style.vertical_position = value;
    }
    if let Some(value) = parse_after::<f32>(args, "--font-size") {
        style.font_size = value;
    }
    if let Some(value) = parse_after::<f32>(args, "--line-height") {
        style.line_height = value;
    }
    if let Some(value) = parse_after::<usize>(args, "--visible-lines") {
        style.visible_lines = value;
    }
    if let Some(value) = parse_after::<f32>(args, "--max-width") {
        style.max_width = value;
    }
    if let Some(value) = parse_after::<f32>(args, "--shadow-opacity") {
        style.shadow_opacity = value;
    }
    if let Some(value) = parse_after::<f32>(args, "--shadow-blur") {
        style.shadow_blur = value;
    }
    if let Some(value) = parse_after::<f32>(args, "--shadow-lift") {
        style.shadow_lift = value;
    }
    if let Some(value) = parse_after::<f32>(args, "--rolling-window") {
        style.rolling_window_seconds = value;
    }
    style.clamp()
}

fn value_after(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|x| x == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn parse_after<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    value_after(args, flag).and_then(|value| value.parse::<T>().ok())
}
