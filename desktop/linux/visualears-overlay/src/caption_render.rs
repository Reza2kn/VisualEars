//! Caption rendering with cosmic-text — proper Arabic/Persian contextual shaping (rustybuzz) and
//! RTL/bidi layout, rasterized to an RGBA buffer. Works fully headless (no window), so the Persian
//! can be PNG-verified on a display-less box, the same way the macOS CaptionView was screenshotted.

use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Wrap,
};

use crate::style::OverlayStyle;

const VAZIRMATN: &[u8] = include_bytes!("../assets/fonts/Vazirmatn.ttf");
const CREAM: (u8, u8, u8) = (255, 247, 224);
const SHADOW: (u8, u8, u8) = (30, 16, 22);

pub struct CaptionRenderer {
    font_system: FontSystem,
    cache: SwashCache,
}

impl Default for CaptionRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptionRenderer {
    pub fn new() -> Self {
        let mut font_system = FontSystem::new();
        font_system.db_mut().load_font_data(VAZIRMATN.to_vec());
        Self {
            font_system,
            cache: SwashCache::new(),
        }
    }

    /// Linux port of macOS `CaptionLayout.visualLines` + `CaptionLayout.window`.
    /// Each finalized/live block is greedily width-wrapped in isolation. Appending a word can
    /// therefore affect only the final visual line, while the selected newest N lines roll upward
    /// as complete, stable units.
    pub fn rollup_text(
        &mut self,
        blocks: &[String],
        viewport_width: usize,
        font_px: f32,
        style: &OverlayStyle,
    ) -> String {
        let max_width = viewport_width as f32 * style.max_width.clamp(0.1, 1.0);
        let mut visual_lines = Vec::new();
        for block in blocks {
            visual_lines.extend(self.wrap_block(block, font_px, style.line_height, max_width));
        }
        let keep = style.visible_lines.max(1);
        let start = visual_lines.len().saturating_sub(keep);
        visual_lines[start..].join("\n")
    }

