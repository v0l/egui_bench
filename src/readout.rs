use crate::text::{Line, legend};
use crate::theme::{self, LEGEND, LEGEND_SIZE};
use egui::{Color32, Pos2, RichText, Sense, Ui};

pub const READOUT_W: f32 = 104.0;
pub const LABEL_W: f32 = 90.0;
const LABEL_GAP: f32 = 8.0;

pub fn readout(ui: &mut Ui, label: &str, v: impl Into<String>, tint: Color32) {
    let v = v.into();
    ui.allocate_ui_with_layout(
        egui::vec2(READOUT_W.min(crate::text::room(ui)), 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add(egui::Label::new(legend(label)).wrap());
            ui.add(egui::Label::new(RichText::new(v).font(theme::figure(17.0)).color(tint)).wrap());
        },
    );
}

pub fn readouts(ui: &mut Ui, items: &[(&str, String, Color32)]) {
    const GAP: f32 = 18.0;
    let per_row = ((ui.available_width() / (READOUT_W + GAP)).floor() as usize).max(1);
    for chunk in items.chunks(per_row) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (label, v, tint) in chunk {
                readout(ui, label, v.clone(), *tint);
            }
        });
        ui.add_space(4.0);
    }
}

pub fn hero(ui: &mut Ui, label: &str, v: &str, unit: &str, tint: Color32) {
    const SIZE: f32 = 42.0;
    let room = crate::text::room(ui);
    let one = |text: &str, font, ink| {
        let mut job = egui::text::LayoutJob::default();
        job.append(text, 0.0, egui::TextFormat { font_id: font, color: ink, ..Default::default() });
        job
    };
    // A number is not broken across lines. It shrinks to fit the room instead, and only wraps
    // once it is down to the smallest size that can still be read.
    let mut size = SIZE;
    let figure_g = loop {
        let galley = crate::text::lay(ui, one(v, theme::figure(size), tint), room);
        let wide =
            ui.fonts_mut(|f| f.layout_no_wrap(v.to_owned(), theme::figure(size), tint)).size().x;
        if wide <= room || size <= 16.0 {
            break galley;
        }
        size = (size * room / wide).floor().max(16.0).min(size - 1.0);
    };
    let unit_g = crate::text::lay(
        ui,
        one(&unit.to_uppercase(), theme::legend_font(14.0), tint.gamma_multiply(0.75)),
        room,
    );
    let label_g = crate::text::lay(
        ui,
        one(&label.to_uppercase(), theme::legend_font(LEGEND_SIZE), LEGEND),
        room,
    );
    // The unit sits beside the figure when both fit the width, and under it when they do not.
    let beside = figure_g.size().x + 5.0 + unit_g.size().x <= room;
    let w = if beside {
        figure_g.size().x + 5.0 + unit_g.size().x
    } else {
        figure_g.size().x.max(unit_g.size().x)
    };
    let under = if beside { 0.0 } else { unit_g.size().y };
    let h = figure_g.size().y + under + 1.0 + label_g.size().y;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w.max(label_g.size().x), h), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let base = rect.top() + figure_g.size().y;
    let unit_at = if beside {
        Pos2::new(
            rect.left() + figure_g.size().x + 5.0,
            base - unit_g.size().y - figure_g.size().y * 0.14,
        )
    } else {
        Pos2::new(rect.left(), base)
    };
    let p = ui.painter();
    p.galley(rect.left_top(), figure_g, tint);
    p.galley(unit_at, unit_g, tint);
    p.galley(Pos2::new(rect.left(), base + under + 1.0), label_g, LEGEND);
}

pub fn reading(ui: &mut Ui, label: &str, text: impl Into<String>) {
    Line::new().legend(label).hanging(ui, LABEL_W + LABEL_GAP, Line::new().value(text).size(11.0));
}
