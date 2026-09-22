use crate::theme::{
    self, FIGURE_FONT, LEGEND, LEGEND_FONT, LEGEND_SIZE, READOUT, TRACE, VALUE, VALUE_SIZE,
};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontFamily, FontId, RichText, Sense, Stroke, Vec2};

const TRACKING: f32 = 1.5;
const SPAN_GAP: f32 = 8.0;
const LINE_H: f32 = 18.0;
const LINE_BASELINE: f32 = 13.0;

pub fn legend(text: impl Into<String>) -> RichText {
    RichText::new(text.into().to_uppercase())
        .font(theme::legend_font(LEGEND_SIZE))
        .extra_letter_spacing(TRACKING)
        .color(LEGEND)
}

pub fn value(text: impl Into<String>) -> RichText {
    RichText::new(text).font(theme::figure(VALUE_SIZE)).color(VALUE)
}

pub fn action(text: impl Into<String>) -> RichText {
    RichText::new(text).font(theme::legend_font(VALUE_SIZE + 0.5)).color(VALUE)
}

pub fn legend_job(text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut f = Line::face(LEGEND_FONT, LEGEND_SIZE, LEGEND);
    f.extra_letter_spacing = TRACKING;
    job.append(&text.to_uppercase(), 0.0, f);
    job
}

#[derive(Default)]
pub struct Line {
    job: LayoutJob,
    gap: Option<f32>,
}

impl Line {
    pub fn new() -> Self {
        Self::default()
    }

    fn face(name: &'static str, size: f32, colour: Color32) -> TextFormat {
        TextFormat {
            font_id: FontId::new(size, FontFamily::Name(name.into())),
            color: colour,
            ..Default::default()
        }
    }

    fn add(mut self, text: impl Into<String>, format: TextFormat) -> Self {
        let lead = match self.job.sections.is_empty() {
            true => 0.0,
            false => self.gap.take().unwrap_or(SPAN_GAP),
        };
        self.job.append(&text.into(), lead, format);
        self
    }

    pub fn legend(self, text: &str) -> Self {
        let mut f = Self::face(LEGEND_FONT, LEGEND_SIZE, LEGEND);
        f.extra_letter_spacing = TRACKING;
        self.add(text.to_uppercase(), f)
    }

    pub fn value(self, text: impl Into<String>) -> Self {
        self.add(text, Self::face(FIGURE_FONT, VALUE_SIZE, VALUE))
    }

    pub fn set(self, text: impl Into<String>) -> Self {
        self.add(text, Self::face(FIGURE_FONT, VALUE_SIZE, READOUT))
    }

    pub fn measured(self, text: impl Into<String>) -> Self {
        self.add(text, Self::face(FIGURE_FONT, VALUE_SIZE, TRACE))
    }

    pub fn note(self, text: impl Into<String>) -> Self {
        self.add(
            text,
            TextFormat { font_id: FontId::proportional(12.0), color: LEGEND, ..Default::default() },
        )
    }

    pub fn gap(mut self, px: f32) -> Self {
        self.gap = Some(px);
        self
    }

    pub fn column(mut self, ui: &egui::Ui, x: f32) -> Self {
        let so_far = ui.ctx().fonts_mut(|f| f.layout_job(self.job.clone()).size().x);
        self.gap = Some((x - so_far).max(SPAN_GAP));
        self
    }

    pub fn tint(mut self, colour: Color32) -> Self {
        if let Some(s) = self.job.sections.last_mut() {
            s.format.color = colour;
        }
        self
    }

    pub fn size(mut self, px: f32) -> Self {
        if let Some(s) = self.job.sections.last_mut() {
            s.format.font_id.size = px;
        }
        self
    }

    pub fn job(self) -> LayoutJob {
        self.job
    }

    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let galley = ui.fonts_mut(|f| f.layout_job(self.job));
        let baseline = galley
            .rows
            .first()
            .map(|r| r.pos.y + r.glyphs.iter().map(|g| g.pos.y).fold(0.0f32, f32::max))
            .unwrap_or(LINE_BASELINE);
        let size = galley.size();
        let lift = (LINE_BASELINE - baseline).max(0.0);
        let h = LINE_H.max(size.y + lift);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(size.x, h), Sense::hover());
        if ui.is_rect_visible(rect) {
            ui.painter().galley(egui::pos2(rect.left(), rect.top() + lift), galley, VALUE);
        }
        resp
    }

    pub fn elided(mut self, ui: &mut egui::Ui) -> egui::Response {
        self.job.wrap.max_width = ui.available_width();
        self.job.wrap.max_rows = 1;
        self.job.wrap.overflow_character = Some('…');
        self.show(ui)
    }

    pub fn wrapped(mut self, ui: &mut egui::Ui) -> egui::Response {
        self.job.wrap.max_width = ui.available_width();
        ui.add(egui::Label::new(self.job).wrap())
    }
}

pub fn note(ui: &mut egui::Ui, text: impl Into<String>, colour: Color32) {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.add(egui::Label::new(RichText::new(text).size(11.5).color(colour)).wrap());
        },
    );
}

pub fn hint(ui: &mut egui::Ui, text: &str) {
    Line::new().note(text).size(10.5).wrapped(ui);
}

pub fn help(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    let col = if r.hovered() { READOUT } else { LEGEND };
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.circle_stroke(rect.center(), 6.0, Stroke::new(1.0, col));
        p.text(rect.center(), egui::Align2::CENTER_CENTER, "?", theme::legend_font(10.0), col);
    }
    r.on_hover_text(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_of_different_sizes_share_a_baseline() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let _ = ctx.run_ui(Default::default(), |_| {});
        let job = Line::new().legend("squelch").value("-42 dBFS").size(11.0).job;
        let galley = ctx.fonts_mut(|f| f.layout_job(job));
        assert_eq!(galley.rows.len(), 1, "a short line wrapped");
        let ys: Vec<f32> = galley.rows[0].glyphs.iter().map(|g| g.pos.y).collect();
        let lo = ys.iter().cloned().fold(f32::MAX, f32::min);
        let hi = ys.iter().cloned().fold(f32::MIN, f32::max);
        assert!(hi - lo <= 1.0, "baselines differ by {:.2} px", hi - lo);
    }

    #[test]
    fn a_column_lands_the_value_where_it_was_asked_for() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let _ = ctx.run_ui(Default::default(), |ui| {
            let job = Line::new().legend("mode").column(ui, 90.0).value("FM").job;
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            let x = galley.rows[0].glyphs.last().unwrap().pos.x;
            assert!(x >= 90.0, "value started at {x:.1}");
        });
    }
}