    fn wrap_block(
        &mut self,
        text: &str,
        font_px: f32,
        line_height: f32,
        max_width: f32,
    ) -> Vec<String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            return Vec::new();
        }
        let mut lines = Vec::new();
        let mut current = String::new();
        for word in words {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if current.is_empty()
                || self.measure_width(&candidate, font_px, line_height) <= max_width
            {
                current = candidate;
            } else {
                lines.push(current);
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        lines
    }

    fn measure_width(&mut self, text: &str, font_px: f32, line_height: f32) -> f32 {
        let metrics = Metrics::new(font_px, font_px * line_height.max(0.1));
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, None, None);
        buffer.set_wrap(&mut self.font_system, Wrap::None);
        let attrs = Attrs::new().family(Family::Name("Vazirmatn"));
        buffer.set_text(&mut self.font_system, text, attrs, Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);
        buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0, f32::max)
    }

    /// Render `text` centered into a `w×h` RGBA buffer. `bg` alpha 0 = transparent overlay;
    /// opaque bg is used for the headless PNG test.
    pub fn render(
        &mut self,
        text: &str,
        w: usize,
        h: usize,
        font_px: f32,
        bg: (u8, u8, u8, u8),
    ) -> Vec<u8> {
        self.render_with_style(text, w, h, font_px, bg, &OverlayStyle::default())
    }

    pub fn render_with_style(
        &mut self,
        text: &str,
        w: usize,
        h: usize,
        font_px: f32,
        bg: (u8, u8, u8, u8),
        style: &OverlayStyle,
    ) -> Vec<u8> {
        self.render_with_style_offset(text, w, h, font_px, bg, style, 0)
    }

    pub fn render_with_style_offset(
        &mut self,
        text: &str,
        w: usize,
        h: usize,
        font_px: f32,
        bg: (u8, u8, u8, u8),
        style: &OverlayStyle,
        y_shift: i32,
    ) -> Vec<u8> {
        let mut px = vec![0u8; w * h * 4];
        for i in 0..w * h {
            px[i * 4] = bg.0;
            px[i * 4 + 1] = bg.1;
            px[i * 4 + 2] = bg.2;
            px[i * 4 + 3] = bg.3;
        }

        let metrics = Metrics::new(font_px, font_px * style.line_height.max(0.1));
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        let text_w = w as f32 * style.max_width.clamp(0.1, 1.0);
        // `rollup_text` has already selected and prewrapped exactly N visual lines using this same
        // font and width. Disable a second wrap pass so those stable lines remain immutable.
        buffer.set_size(&mut self.font_system, Some(text_w), Some(h as f32));
        buffer.set_wrap(&mut self.font_system, Wrap::None);
        let attrs = Attrs::new().family(Family::Name("Vazirmatn"));
        buffer.set_text(&mut self.font_system, text, attrs, Shaping::Advanced);
        for line in buffer.lines.iter_mut() {
            // AppKit `.natural` anchors Persian at the physical right edge. cosmic-text's `End`
            // is the logical end (left for RTL), so use physical Right to match macOS here.
            line.set_align(Some(Align::Right));
        }
        buffer.shape_until_scroll(&mut self.font_system, false);

        let n_runs = buffer.layout_runs().count().max(1) as f32;
        let y_off = ((h as f32 - n_runs * metrics.line_height) / 2.0).max(0.0) as i32 + y_shift;
        let x_off = ((w as f32 - text_w) / 2.0).max(0.0) as i32;
        let shadow_lift = style.shadow_lift.round().clamp(-12.0, 18.0) as i32;
        let shadow_blur = style.shadow_blur.round().clamp(0.0, 44.0) as i32;
        let shadow_alpha = (style.shadow_opacity.clamp(0.0, 1.0) * 126.0).round() as u8;
        let blur_step = (shadow_blur / 5).max(1);
        let shadow_passes = [
            (0, shadow_lift + shadow_lift.signum(), shadow_alpha),
            (-blur_step, shadow_lift, shadow_alpha.saturating_sub(40)),
            (blur_step, shadow_lift, shadow_alpha.saturating_sub(40)),
            (0, shadow_lift - blur_step, shadow_alpha.saturating_sub(58)),
            (0, shadow_lift + blur_step, shadow_alpha.saturating_sub(58)),
            (
                -blur_step,
                shadow_lift + blur_step,
                shadow_alpha.saturating_sub(72),
            ),
            (
                blur_step,
                shadow_lift + blur_step,
                shadow_alpha.saturating_sub(72),
            ),
        ];
        for (dx, dy, alpha) in shadow_passes {
            draw_buffer(
                &mut buffer,
                &mut self.font_system,
                &mut self.cache,
                &mut px,
                w,
                h,
                x_off + dx,
                y_off + dy,
                Color::rgba(SHADOW.0, SHADOW.1, SHADOW.2, alpha),
            );
        }
        draw_buffer(
            &mut buffer,
            &mut self.font_system,
            &mut self.cache,
            &mut px,
            w,
            h,
            x_off,
            y_off,
            Color::rgb(CREAM.0, CREAM.1, CREAM.2),
        );
        px
    }
}

fn draw_buffer(
    buffer: &mut Buffer,
    font_system: &mut FontSystem,
    cache: &mut SwashCache,
    px: &mut [u8],
    w: usize,
    h: usize,
    x_off: i32,
    y_off: i32,
    color: Color,
) {
    buffer.draw(font_system, cache, color, |x, y, cw, ch, color| {
        let a = color.a() as f32 / 255.0;
        if a <= 0.0 {
            return;
        }
        for dy in 0..ch as i32 {
            for dx in 0..cw as i32 {
                let px_x = x + dx + x_off;
                let px_y = y + dy + y_off;
                if px_x < 0 || px_y < 0 || px_x >= w as i32 || px_y >= h as i32 {
                    continue;
                }
                let idx = (px_y as usize * w + px_x as usize) * 4;
                let src = [color.r(), color.g(), color.b()];
                for c in 0..3 {
                    px[idx + c] = (src[c] as f32 * a + px[idx + c] as f32 * (1.0 - a)) as u8;
                }
                px[idx + 3] = px[idx + 3].max((a * 255.0) as u8);
            }
        }
    });
}

pub fn save_png(rgba: &[u8], w: u32, h: u32, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    image::save_buffer(path, rgba, w, h, image::ExtendedColorType::Rgba8)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CaptionRenderer;
    use crate::style::OverlayStyle;

    #[test]
    fn mac_style_rollup_keeps_exact_newest_visual_line_count_and_tail() {
        let mut renderer = CaptionRenderer::new();
        let mut style = OverlayStyle::default();
        style.visible_lines = 2;
        style.max_width = 0.5;
        let text = renderer.rollup_text(
            &["این یک جمله بلند است که باید پیوسته به خط بعدی حرکت کند".into()],
            520,
            64.0,
            &style,
        );
        assert!(text.lines().count() <= 2);
        assert!(text.ends_with("حرکت کند"));
    }
}
